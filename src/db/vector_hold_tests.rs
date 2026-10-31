//! The block of vectors: same scores as the plain cosine, held only from the threshold on, and never
//! stale after a write of this connection (cascades included) or of another one.

use std::collections::HashMap;

use super::embeddings::{cosine, vector_to_blob};
use super::models::*;
use super::Database;

const MODEL: i64 = 1;

fn add_context(db: &Database, id: i64) {
    db.conn.execute("INSERT INTO contexts (id, name, chunking_strategy) VALUES (?1, ?1, 'structural')", [id]).unwrap();
}

fn add_vector(db: &Database, context: i64, chunk: i64, vector: &[f32]) {
    db.conn
        .execute(
            "INSERT OR IGNORE INTO embedding_models (id, identifier, kind, native_dim, default_dim) VALUES (1, 'm', 'local_onnx', 4, 4)",
            [],
        )
        .unwrap();
    db.conn.execute("INSERT OR IGNORE INTO documents (id, context_id, name) VALUES (?1, ?2, 'd')", [chunk, context]).unwrap();
    db.conn
        .execute(
            "INSERT INTO chunks (id, context_id, document_id, chunk_index, char_start, char_end, text) VALUES (?1, ?2, ?1, 0, 0, 1, 'x')",
            [chunk, context],
        )
        .unwrap();
    let e = NewEmbedding {
        chunk_id: chunk,
        context_id: context,
        document_id: chunk,
        embedding_model_id: MODEL,
        dim: vector.len() as i64,
        vector: vector.to_vec(),
    };
    db.insert_embedding(&e).unwrap();
}

fn filled(id_base: i64, n: i64) -> Database {
    let db = Database::open_in_memory().unwrap();
    add_context(&db, 1);
    for i in 0..n {
        add_vector(&db, 1, id_base + i, &[1.0, i as f32, 0.5, -(i as f32)]);
    }
    db
}

fn held(db: &Database) -> Option<i64> {
    db.vectors.held.borrow().as_ref().map(|t| t.len() as i64)
}

fn ids(db: &Database, query: &[f32]) -> Vec<i64> {
    db.search_context(1, query, 10, None).unwrap().iter().map(|h| h.chunk_id).collect()
}

#[test]
fn scores_equal_the_cosine_of_the_stored_vectors() {
    let db = filled(10, 5);
    let query = [0.3, 0.1, -0.2, 0.9];
    let hits = db.search_context(1, &query, 10, None).unwrap();
    assert_eq!(hits.len(), 5);
    for h in hits {
        let stored = db.embedding_vector(h.chunk_id).unwrap().unwrap();
        assert_eq!(h.score, cosine(&query, &stored));
    }
    assert_eq!(db.search_context(1, &[0.0; 4], 1, None).unwrap()[0].score, 0.0, "zero query scores zero");
}

#[test]
fn a_held_block_gives_the_same_hits_as_a_fresh_read() {
    let db = filled(10, 5);
    let query = [0.3, 0.1, -0.2, 0.9];
    let fresh = db.search_context(1, &query, 3, Some(&[11, 13, 14])).unwrap();
    assert_eq!(held(&db), Some(5));
    let again = db.search_context(1, &query, 3, Some(&[11, 13, 14])).unwrap();
    assert_eq!(fresh.iter().map(|h| (h.chunk_id, h.score)).collect::<Vec<_>>(), again.iter().map(|h| (h.chunk_id, h.score)).collect::<Vec<_>>());
    assert_eq!(db.search_context(1, &query, 3, Some(&[])).unwrap().len(), 0);
}

#[test]
fn small_collections_hold_nothing_and_leave_a_large_block_alone() {
    let db = filled(10, 5);
    add_context(&db, 2);
    add_vector(&db, 2, 100, &[1.0, 0.0, 0.0, 0.0]);
    ids(&db, &[1.0, 0.0, 0.0, 0.0]);
    assert_eq!(held(&db), Some(5));
    assert_eq!(db.search_context(2, &[1.0, 0.0, 0.0, 0.0], 5, None).unwrap().len(), 1);
    assert_eq!(held(&db), Some(5), "the small one reads and forgets");
}

