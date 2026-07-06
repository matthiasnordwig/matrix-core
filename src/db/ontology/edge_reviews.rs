//! CRUD for `ontology_edge_reviews` — the non-blocking review list for edges
//! flagged by the stage-2 negation/polarity verification (see
//! `app/src-tauri/src/ontology/extract/verify.rs`) as "unclear" or whose
//! verification call failed outright. Unlike `ontology_quarantine` (s.
//! `quarantine.rs`), rows here never block the pipeline.
use crate::db::{Database, Result};
use crate::db::models::OntologyEdgeReview;

impl Database {
    pub fn insert_ontology_edge_review(
        &self,
        context_id: i64,
        edge_id: i64,
        chunk_id: Option<i64>,
        relation_type: &str,
        evidence: Option<&str>,
        reason: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_edge_reviews (context_id, edge_id, chunk_id, relation_type, evidence, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![context_id, edge_id, chunk_id, relation_type, evidence, reason],
        )?;
        Ok(())
    }

    /// Joins the reviewed edge's source/target node labels (never orphaned:
    /// `ontology_edge_reviews.edge_id` cascades on edge deletion) so the UI
    /// viewer can render the triplet without a second round-trip.
    pub fn list_ontology_edge_reviews(&self, context_id: i64) -> Result<Vec<OntologyEdgeReview>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.context_id, r.edge_id, r.chunk_id, r.relation_type, r.evidence, r.reason, r.created_at, s.label, t.label
             FROM ontology_edge_reviews r
             JOIN ontology_edges e ON e.id = r.edge_id
             JOIN ontology_nodes s ON s.id = e.source_id
             JOIN ontology_nodes t ON t.id = e.target_id
             WHERE r.context_id = ?1"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            Ok(OntologyEdgeReview {
                id: row.get(0)?,
                context_id: row.get(1)?,
                edge_id: row.get(2)?,
                chunk_id: row.get(3)?,
                relation_type: row.get(4)?,
                evidence: row.get(5)?,
                reason: row.get(6)?,
                created_at: row.get(7)?,
                source_label: row.get(8)?,
                target_label: row.get(9)?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn delete_ontology_edge_review(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM ontology_edge_reviews WHERE id = ?1", [id])?;
        Ok(())
    }
}
