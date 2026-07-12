//! Reranker registry (MODEL_INFRA_PLAN.md AP2): CRUD round-trips, the
//! active-id setting helper, and the `schema_v50` migration of a legacy
//! `reranker_model_dir` setting into an active `local_onnx` row.

use super::models::*;
use super::settings::KEY_ACTIVE_RERANKER_ID;
use super::Database;
use rusqlite::Connection;

fn new_local(name: &str, dir: &str) -> NewRerankerModel {
    NewRerankerModel {
        name: name.into(),
        kind: ModelKind::LocalOnnx,
        model_dir: Some(dir.into()),
        api_config: None,
        execution_provider: Some(ExecutionProvider::Coreml),
    }
}

fn new_remote(name: &str, cfg: &str) -> NewRerankerModel {
    NewRerankerModel {
        name: name.into(),
        kind: ModelKind::RemoteApi,
        model_dir: None,
        api_config: Some(cfg.into()),
        execution_provider: None,
    }
}

#[test]
fn reranker_crud_round_trip() {
    let db = Database::open_in_memory().expect("open");
    let created = db.create_reranker_model(&new_local("Local A", "/models/rr")).unwrap();
    assert_eq!(created.name, "Local A");
    assert_eq!(created.kind, ModelKind::LocalOnnx);
    assert_eq!(created.model_dir.as_deref(), Some("/models/rr"));
    assert_eq!(created.execution_provider, Some(ExecutionProvider::Coreml));
    assert!(created.created_at > 0);

    let fetched = db.reranker_model(created.id).unwrap().expect("exists");
    assert_eq!(fetched.id, created.id);

    // Remote round-trip: api_config JSON survives verbatim.
    let cfg = r#"{"base_url":"https://api.jina.ai/v1","model":"jina-reranker-v2","key_ref":"sk-x","api_format":"jina"}"#;
    let remote = db.create_reranker_model(&new_remote("Jina", cfg)).unwrap();
    assert_eq!(remote.kind, ModelKind::RemoteApi);
    assert!(remote.model_dir.is_none());
    assert_eq!(remote.api_config.as_deref(), Some(cfg));

    let all = db.list_reranker_models().unwrap();
    assert_eq!(all.len(), 2);
    // ORDER BY name → "Jina" before "Local A".
    assert_eq!(all[0].name, "Jina");

    let updated = db
        .update_reranker_model(created.id, &new_local("Local A2", "/models/rr2"))
        .unwrap();
    assert_eq!(updated.name, "Local A2");
    assert_eq!(updated.model_dir.as_deref(), Some("/models/rr2"));

    assert!(db.delete_reranker_model(created.id).unwrap());
    assert!(db.reranker_model(created.id).unwrap().is_none());
    assert!(!db.delete_reranker_model(created.id).unwrap()); // idempotent no-op
}

#[test]
fn active_reranker_helper_and_delete_clears_setting() {
    let db = Database::open_in_memory().expect("open");
    assert!(db.active_reranker_model().unwrap().is_none()); // OFF by default

    let m = db.create_reranker_model(&new_local("Active", "/m")).unwrap();
    db.set_setting(KEY_ACTIVE_RERANKER_ID, &m.id).unwrap();
    let active = db.active_reranker_model().unwrap().expect("active row");
    assert_eq!(active.id, m.id);

    // Deleting the active reranker clears the dangling setting → back to OFF.
    assert!(db.delete_reranker_model(m.id).unwrap());
    assert!(db.active_reranker_model().unwrap().is_none());
    assert!(db.get_setting::<i64>(KEY_ACTIVE_RERANKER_ID).unwrap().is_none());
}

/// Migrate a fresh conn up to just before v50, seed the legacy setting, then
/// let the final migration run and assert the promotion.
fn migrate_to_49_then_seed(dir: Option<&str>) -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    // Run every migration through v49 manually (stop before v50 so the legacy
    // `reranker_model_dir` setting can be seeded pre-promotion). `Database::init`
    // below then applies v50 (the promotion) and v51.
    for (i, sql) in super::MIGRATIONS[..49].iter().enumerate() {
        let target = (i + 1) as i64;
        let tx = conn.transaction().unwrap();
        tx.execute_batch(sql).unwrap();
        if target == 9 {
            super::migrate_v8_to_v9_profiles(&tx).unwrap();
        }
        tx.pragma_update(None, "user_version", target).unwrap();
        tx.commit().unwrap();
    }
    if let Some(d) = dir {
        // Store JSON-encoded, exactly as set_setting would.
        let json = serde_json::to_string(&d.to_string()).unwrap();
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('reranker_model_dir', ?1)",
            rusqlite::params![json],
        )
        .unwrap();
    }
    conn
}

#[test]
fn migration_promotes_existing_reranker_dir_to_active_local_row() {
    let conn = migrate_to_49_then_seed(Some("/models/jina-reranker-v2"));
    let db = Database::init(conn).unwrap();
    assert_eq!(db.schema_version().unwrap(), 52);

    // An active local_onnx row now exists with the migrated dir.
    let rows = db.list_reranker_models().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, ModelKind::LocalOnnx);
    assert_eq!(rows[0].model_dir.as_deref(), Some("/models/jina-reranker-v2"));
    assert_eq!(rows[0].name, "Local reranker (migrated)");

    let active = db.active_reranker_model().unwrap().expect("active");
    assert_eq!(active.id, rows[0].id);

    // Old key is gone.
    assert!(db
        .get_setting::<String>("reranker_model_dir")
        .unwrap()
        .is_none());
}

