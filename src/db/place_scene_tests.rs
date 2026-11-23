//! schema_v86: place and device of a document (`doc_place.rs`), scene labels of its pictures
//! (`scene_labels.rs`) and the search filter on both (`narrow_scope_with`).

use super::models::*;
use super::Database;

fn context(db: &Database, name: &str) -> i64 {
    db.create_context(&NewContext { name: name.into(), chunking_strategy: "structural".into(), ..Default::default() }).unwrap().id
}

fn doc(db: &Database, ctx: i64, name: &str) -> i64 {
    db.create_document(&NewDocument { context_id: ctx, name: name.into(), zip_entry: None, byte_size: None, page_count: None, content_hash: None, extracted_text: Some("x".into()) })
        .unwrap()
        .id
}

/// A page image of `doc` visible from `t0` to `t1` (ms; `None` = a photo), returns its id.
fn picture(db: &Database, ctx: i64, doc: i64, page: i64, t: Option<(i64, i64)>) -> i64 {
    let (t0, t1) = t.map_or((None, None), |(a, b)| (Some(a), Some(b)));
    db.conn
        .execute(
            "INSERT INTO page_images (context_id, document_id, page, width, height, image, t_start_ms, t_end_ms) VALUES (?1, ?2, ?3, 4, 3, x'00', ?4, ?5)",
            rusqlite::params![ctx, doc, page, t0, t1],
        )
        .unwrap();
    db.conn.last_insert_rowid()
}

#[test]
fn place_and_device_round_trip_and_wait_for_their_place() {
    let db = Database::open_in_memory().unwrap();
    let ctx = context(&db, "trip");
    let id = doc(&db, ctx, "IMG_0001.MOV");
    assert_eq!(db.document_place(id).unwrap(), Some(DocPlace::default()));
    let p = DocPlace { lat: Some(48.2082), lon: Some(16.3738), place_id: None, device: Some("Apple iPhone 15 Pro".into()) };
    db.set_document_place(id, &p).unwrap();
    assert_eq!(db.document_place(id).unwrap(), Some(p.clone()));
    assert_eq!(db.unplaced_documents(10).unwrap(), vec![(id, 48.2082, 16.3738)]);
    db.set_document_place_id(id, Some(2_761_369)).unwrap();
    assert!(db.unplaced_documents(10).unwrap().is_empty());
    assert_eq!(db.document_place(id).unwrap().unwrap(), DocPlace { place_id: Some(2_761_369), ..p });
    assert_eq!(db.context_place_ids(&[ctx]).unwrap(), vec![2_761_369]);
    assert_eq!(db.document_place(id + 1).unwrap(), None);
}

#[test]
fn labels_are_replaced_per_source_and_remembered_without_any() {
    let db = Database::open_in_memory().unwrap();
    let ctx = context(&db, "v");
    let id = doc(&db, ctx, "clip.mp4");
    let still = picture(&db, ctx, id, 1, Some((0, 5_000)));
    db.conn.execute("INSERT INTO media_segments (document_id, t_start_ms, t_end_ms, kind, text, page_image_id) VALUES (?1, 0, 5000, 'scene', '', ?2)", [id, still]).unwrap();
    assert_eq!(db.scene_still_ids(ctx).unwrap(), vec![still]);
    // A photo counts, a PDF's graphics page and the video still do not.
    let photo_doc = doc(&db, ctx, "IMG_1.jpg");
    let photo = picture(&db, ctx, photo_doc, 1, None);
    let pdf = db.create_document(&NewDocument { context_id: ctx, name: "r.pdf".into(), zip_entry: None, byte_size: None, page_count: Some(3), content_hash: None, extracted_text: Some("x".into()) }).unwrap().id;
    picture(&db, ctx, pdf, 2, None);
    assert_eq!(db.photo_picture_ids(ctx).unwrap(), vec![photo]);
    let model = 7i64;
    db.conn.execute("INSERT INTO embedding_models (id, identifier, kind, native_dim, default_dim) VALUES (7, 'm', 'local_onnx', 2, 2)", []).unwrap();
    db.upsert_page_image_embedding(still, model, &[0.6, 0.8]).unwrap();
    let todo = db.pictures_to_label_by_vectors(&[still], model).unwrap();
    assert_eq!(todo.len(), 1);
    assert_eq!((todo[0].0, todo[0].1), (still, id));
    db.set_scene_labels(still, id, super::scene_labels::SOURCE_VECTORS, &["outdoors", "water"], Some(model)).unwrap();
    assert!(db.pictures_to_label_by_vectors(&[still], model).unwrap().is_empty(), "done for this model");
    db.set_scene_labels(still, id, super::scene_labels::SOURCE_VECTORS, &["outdoors", "beach"], Some(model)).unwrap();
    // Vision found nothing: remembered all the same.
    assert_eq!(db.pictures_to_label_by_vision(&[still]).unwrap(), vec![(still, id)]);
    db.set_scene_labels(still, id, super::scene_labels::SOURCE_VISION, &[], None).unwrap();
    assert!(db.pictures_to_label_by_vision(&[still]).unwrap().is_empty());
    let labels = db.scene_labels_of(id, None).unwrap();
    assert_eq!(labels, vec![(still, "vectors".to_string(), "beach".to_string()), (still, "vectors".into(), "outdoors".into())]);
    assert_eq!(db.scene_labels_of(id, Some((1_000, 2_000))).unwrap().len(), 2, "still visible then");
    assert!(db.scene_labels_of(id, Some((6_000, 9_000))).unwrap().is_empty(), "not visible then");
    db.conn.execute("DELETE FROM page_images WHERE id = ?1", [still]).unwrap();
    assert!(db.scene_labels_of(id, None).unwrap().is_empty(), "labels go with the picture");
}

