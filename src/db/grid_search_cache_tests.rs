//! schema_v80: `contexts.search_version` rises with every write that can change a search's result
//! (and only then), a rise or a deleted collection takes the collection's memory entries with it, and
//! the memory keeps under its cap by dropping the least recently used entries.

use super::Database;

fn db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.conn
        .execute_batch(
            "INSERT INTO embedding_models (id, identifier, kind, native_dim, default_dim) VALUES (1, 'm1', 'local_onnx', 2, 2);
             INSERT INTO embedding_models (id, identifier, kind, native_dim, default_dim) VALUES (2, 'm2', 'local_onnx', 2, 2);
             INSERT INTO contexts (id, name, chunking_strategy, embedding_model_id) VALUES (1, 'a', 'structural', 1);
             INSERT INTO contexts (id, name, chunking_strategy, embedding_model_id) VALUES (2, 'b', 'structural', 1);
             INSERT INTO documents (id, context_id, name) VALUES (1, 1, 'd1');
             INSERT INTO documents (id, context_id, name) VALUES (2, 2, 'd2');
             INSERT INTO chunks (id, context_id, document_id, chunk_index, char_start, char_end, text) VALUES (10, 1, 1, 0, 0, 3, 'abc');
             INSERT INTO chunks (id, context_id, document_id, chunk_index, char_start, char_end, text) VALUES (20, 2, 2, 0, 0, 3, 'xyz');",
        )
        .unwrap();
    db
}

fn version(db: &Database, ctx: i64) -> i64 {
    db.context_search_stamps(&[ctx]).unwrap()[0].search_version
}

/// `sql` raises collection 1's version (and leaves collection 2's alone).
fn raises(db: &Database, sql: &str) {
    let (before, other) = (version(db, 1), version(db, 2));
    db.conn.execute_batch(sql).unwrap();
    assert!(version(db, 1) > before, "{sql} must raise the version");
    assert_eq!(version(db, 2), other, "{sql} must not touch another collection");
}

fn keeps(db: &Database, sql: &str) {
    let before = version(db, 1);
    db.conn.execute_batch(sql).unwrap();
    assert_eq!(version(db, 1), before, "{sql} must not raise the version");
}

fn entry(db: &Database, key: &str, contexts: &[i64]) {
    db.grid_search_cache_put(&[(key.to_string(), "[]".to_string())], contexts, 100).unwrap();
}

fn has(db: &Database, key: &str) -> bool {
    db.grid_search_cache_get(&[key.to_string()]).unwrap().contains_key(key)
}

#[test]
fn passages_raise_the_version() {
    let db = db();
    raises(&db, "INSERT INTO chunks (id, context_id, document_id, chunk_index, char_start, char_end, text) VALUES (11, 1, 1, 1, 3, 6, 'def')");
    raises(&db, "UPDATE chunks SET text = 'abd' WHERE id = 10");
    raises(&db, "UPDATE chunks SET is_omitted = 1 WHERE id = 10");
    raises(&db, "UPDATE chunks SET duplicate_of = 11 WHERE id = 10");
    raises(&db, "DELETE FROM chunks WHERE id = 11");
}

#[test]
fn renumbering_and_notes_keep_the_version() {
    let db = db();
    keeps(&db, "UPDATE chunks SET chunk_index = 5 WHERE id = 10");
    keeps(&db, "UPDATE chunks SET metadata = '{\"document_title\":\"t\"}' WHERE id = 10");
    keeps(&db, "UPDATE chunks SET text = 'abc' WHERE id = 10");
    keeps(&db, "UPDATE contexts SET name = 'renamed', description = 'x' WHERE id = 1");
}

#[test]
fn vectors_raise_the_version() {
    let db = db();
    let add = "INSERT INTO embeddings (chunk_id, context_id, document_id, embedding_model_id, dim, vector) VALUES (10, 1, 1, 1, 2, x'0000803f00000000')";
    raises(&db, add);
    // Re-embedding replaces the row.
    raises(&db, &add.replace("INSERT", "INSERT OR REPLACE"));
    raises(&db, "DELETE FROM embeddings WHERE context_id = 1");
    raises(&db, "INSERT INTO embedding_paragraphs (chunk_id, part, context_id, document_id, embedding_model_id, start_byte, end_byte, vector) VALUES (10, 0, 1, 1, 1, 0, 3, x'00')");
    keeps(&db, "INSERT INTO page_images (id, context_id, document_id, page, width, height, image) VALUES (5, 1, 1, 1, 1, 1, x'00')");
    raises(&db, "INSERT INTO page_image_embeddings (page_image_id, context_id, embedding_model_id, dim, vector) VALUES (5, 1, 1, 2, x'00')");
    raises(&db, "DELETE FROM page_images WHERE id = 5");
}

