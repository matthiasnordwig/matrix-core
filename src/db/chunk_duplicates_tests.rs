//! Repeated passages: stored once for search, handed over when the standing one goes.

use super::models::{NewChunk, NewDocument, NewEmbedding};
use super::Database;

fn chunk(context_id: i64, document_id: i64, text: &str) -> NewChunk {
    NewChunk {
        context_id, document_id, chunk_index: 0, char_start: 0, char_end: text.len() as i64,
        text: text.into(), signature: None, is_omitted: false, metadata: "{}".into(),
    }
}

fn setup() -> (Database, i64, i64, i64) {
    let db = Database::open_in_memory().unwrap();
    let (ctx, _) = crate::db::chunks::chunks_tests::seed(&db);
    let (a, b) = (doc(&db, ctx, "a.eml"), doc(&db, ctx, "b.eml"));
    (db, ctx, a, b)
}

fn doc(db: &Database, context_id: i64, name: &str) -> i64 {
    db.create_document(&NewDocument {
        context_id, name: name.into(), zip_entry: None, byte_size: None, page_count: None, content_hash: None, extracted_text: None,
    }).unwrap().id
}

#[test]
fn second_passage_of_a_key_is_omitted_and_points_to_the_first() {
    let (db, ctx, a, b) = setup();
    let first = db.create_chunk_once(&chunk(ctx, a, "text"), Some("k")).unwrap();
    let second = db.create_chunk_once(&chunk(ctx, b, "text"), Some("k")).unwrap();
    assert!(!first.is_omitted);
    assert!(second.is_omitted);
    assert_eq!(db.duplicate_homes(&[first.id, second.id]).unwrap()[&first.id], vec!["b.eml".to_string()]);
    assert_eq!(db.duplicate_homes(&[second.id]).unwrap().len(), 0);
}

#[test]
fn passages_without_a_key_stay_as_they_are() {
    let (db, ctx, a, b) = setup();
    db.create_chunk_once(&chunk(ctx, a, "text"), None).unwrap();
    assert!(!db.create_chunk_once(&chunk(ctx, b, "text"), None).unwrap().is_omitted);
}

#[test]
fn deleting_the_standing_passage_hands_vector_and_repeats_to_the_next() {
    let (db, ctx, a, b) = setup();
    let c_doc = doc(&db, ctx, "c.eml");
    let first = db.create_chunk_once(&chunk(ctx, a, "text"), Some("k")).unwrap();
    let second = db.create_chunk_once(&chunk(ctx, b, "text"), Some("k")).unwrap();
    let third = db.create_chunk_once(&chunk(ctx, c_doc, "text"), Some("k")).unwrap();
    db.insert_embedding(&NewEmbedding { chunk_id: first.id, context_id: ctx, document_id: a, embedding_model_id: db.context(ctx).unwrap().unwrap().embedding_model_id.unwrap(), dim: 2, vector: vec![1.0, 0.0] }).unwrap();
    db.delete_document(a).unwrap();
    let heir = db.chunk(second.id).unwrap().unwrap();
    assert!(!heir.is_omitted, "the text stays findable");
    assert_eq!(db.duplicate_homes(&[second.id]).unwrap()[&second.id], vec!["c.eml".to_string()]);
    assert!(db.chunk(third.id).unwrap().unwrap().is_omitted);
    assert_eq!(db.count_embeddings_for_context(ctx).unwrap(), 1, "no second embedding needed");
    // A later copy still finds the heir by its key.
    let d_doc = doc(&db, ctx, "d.eml");
    assert!(db.create_chunk_once(&chunk(ctx, d_doc, "text"), Some("k")).unwrap().is_omitted);
}