#[test]
fn the_filter_narrows_by_place_and_label_together_with_the_date() {
    let db = Database::open_in_memory().unwrap();
    let ctx = context(&db, "life");
    let other = context(&db, "other");
    let vienna_lake = doc(&db, ctx, "a.jpg");
    let vienna_office = doc(&db, ctx, "b.jpg");
    let berlin_lake = doc(&db, ctx, "c.jpg");
    let elsewhere = doc(&db, other, "d.jpg");
    for (id, place) in [(vienna_lake, 1), (vienna_office, 1), (berlin_lake, 2), (elsewhere, 1)] {
        db.set_document_place(id, &DocPlace { lat: Some(1.0), lon: Some(2.0), place_id: Some(place), device: None }).unwrap();
    }
    for (id, label) in [(vienna_lake, "water"), (vienna_office, "office"), (berlin_lake, "water")] {
        let p = picture(&db, ctx, id, 1, None);
        db.set_scene_labels(p, id, "vectors", &[label], Some(1)).unwrap();
    }
    db.set_document_meta(vienna_lake, &DocMeta { date: Some("2024-07-12".into()), ..Default::default() }).unwrap();
    let none = DocFilter::default();
    let at = |ids: Vec<i64>| PictureFilter { place_ids: Some(ids), scene: vec![] };
    assert_eq!(db.narrow_scope_with(&[ctx], None, &none, &PictureFilter::default()).unwrap(), None, "no filter, no scope");
    assert_eq!(db.narrow_scope_with(&[ctx], None, &none, &at(vec![1])).unwrap(), Some(vec![vienna_lake, vienna_office]));
    let water = PictureFilter { place_ids: None, scene: vec!["water".into()] };
    assert_eq!(db.narrow_scope_with(&[ctx], None, &none, &water).unwrap(), Some(vec![vienna_lake, berlin_lake]));
    let both = PictureFilter { place_ids: Some(vec![1]), scene: vec!["water".into()] };
    assert_eq!(db.narrow_scope_with(&[ctx], None, &none, &both).unwrap(), Some(vec![vienna_lake]));
    assert_eq!(db.narrow_scope_with(&[ctx], Some(vec![vienna_office]), &none, &both).unwrap(), Some(vec![]), "within the file choice");
    let summer = DocFilter { date_from: Some("2024-06".into()), date_to: Some("2024-08".into()), ..Default::default() };
    assert_eq!(db.narrow_scope_with(&[ctx], None, &summer, &water).unwrap(), Some(vec![vienna_lake]));
    assert_eq!(db.narrow_scope_with(&[ctx], None, &none, &at(vec![])).unwrap(), Some(vec![]), "an unknown place matches nothing");
}

#[test]
fn a_copied_collection_keeps_place_and_labels() {
    let db = Database::open_in_memory().unwrap();
    let src = context(&db, "src");
    let dst = context(&db, "dst");
    let id = doc(&db, src, "a.jpg");
    let place = DocPlace { lat: Some(47.0), lon: Some(11.0), place_id: Some(5), device: Some("Pixel 8".into()) };
    db.set_document_place(id, &place).unwrap();
    let p = picture(&db, src, id, 1, None);
    db.set_scene_labels(p, id, "vision", &["beach"], None).unwrap();
    db.copy_context_chunks(src, dst).unwrap();
    let copy: i64 = db.conn.query_row("SELECT id FROM documents WHERE context_id = ?1", [dst], |r| r.get(0)).unwrap();
    assert_eq!(db.document_place(copy).unwrap(), Some(place));
    let labels: Vec<(String, String)> = db.scene_labels_of(copy, None).unwrap().into_iter().map(|(_, s, l)| (s, l)).collect();
    assert_eq!(labels, vec![("vision".to_string(), "beach".to_string())]);
    let copied_picture: i64 = db.conn.query_row("SELECT id FROM page_images WHERE document_id = ?1", [copy], |r| r.get(0)).unwrap();
    assert!(db.pictures_to_label_by_vision(&[copied_picture]).unwrap().is_empty(), "Vision is not asked again");
}
