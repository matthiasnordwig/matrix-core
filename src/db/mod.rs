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
mod structural_profiles;
mod grid_profiles;
mod profiles;
mod registries;
mod pools;
mod settings;
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
                
                if target == 9 {
                    migrate_v8_to_v9_profiles(&tx)?;
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

#[cfg(test)]
mod tests;
