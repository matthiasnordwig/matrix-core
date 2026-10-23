//! The Rust hooks of `Database::migrate` (`mod.rs`) for the migrations plain SQL
//! cannot do: v9 (structural profiles -> `structural_patterns` rows), v50
//! (promote the legacy `reranker_model_dir` setting) and v51/v52 (table rebuilds
//! that widen a `kind` CHECK with foreign-key enforcement off; their `.sql`
//! files are no-ops). Each runs once, in migration order.

use rusqlite::{Connection, OptionalExtension};

use super::{CoreError, Result};

pub(super) fn migrate_v8_to_v9_profiles(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    let mut stmt = tx.prepare("SELECT id, heading_triggers, definition_triggers, ignore_patterns, target_chunk_size FROM structural_profiles")?;
    
    struct OldProf {
        id: i64,
        headings: String,
        defs: String,
        ignores: String,
        target: i64,
    }
    
    let old_profiles: Vec<OldProf> = stmt.query_map([], |row| {
        Ok(OldProf {
            id: row.get(0)?,
            headings: row.get(1)?,
            defs: row.get(2)?,
            ignores: row.get(3)?,
            target: row.get(4)?,
        })
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    
    for p in old_profiles {
        tx.execute("UPDATE structural_profiles SET max_chunk_chars = ?1, min_chunk_chars = 200 WHERE id = ?2", rusqlite::params![p.target, p.id])?;
        
        let mut insert = tx.prepare("INSERT INTO structural_patterns (profile_id, group_name, role, regex, flags, priority, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?;
        
        if !p.headings.trim().is_empty() {
            let list: Vec<&str> = p.headings.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
            if !list.is_empty() {
                let regex = format!("^((?:{})\\s*[\\d.a-zA-Z]+)\\s*(.*)", list.join("|"));
                insert.execute(rusqlite::params![p.id, "Überschriften", "heading_l1", regex, "i", 100, 0])?;
            }
        }
        
        if !p.defs.trim().is_empty() {
            let list: Vec<&str> = p.defs.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
            if !list.is_empty() {
                let regex = format!("\\b(?:{})", list.join("|"));
                insert.execute(rusqlite::params![p.id, "Definitionen", "definition", regex, "i", 50, 1])?;
            }
        }
        
        if !p.ignores.trim().is_empty() {
            let list: Vec<&str> = p.ignores.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
            if !list.is_empty() {
                let regex = format!("(?:{})", list.join("|"));
                insert.execute(rusqlite::params![p.id, "Ignorieren", "ignore", regex, "i", 200, 2])?;
            }
        }
    }
    
    Ok(())
}

/// MODEL_INFRA_PLAN.md AP2: promote a pre-existing `reranker_model_dir` setting
/// into a first-class, active `local_onnx` reranker row so rerank behavior does
/// not silently change across the migration. Idempotent (safe on re-run and on
/// DBs that never had the setting): does nothing if the setting is
/// absent/empty, if a `reranker_models` row already exists, or if
/// `active_reranker_id` is already set. Removes the now-obsolete
/// `reranker_model_dir` key afterwards.
pub(super) fn migrate_v49_to_v50_reranker(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    // Settings are stored JSON-encoded (see settings.rs); a String value is a
    // quoted JSON string, so decode it.
    let raw: Option<String> = tx
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'reranker_model_dir'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let dir = raw
        .and_then(|json| serde_json::from_str::<String>(&json).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Don't clobber an already-migrated registry (idempotency + defensive
    // against a partially-applied run).
    let existing_rows: i64 =
        tx.query_row("SELECT COUNT(*) FROM reranker_models", [], |r| r.get(0))?;
    let active_set: bool = tx
        .query_row(
            "SELECT 1 FROM app_settings WHERE key = 'active_reranker_id'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();

    if let Some(dir) = dir {
        if existing_rows == 0 && !active_set {
            tx.execute(
                "INSERT INTO reranker_models (name, kind, model_dir, execution_provider)
                 VALUES ('Local reranker (migrated)', 'local_onnx', ?1, NULL)",
                rusqlite::params![dir],
            )?;
            let id = tx.last_insert_rowid();
            // Store JSON-encoded to match settings.rs's set_setting encoding.
            tx.execute(
                "INSERT INTO app_settings (key, value) VALUES ('active_reranker_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![id.to_string()],
            )?;
        }
    }

    // Old key is obsolete regardless of whether a row was created (idempotent).
    tx.execute("DELETE FROM app_settings WHERE key = 'reranker_model_dir'", [])?;
    Ok(())
}

/// MODEL_INFRA_PLAN.md AP4b: widen `embedding_models.kind`'s CHECK to accept
/// `local_gguf` (the GGUF/llama.cpp on-device embedder). SQLite cannot ALTER a
/// CHECK constraint, so this is a full table rebuild (create new → copy → drop
/// old → rename), preserving every column (v1 base + v6 rate-limit columns) and
/// every row `id` verbatim so the FK links from `contexts.embedding_model_id`
/// (SET NULL), `embeddings.embedding_model_id` (CASCADE) and
/// `ontology_type_vector_cache.embedding_model_id` (CASCADE) stay valid.
///
/// FK enforcement MUST be off during the rebuild: with it on, `DROP TABLE`
/// fires the children's ON DELETE actions and destroys their rows (verified).
/// `PRAGMA foreign_keys` is a no-op inside a transaction, so we toggle it
/// outside and wrap only the DDL/DML in the transaction, running
/// `foreign_key_check` before commit as a guard. No indexes/triggers exist on
/// this table (verified across all schema_v*.sql), so none need recreating.
/// Idempotent: bails out early if the widened CHECK is already in place.
pub(super) fn migrate_v50_to_v51_embedding_kind(conn: &mut Connection) -> Result<()> {
    // Idempotency guard: if a prior (partial) run already rebuilt the table,
    // the `local_gguf` token is present in the stored table SQL — skip.
    let already: bool = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='embedding_models'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|sql| sql.contains("local_gguf"))
        .unwrap_or(false);

    // Toggle FK off OUTSIDE a transaction (it is a no-op inside one). Restore to
    // ON afterwards to match the connection's normal invariant (set in `init`).
    conn.pragma_update(None, "foreign_keys", false)?;
    let result = (|| -> Result<()> {
        if !already {
            let tx = conn.transaction()?;
            tx.execute_batch(
                "CREATE TABLE embedding_models_new (
                    id                 INTEGER PRIMARY KEY,
                    identifier         TEXT    NOT NULL UNIQUE,
                    kind               TEXT    NOT NULL CHECK (kind IN ('local_onnx', 'local_gguf', 'remote_api')),
                    model_path         TEXT,
                    tokenizer_path     TEXT,
                    api_config         TEXT,
                    execution_provider TEXT    CHECK (execution_provider IN ('ane', 'coreml', 'cpu')),
                    is_matryoshka      INTEGER NOT NULL DEFAULT 0,
                    native_dim         INTEGER NOT NULL,
                    default_dim        INTEGER NOT NULL,
                    normalize          INTEGER NOT NULL DEFAULT 1,
                    created_at         INTEGER NOT NULL DEFAULT (unixepoch()),
                    tpm_limit          INTEGER,
                    rpm_limit          INTEGER,
                    max_concurrency    INTEGER NOT NULL DEFAULT 1
                 );
                 INSERT INTO embedding_models_new (
                    id, identifier, kind, model_path, tokenizer_path, api_config,
                    execution_provider, is_matryoshka, native_dim, default_dim,
                    normalize, created_at, tpm_limit, rpm_limit, max_concurrency
                 )
                 SELECT
                    id, identifier, kind, model_path, tokenizer_path, api_config,
                    execution_provider, is_matryoshka, native_dim, default_dim,
                    normalize, created_at, tpm_limit, rpm_limit, max_concurrency
                 FROM embedding_models;
                 DROP TABLE embedding_models;
                 ALTER TABLE embedding_models_new RENAME TO embedding_models;",
            )?;
            // Guard: no dangling FKs introduced by the rebuild.
            let violations: i64 =
                tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0))?;
            if violations != 0 {
                // Defensive guard (should never fire): surface as a DB error so
                // the whole migration aborts and FK enforcement is restored.
                return Err(CoreError::Sqlite(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
                    Some(format!(
                        "schema_v51 rebuild left {violations} foreign-key violations"
                    )),
                )));
            }
            tx.pragma_update(None, "user_version", 51i64)?;
            tx.commit()?;
        } else {
            // Table already widened by a partial run; just bump the version.
            conn.pragma_update(None, "user_version", 51i64)?;
        }
        Ok(())
    })();
    // Always restore FK enforcement, even on error.
    conn.pragma_update(None, "foreign_keys", true)?;
    result
}

