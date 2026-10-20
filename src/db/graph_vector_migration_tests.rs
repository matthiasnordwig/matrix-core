//! schema_v67: graph vectors get their embedding model (`vector_model_id`).
//! The backfill keeps a node vector only where it is provably from the
//! context's current model (node created at/after the oldest chunk vector of
//! that model) and drops the rest; community vectors are all dropped (the
//! summary cache copies them into fresh rows, so their age proves nothing).

use super::Database;
use rusqlite::Connection;

/// A connection migrated through v66 (same hooks as `Database::migrate`).
fn migrate_to_66() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    for (i, sql) in super::MIGRATIONS[..66].iter().enumerate() {
        let target = (i + 1) as i64;
        if target == 51 {
            super::migrate_v50_to_v51_embedding_kind(&mut conn).unwrap();
            continue;
        }
        if target == 52 {
            super::migrate_v51_to_v52_reranker_kind(&mut conn).unwrap();
            continue;
        }
        let tx = conn.transaction().unwrap();
        tx.execute_batch(sql).unwrap();
        if target == 9 {
            super::migrate_v8_to_v9_profiles(&tx).unwrap();
        }
        if target == 50 {
            super::migrate_v49_to_v50_reranker(&tx).unwrap();
        }
        tx.pragma_update(None, "user_version", target).unwrap();
        tx.commit().unwrap();
    }
    conn
}

/// ctx 1 = model 1 throughout (chunk vectors since t=1000); ctx 2 = switched
/// to model 2 (chunk vectors re-made at t=5000, graph from t=1000 — the
/// "KfW-Geschäftsbericht" case); ctx 3 = no model at all.
fn seed(conn: &Connection) {
    conn.execute_batch(
        "INSERT INTO embedding_models (id, identifier, kind, model_path, native_dim, default_dim, max_concurrency)
             VALUES (1, 'jina', 'local_onnx', '/m/a.onnx', 2, 2, 1),
                    (2, 'gemma', 'local_onnx', '/m/b.onnx', 2, 2, 1);
         INSERT INTO chunking_profiles (id, name, prompt, overlap_ratio, max_signature_len, metadata_fields, match_strategy)
             VALUES (1, 'p', 'x {{pre_chunk}}', 0.2, 80, '[]', 'exact_forward');
         INSERT INTO contexts (id, name, chunking_profile_id, embedding_model_id, embedding_dim, chunking_strategy)
             VALUES (1, 'same', 1, 1, 2, 'Semantic'),
                    (2, 'switched', 1, 2, 2, 'Semantic'),
                    (3, 'unbound', 1, NULL, 2, 'Semantic');
         INSERT INTO documents (id, context_id, name) VALUES (1, 1, 'a.pdf'), (2, 2, 'b.pdf');
         INSERT INTO chunks (id, document_id, context_id, chunk_index, char_start, char_end, text)
             VALUES (1, 1, 1, 0, 0, 5, 'hello'), (2, 2, 2, 0, 0, 5, 'hello');
         INSERT INTO embeddings (chunk_id, context_id, document_id, embedding_model_id, dim, vector, created_at)
             VALUES (1, 1, 1, 1, 2, x'0000803F00000000', 1000),
                    (2, 2, 2, 2, 2, x'0000803F00000000', 5000);
         INSERT INTO ontology_nodes (id, context_id, label, entity_type, raw_entity_type, description, vector_blob, created_at)
             VALUES (10, 1, 'after',   'T', 'T', '', x'0000803F00000000', 2000),
                    (11, 1, 'before',  'T', 'T', '', x'0000803F00000000', 500),
                    (12, 1, 'novec',   'T', 'T', '', NULL, 3000),
                    (20, 2, 'stale',   'T', 'T', '', x'0000803F00000000', 1000),
                    (30, 3, 'nomodel', 'T', 'T', '', x'0000803F00000000', 9000);
         INSERT INTO ontology_communities (id, context_id, community_label, node_count, summary_text, vector_blob)
             VALUES (40, 1, 'C', 2, 'summary', x'0000803F00000000');",
    )
    .unwrap();
}

fn node_row(db: &Database, id: i64) -> (bool, Option<i64>) {
    db.conn
        .query_row(
            "SELECT vector_blob IS NOT NULL, vector_model_id FROM ontology_nodes WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

#[test]
fn v67_keeps_only_provable_node_vectors_and_drops_community_vectors() {
    let conn = migrate_to_66();
    seed(&conn);
    let db = Database::init(conn).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);

    assert_eq!(node_row(&db, 10), (true, Some(1)), "created after the model's first chunk vector: kept, stamped");
    assert_eq!(node_row(&db, 11), (false, None), "older than the current chunk vectors: unprovable, dropped");
    assert_eq!(node_row(&db, 12), (false, None));
    assert_eq!(node_row(&db, 20), (false, None), "graph older than the model switch: dropped");
    assert_eq!(node_row(&db, 30), (false, None), "context without a model: dropped");

    let (summary, blob): (String, Option<Vec<u8>>) = db
        .conn
        .query_row("SELECT summary_text, vector_blob FROM ontology_communities WHERE id = 40", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(summary, "summary", "the summary (LLM work) stays");
    assert!(blob.is_none());

    // The switched collection no longer compares across spaces: nothing to
    // search, and the dropped node is queued for re-embedding.
    assert!(db.search_ontology_nodes_semantic(2, 2, &[1.0, 0.0], 10).unwrap().is_empty());
    let missing: Vec<i64> = db.get_ontology_nodes_missing_embeddings(2).unwrap().into_iter().map(|n| n.0).collect();
    assert_eq!(missing, vec![20]);
    assert_eq!(db.list_communities_missing_vector(1).unwrap().len(), 1);
    // The provable vector still works.
    assert_eq!(db.search_ontology_nodes_semantic(1, 1, &[1.0, 0.0], 10).unwrap()[0].0, 10);
}
