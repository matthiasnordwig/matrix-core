//! Embedding space of graph vectors (schema_v67): `ontology_nodes` and
//! `ontology_communities` store, next to `vector_blob`, the model that wrote it
//! (`vector_model_id`). A vector counts as usable only while that model is the
//! context's current `embedding_model_id`; everything else counts as missing, so
//! `get_ontology_nodes_missing_embeddings`/`list_communities_missing_vector`
//! pick it up for re-embedding and no reader ever compares across spaces.
//!
//! `clear_graph_vectors_for_context` is the model-change hook
//! (`services::commands::contexts::update_context`, next to the chunk vectors);
//! the two counters feed the Graph view's "needs re-embedding" hint and the
//! MCP `search_graph_nodes` note.
use crate::db::{Database, Result};

impl Database {
    /// Drops every node and community vector of a context (blob and model) —
    /// they belong to the previous search model. Returns `(nodes, communities)`
    /// that had a vector.
    pub fn clear_graph_vectors_for_context(&self, context_id: i64) -> Result<(usize, usize)> {
        let nodes = self.conn.execute(
            "UPDATE ontology_nodes SET vector_blob = NULL, vector_model_id = NULL
              WHERE context_id = ?1 AND vector_blob IS NOT NULL",
            [context_id],
        )?;
        let communities = self.conn.execute(
            "UPDATE ontology_communities SET vector_blob = NULL, vector_model_id = NULL
              WHERE context_id = ?1 AND vector_blob IS NOT NULL",
            [context_id],
        )?;
        Ok((nodes, communities))
    }

    /// `(all nodes, nodes with a vector of the context's current model)`.
    pub fn node_vector_counts(&self, context_id: i64) -> Result<(i64, i64)> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(o.vector_blob IS NOT NULL AND o.vector_model_id = c.embedding_model_id), 0)
               FROM ontology_nodes o JOIN contexts c ON c.id = o.context_id
              WHERE o.context_id = ?1",
            [context_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    /// `(communities with a summary visible to retrieval, of those with a
    /// vector of the current model)` — "visible" = the active lens or
    /// `lens_id IS NULL`, the same filter graph retrieval uses.
    pub fn community_vector_counts(&self, context_id: i64) -> Result<(i64, i64)> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(co.vector_blob IS NOT NULL AND co.vector_model_id = c.embedding_model_id), 0)
               FROM ontology_communities co JOIN contexts c ON c.id = co.context_id
              WHERE co.context_id = ?1 AND co.summary_text != ''
                AND (co.lens_id = c.active_lens_id OR co.lens_id IS NULL)",
            [context_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }
}
