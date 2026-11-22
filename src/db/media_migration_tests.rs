//! schema_v85: time and origin of every document (`doc_time.rs`), the tables of time-based documents
//! (`media_documents`, `media_segments`) and the time range of a page image — on a fresh database and on
//! an existing v84 one; deleting a document takes its media rows with it, a deleted page image only
//! unhooks its segment.

use super::models::*;
use super::vision_tests::migrate_to;
use super::Database;

fn context(db: &Database) -> i64 {
    db.create_context(&NewContext { name: "media".into(), chunking_strategy: "structural".into(), ..Default::default() }).unwrap().id
}

fn doc(db: &Database, ctx: i64, name: &str) -> i64 {
    db.create_document(&NewDocument { context_id: ctx, name: name.into(), zip_entry: None, byte_size: None, page_count: None, content_hash: None, extracted_text: Some("x".into()) })
        .unwrap()
        .id
}

fn count(db: &Database, sql: &str) -> i64 {
    db.conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn time_and_origin_start_unknown_and_round_trip() {
    let db = Database::open_in_memory().unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);
    let id = doc(&db, context(&db), "talk.m4a");
    assert_eq!(db.document_time(id).unwrap(), Some(DocTime::default()));
    let t = DocTime { taken_at_ms: Some(1_763_820_180_000), tz_offset_min: Some(60), time_source: Some("quicktime".into()), origin: Some("uploaded".into()) };
    db.set_document_time(id, &t).unwrap();
    assert_eq!(db.document_time(id).unwrap(), Some(t));
    db.set_document_time(id, &DocTime { origin: Some("captured_in_matrix".into()), ..Default::default() }).unwrap();
    assert_eq!(db.document_time(id).unwrap().unwrap().taken_at_ms, None, "None clears a column");
    assert_eq!(db.document_time(id + 1).unwrap(), None);
}

#[test]
fn media_rows_go_with_their_document_and_a_page_image_only_unhooks() {
    let db = Database::open_in_memory().unwrap();
    let ctx = context(&db);
    let id = doc(&db, ctx, "talk.mp4");
    db.conn
        .execute_batch(&format!(
            "INSERT INTO media_documents (document_id, kind, duration_ms) VALUES ({id}, 'video', 60000);
             INSERT INTO page_images (context_id, document_id, page, width, height, image, t_start_ms, t_end_ms)
                 VALUES ({ctx}, {id}, 1, 4, 3, x'00', 1000, 9000);
             INSERT INTO media_segments (document_id, t_start_ms, t_end_ms, kind, text, page_image_id)
                 VALUES ({id}, 1000, 9000, 'slide', 'Agenda', (SELECT id FROM page_images WHERE document_id = {id}));"
        ))
        .unwrap();
    let (engines, speakers, phases, complete): (String, String, String, i64) = db
        .conn
        .query_row("SELECT engines, speakers, phases, complete FROM media_documents WHERE document_id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap();
    assert_eq!((engines.as_str(), speakers.as_str(), phases.as_str(), complete), ("{}", "[]", "{}", 0));

    db.conn.execute("DELETE FROM page_images WHERE document_id = ?1", [id]).unwrap();
    assert_eq!(count(&db, "SELECT COUNT(*) FROM media_segments WHERE page_image_id IS NULL"), 1, "segment stays, unhooked");

    db.delete_document(id).unwrap();
    assert_eq!(count(&db, "SELECT COUNT(*) FROM media_documents"), 0);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM media_segments"), 0);
}

#[test]
fn v85_applies_on_an_existing_v84_database() {
    let conn = migrate_to(84);
    conn.execute_batch(
        "INSERT INTO contexts (id, name, chunking_strategy) VALUES (1, 'ctx', 'structural');
         INSERT INTO documents (id, context_id, name, extracted_text) VALUES (7, 1, 'old.pdf', 'x');
         INSERT INTO page_images (context_id, document_id, page, width, height, image) VALUES (1, 7, 1, 4, 3, x'00');",
    )
    .unwrap();
    conn.execute_batch(super::MIGRATIONS[84]).unwrap();
    let old: (Option<i64>, Option<String>) = conn.query_row("SELECT taken_at_ms, origin FROM documents WHERE id = 7", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(old, (None, None));
    let frame: (Option<i64>, Option<i64>) = conn.query_row("SELECT t_start_ms, t_end_ms FROM page_images", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(frame, (None, None), "pages of PDFs have no time");
    let idx: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'idx_media_segments_doc_time'", [], |r| r.get(0)).unwrap();
    assert_eq!(idx, 1);
}
