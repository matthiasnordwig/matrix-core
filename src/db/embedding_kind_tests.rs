//! `schema_v51` (MODEL_INFRA_PLAN.md AP4b): widening `embedding_models.kind`'s
//! CHECK to accept `local_gguf` via a full table rebuild. These tests prove the
//! rebuild (a) preserves all pre-existing rows verbatim, (b) preserves the FK
//! links from `contexts` (SET NULL), `embeddings` (CASCADE) and
//! `ontology_type_vector_cache` (CASCADE) — i.e. no child row is silently
//! deleted/NULLed by the DROP TABLE — and (c) now accepts `local_gguf` while
//! still rejecting garbage kinds. Also: post-migration `create_embedding_model`
//! with `ModelKind::LocalGguf` succeeds (CRUD round-trip).

use super::models::*;
use super::Database;
use rusqlite::Connection;

/// Migrate a fresh conn up to just before v51 (i.e. through v50), then seed an
/// `embedding_models` row plus FK-referencing child rows in all three tables,
/// so the v51 rebuild can be proven to preserve them.
fn migrate_to_50_then_seed() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    // Run every migration up to and including v50 manually (stop before the v51
    // rebuild we are about to test), matching mod.rs's loop. `schema_v50.sql` is
    // index 49 (target 50), so take the first 50 entries.
    for (i, sql) in super::MIGRATIONS[..50].iter().enumerate() {
        let target = (i + 1) as i64;
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
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        50
    );

    // Seed an embedding model (id 1) + a second (id 2), a chunking profile, a
    // context bound to model 1, a document + chunk, an embeddings row bound to
    // model 1, and an ontology_type_vector_cache row bound to model 2 — all via
    // raw SQL so we exercise exactly the FK edges the rebuild must survive.
    conn.execute_batch(
        "INSERT INTO embedding_models (id, identifier, kind, model_path, native_dim, default_dim, max_concurrency)
             VALUES (1, 'jina-onnx', 'local_onnx', '/m/model.onnx', 4, 4, 1),
                    (2, 'remote-x',  'remote_api', NULL, 8, 8, 3);
         INSERT INTO chunking_profiles (id, name, prompt, overlap_ratio, max_signature_len, metadata_fields, match_strategy)
             VALUES (1, 'p', 'x {{pre_chunk}}', 0.2, 80, '[]', 'exact_forward');
         INSERT INTO contexts (id, name, chunking_profile_id, embedding_model_id, embedding_dim, chunking_strategy)
             VALUES (1, 'ctx', 1, 1, 4, 'Semantic');
         INSERT INTO documents (id, context_id, name) VALUES (1, 1, 'd.pdf');
         INSERT INTO chunks (id, document_id, context_id, chunk_index, char_start, char_end, text)
             VALUES (1, 1, 1, 0, 0, 5, 'hello');
         INSERT INTO embeddings (chunk_id, context_id, document_id, embedding_model_id, dim, vector)
             VALUES (1, 1, 1, 1, 4, x'00000000000000000000000000000000');
         INSERT INTO ontology_type_vector_cache (embedding_model_id, raw_type, vector)
             VALUES (2, 'Person', x'0000000000000000');",
    )
    .unwrap();
    conn
}

#[test]
fn v51_rebuild_preserves_rows_and_fk_links() {
    let conn = migrate_to_50_then_seed();
    let db = Database::init(conn).unwrap();
    assert_eq!(db.schema_version().unwrap(), 56);

    // Both embedding_models rows survive verbatim (ids + kinds intact).
    let models = db.list_embedding_models().unwrap();
    assert_eq!(models.len(), 2);
    let ids: Vec<i64> = models.iter().map(|m| m.id).collect();
    assert!(ids.contains(&1) && ids.contains(&2));

    // FK links survived the DROP/rename (this is the whole point — with FK
    // enforcement on during the rebuild these would have been NULLed/cascaded).
    let ctx = db.context(1).unwrap().expect("context 1 exists");
    assert_eq!(
        ctx.embedding_model_id,
        Some(1),
        "context FK must still point to model 1 (not NULLed by ON DELETE SET NULL)"
    );

    // The embeddings row (ON DELETE CASCADE from embedding_models) still exists.
    let conn2 = db_conn(&db);
    let emb_count: i64 = conn2
        .query_row(
            "SELECT COUNT(*) FROM embeddings WHERE embedding_model_id = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(emb_count, 1, "embeddings row must survive the rebuild");

    // The ontology_type_vector_cache row (ON DELETE CASCADE) still exists.
    let cache_count: i64 = conn2
        .query_row(
            "SELECT COUNT(*) FROM ontology_type_vector_cache WHERE embedding_model_id = 2",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cache_count, 1, "type-vector-cache row must survive the rebuild");

    // No dangling FKs anywhere after the rebuild.
    let violations: i64 = conn2
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(violations, 0);

    // FK enforcement was restored to ON after the migration.
    let fk_on: i64 = conn2
        .pragma_query_value(None, "foreign_keys", |r| r.get(0))
        .unwrap();
    assert_eq!(fk_on, 1, "foreign_keys must be back ON after migration");
}

#[test]
fn v51_accepts_local_gguf_and_still_rejects_garbage() {
    let db = Database::open_in_memory().unwrap();
    assert_eq!(db.schema_version().unwrap(), 56);

    // local_gguf is now a valid kind (CRUD round-trip through the typed API).
    let m = db
        .create_embedding_model(&NewEmbeddingModel {
            identifier: "jina-gguf".into(),
            kind: ModelKind::LocalGguf,
            model_path: Some("/m/jina-de-Q8_0.gguf".into()),
            tokenizer_path: None,
            api_config: None,
            execution_provider: None,
            is_matryoshka: false,
            native_dim: 768,
            default_dim: 768,
            normalize: true,
            tpm_limit: None,
            rpm_limit: None,
            max_concurrency: 1,
        })
        .unwrap();
    let fetched = db
        .list_embedding_models()
        .unwrap()
        .into_iter()
        .find(|x| x.id == m.id)
        .unwrap();
    assert_eq!(fetched.kind, ModelKind::LocalGguf);
    assert_eq!(fetched.model_path.as_deref(), Some("/m/jina-de-Q8_0.gguf"));
    assert!(fetched.tokenizer_path.is_none());
    assert!(fetched.execution_provider.is_none());

    // The widened CHECK still rejects an unknown kind (raw INSERT).
    let conn = db_conn(&db);
    let err = conn.execute(
        "INSERT INTO embedding_models (identifier, kind, native_dim, default_dim, max_concurrency)
             VALUES ('bad', 'not_a_kind', 1, 1, 1)",
        [],
    );
    assert!(err.is_err(), "CHECK must still reject unknown kinds");
}

/// Test-only accessor to the underlying connection for raw assertions.
fn db_conn(db: &Database) -> &Connection {
    &db.conn
}
