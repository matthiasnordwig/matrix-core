//! Multi-context retrieval honoring the strict model-to-context binding.
//!
//! Steps (no global cross-model vector compass):
//!   a) group the selected contexts by their distinct embedding model,
//!   b) one query vector per distinct model,
//!   c) isolated brute-force cosine scan per embedding space, then merge the
//!      per-space hits by score and take the global top-k.
//!
//! [`Database::retrieve`] derives the per-model query vectors via a
//! [`QueryEmbedder`] (sync). [`Database::retrieve_with`] takes them
//! precomputed, which lets an async caller (the bridge) embed over the network
//! first and then run the sync scan.

use std::collections::HashMap;

use super::QueryEmbedder;
use crate::db::models::ScoredChunk;
use crate::{Database, Result};

impl Database {
    /// Map each selected context to its embedding model id.
    fn contexts_by_model(&self, context_ids: &[i64]) -> Result<HashMap<i64, Vec<i64>>> {
        let mut by_model: HashMap<i64, Vec<i64>> = HashMap::new();
        for &cid in context_ids {
            if let Some(ctx) = self.context(cid)?
                && let Some(model_id) = ctx.embedding_model_id
            {
                by_model.entry(model_id).or_default().push(cid);
            }
        }
        Ok(by_model)
    }

    /// Retrieve top-`k` chunks using query vectors precomputed per embedding
    /// model (`query_by_model[model_id]`). Cosine is scale-invariant, so inputs
    /// need not be normalized and scores are comparable across spaces.
    pub fn retrieve_with(
        &self,
        context_ids: &[i64],
        query_by_model: &HashMap<i64, Vec<f32>>,
        top_k: usize,
    ) -> Result<Vec<ScoredChunk>> {
        let mut merged: Vec<ScoredChunk> = Vec::new();
        for (model_id, ctxs) in self.contexts_by_model(context_ids)? {
            let Some(qvec) = query_by_model.get(&model_id) else {
                continue;
            };
            for cid in ctxs {
                merged.extend(self.search_context(cid, qvec, top_k)?);
            }
        }
        merged.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        merged.truncate(top_k);
        Ok(merged)
    }

    /// Retrieve top-`k` chunks across `context_ids`, embedding the query per
    /// distinct model via `embedder`.
    pub fn retrieve(
        &self,
        context_ids: &[i64],
        query: &str,
        top_k: usize,
        embedder: &dyn QueryEmbedder,
    ) -> Result<Vec<ScoredChunk>> {
        let mut query_by_model: HashMap<i64, Vec<f32>> = HashMap::new();
        for model_id in self.contexts_by_model(context_ids)?.keys().copied() {
            if let Some(model) = self.embedding_model(model_id)? {
                query_by_model.insert(model_id, embedder.embed_query(&model, query)?);
            }
        }
        self.retrieve_with(context_ids, &query_by_model, top_k)
    }
}
