//! Embedded SQLite persistence layer for the Matrix Rust Core.
//!
//! The engine is pure Rust and Tauri-free: the caller supplies the database
//! file path (on iOS, the app-data dir resolved by the Tauri bridge). Vectors
//! are stored as raw f32 BLOBs and scanned with pure-Rust cosine — no
//! `sqlite-vec` / HNSW C-extensions, so the crate links statically for
//! `aarch64-apple-ios`.

use std::path::Path;

use rusqlite::Connection;

pub mod models;

mod chunks;
mod contexts;
pub mod embeddings;
mod grid;
mod profiles;
mod registries;
mod settings;

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

sql_enum!(ModelKind { ModelKind::LocalOnnx => "local_onnx", ModelKind::RemoteApi => "remote_api" });
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

/// Ordered migrations; index i brings the schema to version i+1.
const MIGRATIONS: &[&str] = &[
    include_str!("schema_v1.sql"),
    include_str!("schema_v2.sql"),
    include_str!("schema_v3.sql"),
    include_str!("schema_v4.sql"),
    include_str!("schema_v5.sql"),
    include_str!("schema_v6.sql"),
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
                let tx = conn.transaction()?;
                tx.execute_batch(sql)?;
                tx.pragma_update(None, "user_version", target)?;
                tx.commit()?;
            }
        }
        Ok(())
    }

    /// Current `PRAGMA user_version`.
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
    }
}

#[cfg(test)]
mod tests;
