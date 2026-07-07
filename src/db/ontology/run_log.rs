//! Persistent ontology run log (`ontology_run_log`, schema_v43): per-phase
//! success/failure counters that survive app restart, unlike the in-memory
//! applog ring buffer (CAPACITY=500). Only the community-summarization phase
//! writes rows for now — see `app/src-tauri/src/ontology/community/mod.rs`
//! and BACKLOG.md.
use crate::db::{Database, Result};
use crate::db::models::OntologyRunLogEntry;

impl Database {
    /// Insert one completed phase record. Returns the row id.
    pub fn insert_ontology_run_log(
        &self,
        run_id: &str,
        context_id: i64,
        phase: &str,
        started_at: i64,
        finished_at: Option<i64>,
        attempted: i64,
        succeeded: i64,
        failed: i64,
        sample_error: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO ontology_run_log
                (run_id, context_id, phase, started_at, finished_at, attempted, succeeded, failed, sample_error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![run_id, context_id, phase, started_at, finished_at, attempted, succeeded, failed, sample_error],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// All run-log rows for a context, most recent first (started_at DESC, id DESC).
    pub fn list_ontology_run_log(&self, context_id: i64) -> Result<Vec<OntologyRunLogEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, run_id, context_id, phase, started_at, finished_at, attempted, succeeded, failed, sample_error
             FROM ontology_run_log WHERE context_id = ?1 ORDER BY started_at DESC, id DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![context_id], |row| {
            Ok(OntologyRunLogEntry {
                id: row.get(0)?,
                run_id: row.get(1)?,
                context_id: row.get(2)?,
                phase: row.get(3)?,
                started_at: row.get(4)?,
                finished_at: row.get::<_, Option<i64>>(5)?,
                attempted: row.get(6)?,
                succeeded: row.get(7)?,
                failed: row.get(8)?,
                sample_error: row.get::<_, Option<String>>(9)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}
