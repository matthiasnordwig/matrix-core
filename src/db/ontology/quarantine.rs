//! Quarantine: genuinely-failed extraction chunks awaiting manual repair, see
//! `app/src/tabs/contexts/QuarantineViewer.tsx`. (The table `ontology_chunk_states`
//! next to it is inert: nothing reads or writes it since a passage always goes to
//! the model whole, see `ISSUES.md` „Bewusste Entscheidungen“.)
use crate::db::{Database, Result};

impl Database {
    pub fn insert_quarantined_chunk(&self, context_id: i64, chunk_id: i64, graph_json: &str, error_reason: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO ontology_quarantine (chunk_id, context_id, graph_json, error_reason) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![chunk_id, context_id, graph_json, error_reason],
        )?;
        Ok(())
    }

    pub fn get_quarantined_chunks(&self, context_id: i64) -> Result<Vec<crate::db::models::OntologyQuarantineChunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT chunk_id, context_id, graph_json, error_reason, created_at FROM ontology_quarantine WHERE context_id = ?1"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            Ok(crate::db::models::OntologyQuarantineChunk {
                chunk_id: row.get(0)?,
                context_id: row.get(1)?,
                graph_json: row.get(2)?,
                error_reason: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Number of quarantined chunks of a context (no `graph_json` loaded).
    pub fn count_quarantined_chunks(&self, context_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM ontology_quarantine WHERE context_id = ?1",
            [context_id],
            |row| row.get(0),
        )?)
    }

    pub fn delete_quarantined_chunk(&self, chunk_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM ontology_quarantine WHERE chunk_id = ?1",
            [chunk_id]
        )?;
        Ok(())
    }
}
