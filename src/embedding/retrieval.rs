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

/// Reciprocal Rank Fusion constant (dampening). Standard value.
pub(crate) const RRF_K: usize = 60;

/// Retrieval fan-out N for each list before fusion: `max(50, 5·top_k)`.
pub(crate) fn hybrid_fanout(top_k: usize) -> usize {
    (5 * top_k).max(50)
}

/// Pure RRF fusion over an arbitrary set of rank lists. Each inner slice is one
/// ranked list of chunk ids, best-first (index 0 = rank 1). A chunk's fused
/// score is `Σ 1/(k + rank)` over the lists it appears in (rank 1-based). The
/// result is sorted by score desc (ties broken by ascending chunk_id for
/// determinism) and truncated to `top_k`.
///
/// Fusion works purely on **ranks within each list** — it never mixes scores
/// across lists, which is what preserves the embedding-space isolation
/// invariant when the caller only ever fuses lists from the *same* space.
pub(crate) fn rrf_fuse(lists: &[Vec<i64>], k: usize, top_k: usize) -> Vec<ScoredChunk> {
    let mut acc: HashMap<i64, f32> = HashMap::new();
    for list in lists {
        for (i, &chunk_id) in list.iter().enumerate() {
            let rank = i + 1; // 1-based
            *acc.entry(chunk_id).or_insert(0.0) += 1.0 / (k + rank) as f32;
        }
    }
    let mut fused: Vec<ScoredChunk> = acc
        .into_iter()
        .map(|(chunk_id, score)| ScoredChunk { chunk_id, score })
        .collect();
    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.chunk_id.cmp(&b.chunk_id))
    });
    fused.truncate(top_k);
    fused
}