#[test]
fn migration_noop_without_legacy_setting() {
    let conn = migrate_to_49_then_seed(None);
    let db = Database::init(conn).unwrap();
    assert_eq!(db.schema_version().unwrap(), 52);
    assert!(db.list_reranker_models().unwrap().is_empty());
    assert!(db.active_reranker_model().unwrap().is_none());
}

#[test]
fn migration_noop_with_empty_legacy_setting() {
    let conn = migrate_to_49_then_seed(Some("   "));
    let db = Database::init(conn).unwrap();
    assert!(db.list_reranker_models().unwrap().is_empty());
    // Empty/whitespace dir → nothing created, but the obsolete key is removed.
    assert!(db
        .get_setting::<String>("reranker_model_dir")
        .unwrap()
        .is_none());
}

// --- schema_v52: widen reranker_models.kind for local_gguf ---------------
//
// RERANKER_PERF_PLAN.md Phase 2. Mirrors the schema_v51 embedding-kind tests
// (`embedding_kind_tests.rs`): prove the table rebuild preserves rows verbatim
// and now accepts `local_gguf` while still rejecting garbage. Simpler than v51
// because `reranker_models` has no incoming FKs.

/// Migrate a fresh conn up to and including v51 (stop before the v52 rebuild we
/// are about to test), then seed a couple of reranker rows so the rebuild can be
/// shown to preserve them verbatim. Migrations 1..=50 run through the generic
/// loop (with their Rust hooks); the v51 hook then runs on the owned connection
/// (it manages its own transaction + FK toggling internally).
fn migrate_to_51_then_seed_rerankers() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    // Targets 1..=50 (indices 0..50): schema_v50.sql is index 49.
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
    // v51 rebuilds embedding_models (own tx + FK toggling); run it on the conn.
    super::migrate_v50_to_v51_embedding_kind(&mut conn).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        51
    );
    // Seed a local_onnx row (id 1) + a remote row (id 2) via raw SQL, exactly
    // the columns the v52 rebuild must carry over.
    conn.execute_batch(
        "INSERT INTO reranker_models (id, name, kind, model_dir, api_config, execution_provider)
             VALUES (1, 'Local RR', 'local_onnx', '/m/rr', NULL, 'cpu'),
                    (2, 'Remote RR', 'remote_api', NULL, '{\"base_url\":\"https://x\"}', NULL);",
    )
    .unwrap();
    conn
}

#[test]
fn v52_rebuild_preserves_rows_and_widens_kind() {
    let conn = migrate_to_51_then_seed_rerankers();
    let db = Database::init(conn).unwrap();
    assert_eq!(db.schema_version().unwrap(), 52);

    // Both seeded rows survive verbatim (ids + kinds + columns intact).
    let rows = db.list_reranker_models().unwrap();
    assert_eq!(rows.len(), 2);
    let local = rows.iter().find(|r| r.id == 1).expect("row 1");
    assert_eq!(local.kind, ModelKind::LocalOnnx);
    assert_eq!(local.model_dir.as_deref(), Some("/m/rr"));
    assert_eq!(local.execution_provider, Some(ExecutionProvider::Cpu));
    let remote = rows.iter().find(|r| r.id == 2).expect("row 2");
    assert_eq!(remote.kind, ModelKind::RemoteApi);
    assert_eq!(remote.api_config.as_deref(), Some("{\"base_url\":\"https://x\"}"));

    // local_gguf is now a valid kind (CRUD round-trip through the typed API):
    // the GGUF reranker stores its .gguf file path in `model_dir`.
    let gguf = db
        .create_reranker_model(&NewRerankerModel {
            name: "GGUF RR".into(),
            kind: ModelKind::LocalGguf,
            model_dir: Some("/m/bge-reranker-v2-m3-Q8_0.gguf".into()),
            api_config: None,
            execution_provider: None,
        })
        .unwrap();
    let fetched = db.reranker_model(gguf.id).unwrap().expect("exists");
    assert_eq!(fetched.kind, ModelKind::LocalGguf);
    assert_eq!(
        fetched.model_dir.as_deref(),
        Some("/m/bge-reranker-v2-m3-Q8_0.gguf")
    );
    assert!(fetched.execution_provider.is_none());

    // The widened CHECK still rejects an unknown kind (raw INSERT).
    let conn = db_conn(&db);
    let err = conn.execute(
        "INSERT INTO reranker_models (name, kind) VALUES ('bad', 'not_a_kind')",
        [],
    );
    assert!(err.is_err(), "CHECK must still reject unknown kinds");
}

#[test]
fn v52_fresh_open_lands_at_52_and_accepts_gguf() {
    // The normal fresh-open path already applies v52 (idempotency guard in the
    // hook sees `local_gguf` in the table SQL on any re-run).
    let db = Database::open_in_memory().unwrap();
    assert_eq!(db.schema_version().unwrap(), 52);
    assert!(db.list_reranker_models().unwrap().is_empty());

    let m = db
        .create_reranker_model(&NewRerankerModel {
            name: "GGUF".into(),
            kind: ModelKind::LocalGguf,
            model_dir: Some("/m/x.gguf".into()),
            api_config: None,
            execution_provider: None,
        })
        .unwrap();
    assert_eq!(db.reranker_model(m.id).unwrap().unwrap().kind, ModelKind::LocalGguf);
}

/// Test-only accessor to the underlying connection for raw assertions.
fn db_conn(db: &Database) -> &Connection {
    &db.conn
}
