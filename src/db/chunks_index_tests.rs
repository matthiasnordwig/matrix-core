//! Chunks of one document by index (schema_v73): the plan uses the new index, and the migration
//! applies on an existing v72 database without touching its rows.

use super::vision_tests::migrate_to;
use super::Database;

fn plan(conn: &rusqlite::Connection, sql: &str) -> String {
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(3)).unwrap();
    rows.map(|r| r.unwrap()).collect::<Vec<_>>().join("; ")
}

#[test]
fn lookups_by_document_use_the_index() {
    let db = Database::open_in_memory().unwrap();
    for sql in [
        "SELECT * FROM chunks WHERE document_id = 1 AND chunk_index = 2",
        "SELECT id, text FROM chunks WHERE document_id = 1 ORDER BY chunk_index",
    ] {
        let p = plan(&db.conn, sql);
        assert!(p.contains("idx_chunks_document"), "{sql}: {p}");
        assert!(!p.contains("TEMP B-TREE"), "{sql}: {p}");
    }
}

#[test]
fn v73_applies_on_an_existing_v72_database() {
    let conn = migrate_to(72);
    conn.execute_batch(
        "INSERT INTO contexts (id, name, chunking_strategy) VALUES (1, 'old', 'structural');
         INSERT INTO documents (id, context_id, name) VALUES (1, 1, 'a');
         INSERT INTO chunks (id, context_id, document_id, chunk_index, char_start, char_end, text)
         VALUES (7, 1, 1, 0, 0, 3, 'abc');",
    )
    .unwrap();
    let db = Database::init(conn).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);
    let text: String = db.conn.query_row("SELECT text FROM chunks WHERE id = 7", [], |r| r.get(0)).unwrap();
    assert_eq!(text, "abc");
    assert!(plan(&db.conn, "SELECT * FROM chunks WHERE document_id = 1").contains("idx_chunks_document"));
}
