//! Paragraph vectors (schema_v77): they never enter the vector search, are read back in paragraph
//! order, follow the passage when it is deleted, and a model change removes them.

use super::models::*;
use super::Database;

fn setup(n: i64) -> Database {
    let db = Database::open_in_memory().unwrap();
    db.conn.execute("INSERT INTO contexts (id, name, chunking_strategy) VALUES (1, 'c', 'structural')", []).unwrap();
    db.conn
        .execute("INSERT INTO embedding_models (id, identifier, kind, native_dim, default_dim) VALUES (1, 'm', 'local_onnx', 2, 2)", [])
        .unwrap();
    db.conn.execute("UPDATE contexts SET embedding_model_id = 1 WHERE id = 1", []).unwrap();
    for id in 1..=n {
        db.conn.execute("INSERT INTO documents (id, context_id, name) VALUES (?1, 1, 'd')", [id]).unwrap();
        db.conn
            .execute(
                "INSERT INTO chunks (id, context_id, document_id, chunk_index, char_start, char_end, text) VALUES (?1, 1, ?1, 0, 0, 9, 'para one\n\npara two')",
                [id],
            )
            .unwrap();
    }
    db
}

fn embed(db: &Database, chunk: i64, v: [f32; 2]) {
    db.insert_embedding(&NewEmbedding { chunk_id: chunk, context_id: 1, document_id: chunk, embedding_model_id: 1, dim: 2, vector: v.to_vec() }).unwrap();
}

fn chunk(db: &Database, id: i64) -> Chunk {
    db.chunk(id).unwrap().unwrap()
}

fn hits(db: &Database, q: [f32; 2]) -> Vec<(i64, f32)> {
    db.search_context(1, &q, 10, None).unwrap().into_iter().map(|h| (h.chunk_id, h.score)).collect()
}

fn count(db: &Database) -> i64 {
    db.conn.query_row("SELECT COUNT(*) FROM embedding_paragraphs", [], |r| r.get(0)).unwrap()
}

#[test]
fn paragraph_vectors_are_read_back_but_never_searched() {
    let db = setup(2);
    embed(&db, 1, [1.0, 0.0]);
    embed(&db, 2, [0.8, 0.6]);
    let before = hits(&db, [0.0, 1.0]);
    db.set_paragraph_vectors(&chunk(&db, 1), 1, &[(0, 8, vec![0.0, 1.0]), (10, 18, vec![1.0, 0.0])]).unwrap();
    assert_eq!(hits(&db, [0.0, 1.0]), before, "the vector search reads only the passages' own vectors");
    let parts = db.paragraph_vectors(&[1, 2]).unwrap();
    assert_eq!(parts.iter().map(|p| (p.chunk_id, p.start, p.end, p.embedding_model_id)).collect::<Vec<_>>(), vec![(1, 0, 8, 1), (1, 10, 18, 1)]);
    assert_eq!(parts[0].vector, vec![0.0, 1.0]);
}

#[test]
fn rows_go_with_the_passage_and_with_a_model_change() {
    let db = setup(2);
    embed(&db, 1, [1.0, 0.0]);
    db.set_paragraph_vectors(&chunk(&db, 1), 1, &[(0, 8, vec![0.0, 1.0]), (10, 18, vec![1.0, 0.0])]).unwrap();
    db.set_paragraph_vectors(&chunk(&db, 2), 1, &[(0, 8, vec![0.0, 1.0]), (10, 18, vec![1.0, 0.0])]).unwrap();
    assert_eq!(count(&db), 4);
    db.set_paragraph_vectors(&chunk(&db, 2), 1, &[]).unwrap();
    assert_eq!(count(&db), 2, "replacing with none removes them");
    db.delete_chunks_for_document(1).unwrap();
    assert_eq!(count(&db), 0, "deleted with the chunk");
    db.set_paragraph_vectors(&chunk(&db, 2), 1, &[(0, 8, vec![0.0, 1.0]), (10, 18, vec![1.0, 0.0])]).unwrap();
    db.delete_embeddings_for_context(1).unwrap();
    assert_eq!(count(&db), 0, "a model change drops them with the passage vectors");
}
