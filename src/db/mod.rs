//! Embedded SQLite persistence layer for the Matrix Rust Core.
//!
//! The engine is pure Rust and Tauri-free: the caller supplies the database
//! file path (on iOS, the app-data dir resolved by the Tauri bridge). Vectors
//! are stored as raw f32 BLOBs and scanned with pure-Rust cosine — no
//! `sqlite-vec` / HNSW C-extensions, so the crate links statically for
//! `aarch64-apple-ios`.
//!
//! Schema evolution: the `schema_vN.sql` files are listed in `MIGRATIONS` and
//! run in order, one transaction each, gated by `PRAGMA user_version`. A new
//! migration is a new file with the next number; old files are never edited.
//! `migrate()` calls a Rust hook (`migration_hooks.rs`) for targets plain SQL cannot handle: v9
//! (structural profiles -> `structural_patterns` rows), v50 (promote the legacy
//! `reranker_model_dir` setting) and v51/v52 (table rebuilds that widen a
//! `kind` CHECK, which SQLite cannot ALTER; `schema_v51.sql`/`schema_v52.sql`
//! are deliberate no-ops). Repository methods live in the sibling files as
//! `impl Database` blocks; all row types live in `models.rs`.

use std::path::Path;

use rusqlite::Connection;

pub mod models;
mod scored;

pub mod chat_sessions;
pub mod chunks;
pub mod chunk_refs;
pub mod chunk_kinds;
pub mod collection_summary;
mod contexts;
mod provisional;
pub mod embeddings;
mod fts;
mod grid;
mod structural_profiles;
mod grid_profiles;
mod reasoning_lists;
mod profiles;
mod registries;
pub mod secret_fields;
mod ref_abbreviations;
mod pools;
pub mod settings;
mod eval;
pub(crate) mod page_images;
mod sheet_models;
mod source_models;
mod source_files;
mod grid_run_models;
pub mod sheet_tables;
pub mod sheet_query;
mod transfer;
pub use transfer::TableSchema;
pub mod ontology;
mod savepoint;
mod migration_hooks;
use migration_hooks::{
    migrate_v49_to_v50_reranker, migrate_v50_to_v51_embedding_kind, migrate_v51_to_v52_reranker_kind,
    migrate_v8_to_v9_profiles,
};

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
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error("secrets: {0}")]
    Secrets(#[from] crate::secrets::SecretError),
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
    include_str!("schema_v55.sql"),
    include_str!("schema_v56.sql"),
    include_str!("schema_v57.sql"),
    include_str!("schema_v58.sql"),
    include_str!("schema_v59.sql"),
    include_str!("schema_v60.sql"),
    include_str!("schema_v61.sql"),
    include_str!("schema_v62.sql"),
    include_str!("schema_v63.sql"),
    include_str!("schema_v64.sql"),
    include_str!("schema_v65.sql"),
    include_str!("schema_v66.sql"),
    include_str!("schema_v67.sql"),
    include_str!("schema_v68.sql"),
    include_str!("schema_v69.sql"),
    include_str!("schema_v70.sql"),
    include_str!("schema_v71.sql"),
    include_str!("schema_v72.sql"),
];

/// The embedded database handle. Repository methods are implemented across the
/// sibling modules as `impl Database` blocks.
pub struct Database {
    conn: Connection,
    /// Key for API keys at rest (`secret_fields.rs`); `None` = plaintext as before.
    secret_key: Option<crate::secrets::SecretKey>,
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
        let db = Self { conn, secret_key: None };
        // Safety net for physical sheet tables whose registry row is gone (sheet_tables.rs).
        db.drop_orphan_sheet_tables()?;
        // Parked evidence of deleted documents expires (ontology/evidence_parking.rs).
        db.purge_expired_parked_evidence()?;
        Ok(db)
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

#[cfg(test)]
mod tests;
#[cfg(test)]
mod grid_run_info_tests;
#[cfg(test)]
mod fts_tests;
#[cfg(test)]
mod reranker_tests;
#[cfg(test)]
mod embedding_kind_tests;
#[cfg(test)]
mod graph_vector_migration_tests;
#[cfg(test)]
mod model_paths_tests;
#[cfg(test)]
mod llm_endpoint_tests;
#[cfg(test)]
mod vision_tests;
#[cfg(test)]
mod mmproj_tests;
#[cfg(test)]
mod sample_key_tests;
#[cfg(test)]
mod sheet_query_tests;
#[cfg(test)]
mod source_files_tests;
#[cfg(test)]
mod provisional_tests;
