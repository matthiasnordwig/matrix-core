//! Whole-context ontology teardown (used when a context's documents are
//! re-processed or deleted). Split out of the former monolithic
//! `db/ontology.rs` — see HANDBUCH.md.
use crate::db::{Database, Result};

impl Database {
    pub fn delete_ontology_for_context(&self, context_id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM ontology_edges WHERE context_id = ?1", [context_id])?;
        tx.execute("DELETE FROM ontology_nodes WHERE context_id = ?1", [context_id])?;
        tx.execute("DELETE FROM ontology_extracted_chunks WHERE context_id = ?1", [context_id])?;
        tx.commit()?;
        Ok(())
    }
}