/// RERANKER_PERF_PLAN.md Phase 2: widen `reranker_models.kind`'s CHECK to accept
/// `local_gguf` (the GGUF/llama.cpp on-device reranker). SQLite cannot ALTER a
/// CHECK constraint, so this is a full table rebuild (create new → copy → drop
/// old → rename), preserving every column + every row `id` verbatim.
///
/// Unlike the v51 `embedding_models` rebuild, `reranker_models` has NO incoming
/// foreign keys (the active reranker is the plain settings value
/// `active_reranker_id`, not an FK), so `DROP TABLE` cannot cascade into any
/// child — FK enforcement need not be toggled here. The structure is kept
/// identical to v51 otherwise (own transaction, `foreign_key_check` guard,
/// idempotent) for parity. No indexes/triggers exist on this table (verified
/// across all schema_v*.sql), so none need recreating. Idempotent: bails out
/// early if the widened CHECK is already in place.
pub(super) fn migrate_v51_to_v52_reranker_kind(conn: &mut Connection) -> Result<()> {
    // Idempotency guard: if a prior (partial) run already rebuilt the table, the
    // `local_gguf` token is present in the stored table SQL — skip the rebuild.
    let already: bool = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='reranker_models'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|sql| sql.contains("local_gguf"))
        .unwrap_or(false);

    if already {
        conn.pragma_update(None, "user_version", 52i64)?;
        return Ok(());
    }

    let tx = conn.transaction()?;
    tx.execute_batch(
        "CREATE TABLE reranker_models_new (
            id                 INTEGER PRIMARY KEY,
            name               TEXT NOT NULL,
            kind               TEXT NOT NULL CHECK (kind IN ('local_onnx', 'local_gguf', 'remote_api')),
            model_dir          TEXT,
            api_config         TEXT,
            execution_provider TEXT CHECK (execution_provider IN ('ane', 'coreml', 'cpu')),
            created_at         INTEGER NOT NULL DEFAULT (strftime('%s','now'))
         );
         INSERT INTO reranker_models_new (
            id, name, kind, model_dir, api_config, execution_provider, created_at
         )
         SELECT
            id, name, kind, model_dir, api_config, execution_provider, created_at
         FROM reranker_models;
         DROP TABLE reranker_models;
         ALTER TABLE reranker_models_new RENAME TO reranker_models;",
    )?;
    // Guard: no dangling FKs introduced by the rebuild (there are none by
    // construction, but keep the check for parity with schema_v51).
    let violations: i64 =
        tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0))?;
    if violations != 0 {
        return Err(CoreError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
            Some(format!(
                "schema_v52 rebuild left {violations} foreign-key violations"
            )),
        )));
    }
    tx.pragma_update(None, "user_version", 52i64)?;
    tx.commit()?;
    Ok(())
}