#[test]
fn every_write_to_the_vectors_ends_the_held_block() {
    let db = filled(10, 4);
    let query = [1.0, 0.0, 0.0, 0.0];
    assert_eq!(ids(&db, &query).len(), 4);

    add_vector(&db, 1, 50, &[9.0, 0.0, 0.0, 0.0]);
    assert_eq!(ids(&db, &query)[0], 50, "new vector");

    db.conn.execute("UPDATE embeddings SET vector = ?1 WHERE chunk_id = 10", [vector_to_blob(&[-1.0, 0.0, 0.0, 0.0])]).unwrap();
    assert_eq!(*ids(&db, &query).last().unwrap(), 10, "changed vector");

    let e = NewEmbedding { chunk_id: 10, context_id: 1, document_id: 10, embedding_model_id: MODEL, dim: 4, vector: vec![50.0, 0.0, 0.0, 0.0] };
    db.insert_embedding(&e).unwrap();
    let hits = db.search_context(1, &[1.0, 0.0, 0.0, 0.0], 1, None).unwrap();
    assert_eq!(hits[0].chunk_id, 10, "replaced vector");

    db.delete_document(10).unwrap();
    assert!(!ids(&db, &query).contains(&10), "cascade from the document");

    db.delete_embeddings_for_context(1).unwrap();
    assert!(ids(&db, &query).is_empty(), "model change");
}

#[test]
fn a_write_of_another_connection_ends_the_held_block() {
    let dir = std::env::temp_dir().join(format!("matrix-vector-hold-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.db");
    let db = Database::open(&path).unwrap();
    add_context(&db, 1);
    for i in 0..4 {
        add_vector(&db, 1, 10 + i, &[1.0, i as f32, 0.0, 0.0]);
    }
    let query = [1.0, 0.0, 0.0, 0.0];
    assert_eq!(ids(&db, &query).len(), 4);

    let other = rusqlite::Connection::open(&path).unwrap();
    other.execute("DELETE FROM embeddings WHERE chunk_id = 10", []).unwrap();
    assert_eq!(ids(&db, &query).len(), 3);
    drop((db, other));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn nothing_uncommitted_is_held() {
    let mut db = filled(10, 4);
    let query = [1.0, 0.0, 0.0, 0.0];
    let tx = db.conn.transaction().unwrap();
    tx.execute("DELETE FROM embeddings WHERE chunk_id = 10", []).unwrap();
    tx.rollback().unwrap();
    assert_eq!(ids(&db, &query).len(), 4);

    db.conn.execute_batch("BEGIN; DELETE FROM embeddings WHERE chunk_id = 10;").unwrap();
    assert_eq!(ids(&db, &query).len(), 3);
    assert_eq!(held(&db), None, "read inside a transaction");
    db.conn.execute_batch("ROLLBACK;").unwrap();
    assert_eq!(ids(&db, &query).len(), 4);
}

#[test]
fn different_lengths_fail_as_before_but_only_inside_the_filter() {
    let db = filled(10, 2);
    add_vector(&db, 1, 50, &[1.0, 2.0]);
    let err = db.search_context(1, &[1.0, 0.0, 0.0, 0.0], 5, None).unwrap_err();
    assert!(err.to_string().contains('2'), "{err}");
    assert_eq!(db.search_context(1, &[1.0, 0.0, 0.0, 0.0], 5, Some(&[10, 11])).unwrap().len(), 2);
}

#[test]
fn the_batch_search_reads_the_same_block() {
    let db = filled(10, 5);
    let query = vec![0.3, 0.1, -0.2, 0.9];
    db.conn.execute("UPDATE contexts SET embedding_model_id = 1 WHERE id = 1", []).unwrap();
    let row: HashMap<i64, Vec<f32>> = [(MODEL, query.clone())].into();
    let batch = db.retrieve_batch(&[1], &[row], 3, None).unwrap();
    let single = db.search_context(1, &query, 3, None).unwrap();
    assert_eq!(batch[0].iter().map(|h| (h.chunk_id, h.score)).collect::<Vec<_>>(), single.iter().map(|h| (h.chunk_id, h.score)).collect::<Vec<_>>());
}
