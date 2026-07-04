//! Quarantine (genuinely-failed extraction chunks awaiting manual repair, see
//! `app/src/tabs/contexts/QuarantineViewer.tsx`) and per-chunk extraction
//! resumability state (completed LLM batches + partial graph, so a chunk
//! doesn't restart from scratch after an interrupted extraction run). Split
//! out of the former monolithic `db/ontology.rs` — see HANDBUCH.md.
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

    pub fn delete_quarantined_chunk(&self, chunk_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM ontology_quarantine WHERE chunk_id = ?1",
            [chunk_id]
        )?;
        Ok(())
    }

    pub fn save_chunk_state(&self, context_id: i64, chunk_id: i64, completed_batches_json: &str, partial_graph_json: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_chunk_states (context_id, chunk_id, completed_batches_json, partial_graph_json)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(context_id, chunk_id) DO UPDATE SET
             completed_batches_json = excluded.completed_batches_json,
             partial_graph_json = excluded.partial_graph_json",
            rusqlite::params![context_id, chunk_id, completed_batches_json, partial_graph_json]
        )?;
        Ok(())
    }

    pub fn load_chunk_state(&self, context_id: i64, chunk_id: i64) -> Result<Option<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT completed_batches_json, partial_graph_json FROM ontology_chunk_states WHERE context_id = ?1 AND chunk_id = ?2"
        )?;
        let mut iter = stmt.query_map(rusqlite::params![context_id, chunk_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        if let Some(res) = iter.next() {
            Ok(Some(res?))
        } else {
            Ok(None)
        }
    }

    pub fn delete_chunk_state(&self, context_id: i64, chunk_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM ontology_chunk_states WHERE context_id = ?1 AND chunk_id = ?2",
            rusqlite::params![context_id, chunk_id]
        )?;
        Ok(())
    }
}