#[test]
fn the_embedding_model_raises_the_version() {
    let db = db();
    raises(&db, "UPDATE contexts SET embedding_model_id = 2 WHERE id = 1");
    // An edit of the model both collections use raises both.
    let other = version(&db, 2);
    db.conn.execute_batch("UPDATE embedding_models SET query_prefix = 'q: ' WHERE id = 1").unwrap();
    assert!(version(&db, 2) > other);
}

#[test]
fn a_missing_file_raises_the_version() {
    let db = db();
    keeps(&db, "INSERT INTO source_files (document_id, context_id, path, byte_size, mtime_ns, sha256) VALUES (1, 1, '/a', 1, 1, 'h')");
    raises(&db, "UPDATE source_files SET missing_since = 5 WHERE document_id = 1");
    raises(&db, "UPDATE source_files SET missing_since = NULL WHERE document_id = 1");
}

#[test]
fn deleting_a_document_raises_the_version_through_the_cascade() {
    let db = db();
    raises(&db, "DELETE FROM documents WHERE id = 1");
}

#[test]
fn a_rise_drops_the_collections_entries_only() {
    let db = db();
    entry(&db, "in-a", &[1]);
    entry(&db, "in-a-and-b", &[1, 2]);
    entry(&db, "in-b", &[2]);
    db.conn.execute_batch("UPDATE chunks SET text = 'new' WHERE id = 10").unwrap();
    assert!(!has(&db, "in-a"));
    assert!(!has(&db, "in-a-and-b"), "an entry goes with any of its collections");
    assert!(has(&db, "in-b"));
}

#[test]
fn deleting_a_collection_drops_its_entries() {
    let db = db();
    entry(&db, "in-a", &[1]);
    entry(&db, "in-b", &[2]);
    db.delete_context(1).unwrap();
    assert!(!has(&db, "in-a"));
    assert!(has(&db, "in-b"));
    let links: i64 = db.conn.query_row("SELECT COUNT(*) FROM grid_search_cache_contexts WHERE context_id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(links, 0);
}

#[test]
fn a_reranker_edit_forgets_everything() {
    let db = db();
    db.conn.execute_batch("INSERT INTO reranker_models (id, name, kind) VALUES (1, 'r', 'local_onnx')").unwrap();
    entry(&db, "k", &[1]);
    db.conn.execute_batch("UPDATE reranker_models SET name = 'r2' WHERE id = 1").unwrap();
    assert_eq!(db.grid_search_cache_len().unwrap(), 0);
}

#[test]
fn the_cap_drops_the_least_recently_used() {
    let db = db();
    for k in ["k1", "k2", "k3"] {
        db.grid_search_cache_put(&[(k.into(), "[]".into())], &[1], 3).unwrap();
    }
    assert!(has(&db, "k1"), "reading k1 makes k2 the oldest");
    db.grid_search_cache_put(&[("k4".into(), "[]".into())], &[1], 3).unwrap();
    assert_eq!(db.grid_search_cache_len().unwrap(), 3);
    assert!(!has(&db, "k2"));
    for k in ["k1", "k3", "k4"] {
        assert!(has(&db, k), "{k}");
    }
    let links: i64 = db.conn.query_row("SELECT COUNT(*) FROM grid_search_cache_contexts", [], |r| r.get(0)).unwrap();
    assert_eq!(links, 3, "an evicted entry takes its links");
}

#[test]
fn storing_again_replaces_the_hits() {
    let db = db();
    db.grid_search_cache_put(&[("k".into(), "[1]".into())], &[1], 10).unwrap();
    db.grid_search_cache_put(&[("k".into(), "[2]".into())], &[1], 10).unwrap();
    assert_eq!(db.grid_search_cache_get(&["k".into()]).unwrap()["k"], "[2]");
    assert_eq!(db.grid_search_cache_len().unwrap(), 1);
}

#[test]
fn stamps_follow_the_given_order_and_skip_unknown_ids() {
    let db = db();
    let s = db.context_search_stamps(&[2, 99, 1]).unwrap();
    assert_eq!(s.iter().map(|s| s.context_id).collect::<Vec<_>>(), vec![2, 1]);
    assert_eq!(s[0].embedding_model_id, Some(1));
}

#[test]
fn v80_applies_on_an_existing_v79_database() {
    let conn = super::vision_tests::migrate_to(79);
    conn.execute_batch("INSERT INTO contexts (id, name, chunking_strategy) VALUES (1, 'old', 'structural');").unwrap();
    let db = Database::init(conn).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);
    assert_eq!(version(&db, 1), 0);
}
