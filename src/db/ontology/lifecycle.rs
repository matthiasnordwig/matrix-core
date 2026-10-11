//! Whole-context ontology teardown (used when a context's documents are
//! re-processed or deleted). Deletes edges, nodes and the extracted-chunk
//! markers; dependent rows (edge evidence, lens types/verdicts, edge reviews,
//! community members) go via FK cascade. Communities (`communities.rs`),
//! quarantine/chunk states and lenses are separate and not touched here.
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