impl Database {
    /// Map each selected context to its embedding model id.
    pub(crate) fn contexts_by_model(&self, context_ids: &[i64]) -> Result<HashMap<i64, Vec<i64>>> {
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
    /// `doc_ids = Some(&[…])` restricts every per-context scan to those documents
    /// (file-level retrieval scope, AP8); `None` = whole contexts. The same slice
    /// is applied to the FTS list in the hybrid variant, so both lists are scoped
    /// identically before RRF fusion (no provenance leak).
    pub fn retrieve_with(
        &self,
        context_ids: &[i64],
        query_by_model: &HashMap<i64, Vec<f32>>,
        top_k: usize,
        doc_ids: Option<&[i64]>,
    ) -> Result<Vec<ScoredChunk>> {
        let mut merged: Vec<ScoredChunk> = Vec::new();
        for (model_id, ctxs) in self.contexts_by_model(context_ids)? {
            let Some(qvec) = query_by_model.get(&model_id) else {
                continue;
            };
            for cid in ctxs {
                merged.extend(self.search_context(cid, qvec, top_k, doc_ids)?);
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

    pub fn retrieve_batch(
        &self,
        context_ids: &[i64],
        queries_by_row: &[HashMap<i64, Vec<f32>>],
        top_k: usize,
        doc_ids: Option<&[i64]>,
    ) -> Result<Vec<Vec<ScoredChunk>>> {
        if matches!(doc_ids, Some(d) if d.is_empty()) {
            return Ok(vec![Vec::new(); queries_by_row.len()]);
        }
        let mut row_merged: Vec<Vec<ScoredChunk>> = vec![Vec::new(); queries_by_row.len()];
        for (model_id, ctxs) in self.contexts_by_model(context_ids)? {
            for cid in ctxs {
                let stored = self.scan_context_vectors(cid)?;
                if stored.is_empty() {
                    continue;
                }
                for (row_idx, query_by_model) in queries_by_row.iter().enumerate() {
                    let Some(qvec) = query_by_model.get(&model_id) else {
                        continue;
                    };
                    for sv in &stored {
                        if let Some(allowed) = doc_ids {
                            if !allowed.contains(&sv.document_id) {
                                continue;
                            }
                        }
                        if sv.vector.len() == qvec.len() {
                            row_merged[row_idx].push(ScoredChunk {
                                chunk_id: sv.chunk_id,
                                score: crate::db::embeddings::cosine(qvec, &sv.vector),
                            });
                        }
                    }
                }
            }
        }
        for merged in &mut row_merged {
            merged.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            merged.truncate(top_k);
        }
        Ok(row_merged)
    }

    /// Hybrid retrieval: Reciprocal Rank Fusion of the per-context vector list
    /// and the per-context FTS5/BM25 keyword list. Fusion happens **per context
    /// (per embedding space)** and only on ranks — no cross-space score
    /// comparison — then the RRF scores accumulate globally across all
    /// (model, context) pairs before the final top-`k` cut. The RRF score is
    /// placed in `ScoredChunk::score`.
    pub fn retrieve_hybrid_with(
        &self,
        context_ids: &[i64],
        query_by_model: &HashMap<i64, Vec<f32>>,
        raw_query: &str,
        top_k: usize,
        doc_ids: Option<&[i64]>,
    ) -> Result<Vec<ScoredChunk>> {
        let n = hybrid_fanout(top_k);
        // Accumulate RRF scores globally across contexts/spaces.
        let mut acc: HashMap<i64, f32> = HashMap::new();
        for (model_id, ctxs) in self.contexts_by_model(context_ids)? {
            let Some(qvec) = query_by_model.get(&model_id) else {
                continue;
            };
            for cid in ctxs {
                // Vector list (best-first) restricted to this one context/space.
                // The SAME `doc_ids` filter is applied to BOTH the vector list
                // and the FTS list below, so both are scoped identically before
                // RRF fusion — a leak here would surface out-of-scope chunks with
                // valid provenance (the `is_omitted` leak class).
                let vec_hits = self.search_context(cid, qvec, n, doc_ids)?;
                let vec_ranked: Vec<i64> = vec_hits.iter().map(|h| h.chunk_id).collect();
                // FTS list (best-first) for the same context, same file scope.
                let fts_ranked: Vec<i64> = self
                    .keyword_search_context(cid, raw_query, n, doc_ids)?
                    .into_iter()
                    .map(|(id, _rank)| id)
                    .collect();
                // Fuse the two rank lists for this context, then fold the
                // per-context RRF scores into the global accumulator.
                for sc in rrf_fuse(&[vec_ranked, fts_ranked], RRF_K, n) {
                    *acc.entry(sc.chunk_id).or_insert(0.0) += sc.score;
                }
            }
        }
        let mut merged: Vec<ScoredChunk> = acc
            .into_iter()
            .map(|(chunk_id, score)| ScoredChunk { chunk_id, score })
            .collect();
        merged.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.chunk_id.cmp(&b.chunk_id))
        });
        merged.truncate(top_k);
        Ok(merged)
    }

    /// Grid batch variant of [`Self::retrieve_hybrid_with`]: one hybrid retrieval
    /// per row, each with its own precomputed query vectors and raw query text.
    /// Mirrors [`Self::retrieve_batch`] but preserves the hybrid fusion (and thus
    /// cross-space isolation) for every row.
    pub fn retrieve_hybrid_batch(
        &self,
        context_ids: &[i64],
        queries_by_row: &[HashMap<i64, Vec<f32>>],
        raw_queries: &[String],
        top_k: usize,
        doc_ids: Option<&[i64]>,
    ) -> Result<Vec<Vec<ScoredChunk>>> {
        let mut out = Vec::with_capacity(queries_by_row.len());
        for (row_idx, query_by_model) in queries_by_row.iter().enumerate() {
            let raw = raw_queries.get(row_idx).map(String::as_str).unwrap_or("");
            out.push(self.retrieve_hybrid_with(context_ids, query_by_model, raw, top_k, doc_ids)?);
        }
        Ok(out)
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
        self.retrieve_with(context_ids, &query_by_model, top_k, None)
    }
}
