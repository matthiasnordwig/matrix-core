//! CRUD for `ontology_communities` (label-propagation community summaries).
//! Split out of the former monolithic `db/ontology.rs` — see HANDBUCH.md.
//!
//! NOTE on the Uniper2 community-coloring bugfix: this file intentionally
//! does NOT contain a bulk "assign raw label-propagation IDs to all nodes"
//! method anymore (there used to be an `assign_communities` here). See
//! `app/src-tauri/src/ontology/community.rs` for the full explanation — in
//! short, that method wrote non-`ontology_communities` IDs into
//! `ontology_nodes.community_id`, which broke the frontend's community color
//! lookup for nodes outside the top-100-by-size cutoff.
use crate::db::{Database, Result};

impl Database {
    pub fn create_ontology_community(&self, context_id: i64, community_label: &str, node_count: i64, summary_text: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO ontology_communities (context_id, community_label, node_count, summary_text) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![context_id, community_label, node_count, summary_text]
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_community_vector(&self, community_id: i64, vector_blob: &[u8]) -> Result<()> {
        self.conn.execute("UPDATE ontology_communities SET vector_blob = ?1 WHERE id = ?2", rusqlite::params![vector_blob, community_id])?;
        Ok(())
    }

    pub fn list_ontology_communities(&self, context_id: i64) -> Result<Vec<crate::db::models::OntologyCommunity>> {
        let mut stmt = self.conn.prepare("SELECT id, context_id, community_label, node_count, summary_text, created_at FROM ontology_communities WHERE context_id = ?1")?;
        let rows = stmt.query_map([context_id], |row| {
            Ok(crate::db::models::OntologyCommunity {
                id: row.get(0)?,
                context_id: row.get(1)?,
                community_label: row.get(2)?,
                node_count: row.get(3)?,
                summary_text: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    pub fn delete_communities_for_context(&self, context_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM ontology_communities WHERE context_id = ?1", [context_id])?;
        self.conn.execute("UPDATE ontology_nodes SET community_id = NULL WHERE context_id = ?1", [context_id])?;
        Ok(())
    }
}
