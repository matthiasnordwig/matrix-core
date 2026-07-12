//! Embedded SQLite persistence layer for the Matrix Rust Core.
//!
//! The engine is pure Rust and Tauri-free: the caller supplies the database
//! file path (on iOS, the app-data dir resolved by the Tauri bridge). Vectors
//! are stored as raw f32 BLOBs and scanned with pure-Rust cosine — no
//! `sqlite-vec` / HNSW C-extensions, so the crate links statically for
//! `aarch64-apple-ios`.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

pub mod models;

mod chat_sessions;
pub mod chunks;
pub mod chunk_refs;
mod contexts;
pub mod embeddings;
mod fts;
mod grid;
mod structural_profiles;
mod grid_profiles;
mod reasoning_lists;
mod profiles;
mod registries;
mod pools;
pub mod settings;
mod eval;
pub mod ontology;

/// Errors surfaced by the persistence layer.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("dimension mismatch: expected {expected}, got {got}")]
    DimMismatch { expected: usize, got: usize },
    #[error("invalid vector blob length {0} (not a multiple of 4 bytes)")]
    InvalidBlob(usize),
    #[error("embedding error: {0}")]
    Embedding(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid pool membership: {0}")]
    InvalidPoolMembers(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;

/// Implements `rusqlite::ToSql` + `FromSql` for unit-only enums, mapping each
/// variant to the exact TEXT value used by the schema's CHECK constraints.
macro_rules! sql_enum {
    ($t:ty { $($variant:path => $s:literal),+ $(,)? }) => {
        impl rusqlite::ToSql for $t {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                let s = match self { $($variant => $s),+ };
                Ok(rusqlite::types::ToSqlOutput::from(s))
            }
        }
        impl rusqlite::types::FromSql for $t {
            fn column_result(
                value: rusqlite::types::ValueRef<'_>,
            ) -> rusqlite::types::FromSqlResult<Self> {
                match value.as_str()? {
                    $($s => Ok($variant),)+
                    other => Err(rusqlite::types::FromSqlError::Other(
                        format!("invalid enum value: {other}").into(),
                    )),
                }
            }
        }
    };
}

use models::*;

sql_enum!(ModelKind { ModelKind::LocalOnnx => "local_onnx", ModelKind::LocalGguf => "local_gguf", ModelKind::RemoteApi => "remote_api" });
sql_enum!(ExecutionProvider {
    ExecutionProvider::Ane => "ane",
    ExecutionProvider::Coreml => "coreml",
    ExecutionProvider::Cpu => "cpu",
});
sql_enum!(ContextStatus {
    ContextStatus::Created => "created",
    ContextStatus::Ingesting => "ingesting",
    ContextStatus::Staged => "staged",
    ContextStatus::Embedded => "embedded",
    ContextStatus::Error => "error",
});
sql_enum!(MatchStrategy {
    MatchStrategy::ExactForward => "exact_forward",
    MatchStrategy::Fuzzy => "fuzzy",
});
sql_enum!(PrechunkStatus {
    PrechunkStatus::Pending => "pending",
    PrechunkStatus::Sent => "sent",
    PrechunkStatus::Done => "done",
    PrechunkStatus::Error => "error",
});
sql_enum!(ChatStatus {
    ChatStatus::Queued => "queued",
    ChatStatus::Retrieving => "retrieving",
    ChatStatus::Llm => "llm",
    ChatStatus::Done => "done",
    ChatStatus::Error => "error",
});
sql_enum!(RowRefType { RowRefType::Chunk => "chunk", RowRefType::GridRow => "grid_row" });
sql_enum!(GridDataFormat { GridDataFormat::Plain => "plain", GridDataFormat::Json => "json" });

/// Ordered migrations; index i brings the schema to version i+1.
const MIGRATIONS: &[&str] = &[
    include_str!("schema_v1.sql"),
    include_str!("schema_v2.sql"),
    include_str!("schema_v3.sql"),
    include_str!("schema_v4.sql"),
    include_str!("schema_v5.sql"),
    include_str!("schema_v6.sql"),
    include_str!("schema_v7.sql"),
    include_str!("schema_v8.sql"),
    include_str!("schema_v9.sql"),
    include_str!("schema_v10.sql"),
    include_str!("schema_v11.sql"),
    include_str!("schema_v12.sql"),
    include_str!("schema_v13.sql"),
    include_str!("schema_v14.sql"),
    include_str!("schema_v15.sql"),
    include_str!("schema_v16.sql"),
    include_str!("schema_v17.sql"),
    include_str!("schema_v18.sql"),
    include_str!("schema_v19.sql"),
    include_str!("schema_v20.sql"),
    include_str!("schema_v21.sql"),
    include_str!("schema_v22.sql"),
    include_str!("schema_v23.sql"),
    include_str!("schema_v24.sql"),
    include_str!("schema_v25.sql"),
    include_str!("schema_v26.sql"),
    include_str!("schema_v27.sql"),
    include_str!("schema_v28.sql"),
    include_str!("schema_v29.sql"),
    include_str!("schema_v30.sql"),
    include_str!("schema_v31.sql"),
    include_str!("schema_v32.sql"),
    include_str!("schema_v33.sql"),
    include_str!("schema_v34.sql"),
    include_str!("schema_v35.sql"),
    include_str!("schema_v36.sql"),
    include_str!("schema_v37.sql"),
    include_str!("schema_v38.sql"),
    include_str!("schema_v39.sql"),
    include_str!("schema_v40.sql"),
    include_str!("schema_v41.sql"),
    include_str!("schema_v42.sql"),
    include_str!("schema_v43.sql"),
    include_str!("schema_v44.sql"),
    include_str!("schema_v45.sql"),
    include_str!("schema_v46.sql"),
    include_str!("schema_v47.sql"),
    include_str!("schema_v48.sql"),
    include_str!("schema_v49.sql"),
    include_str!("schema_v50.sql"),
    include_str!("schema_v51.sql"),
    include_str!("schema_v52.sql"),
    include_str!("schema_v53.sql"),
    include_str!("schema_v54.sql"),
];

/// The embedded database handle. Repository methods are implemented across the
/// sibling modules as `impl Database` blocks.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) the database at `path` and run migrations.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// Open an in-memory database (used by tests).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        // Must be set outside a transaction.
        conn.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;",
        )?;
        Self::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    fn migrate(conn: &mut Connection) -> Result<()> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate() {
            let target = (i + 1) as i64;
            if version < target {
                // v51 rebuilds `embedding_models` to widen its `kind` CHECK for
                // `local_gguf`. A table rebuild with FK-referencing children
                // (`contexts`/`embeddings`/`ontology_type_vector_cache`) MUST run
                // with foreign-key enforcement OFF — otherwise `DROP TABLE` fires
                // the children's ON DELETE actions (SET NULL / CASCADE) and
                // silently loses data. `PRAGMA foreign_keys` is a no-op inside a
                // transaction, so this migration manages FK toggling + its own tx
                // outside the generic path below.
                if target == 51 {
                    migrate_v50_to_v51_embedding_kind(conn)?;
                    continue;
                }
                // v52 rebuilds `reranker_models` to widen its `kind` CHECK for
                // `local_gguf`. Unlike v51 this table has no FK-referencing
                // children, so no FK toggling is needed — but the rebuild is kept
                // structurally identical (own tx, verbatim columns/ids,
                // `foreign_key_check` guard, idempotent) for parity.
                if target == 52 {
                    migrate_v51_to_v52_reranker_kind(conn)?;
                    continue;
                }

                let tx = conn.transaction()?;
                tx.execute_batch(sql)?;

                if target == 9 {
                    migrate_v8_to_v9_profiles(&tx)?;
                }
                if target == 50 {
                    migrate_v49_to_v50_reranker(&tx)?;
                }

                tx.pragma_update(None, "user_version", target)?;
                tx.commit()?;
            }
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    pub fn begin_transaction(&self) -> Result<()> {
        self.conn.execute_batch("BEGIN TRANSACTION").map_err(CoreError::Sqlite)
    }
    
    pub fn commit_transaction(&self) -> Result<()> {
        self.conn.execute_batch("COMMIT").map_err(CoreError::Sqlite)
    }

    pub fn rollback_transaction(&self) -> Result<()> {
        self.conn.execute_batch("ROLLBACK").map_err(CoreError::Sqlite)
    }
}

fn migrate_v8_to_v9_profiles(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
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
fn migrate_v49_to_v50_reranker(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
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
fn migrate_v50_to_v51_embedding_kind(conn: &mut Connection) -> Result<()> {
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
fn migrate_v51_to_v52_reranker_kind(conn: &mut Connection) -> Result<()> {
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

#[cfg(test)]
mod tests;
#[cfg(test)]
mod fts_tests;
#[cfg(test)]
mod reranker_tests;
#[cfg(test)]
mod embedding_kind_tests;
