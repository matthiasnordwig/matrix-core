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
//!
//! `retrieve_hybrid_*` additionally fuse the vector list with the FTS5/BM25 list
//! per context via Reciprocal Rank Fusion (ranks only, never scores across
//! spaces; keyword query and list weight per `fusion.rs`); `retrieve_with` stays the pure vector path. Every retrieve fn takes
//! `doc_ids` (file-level scope); in the hybrid path the same filter is applied to
//! ALL lists before fusion, otherwise out-of-scope chunks leak.
//!
//! The hybrid path fuses a third list per context where page-image vectors of
//! the context's model exist (`image_list.rs`): graphics pages scored in the
//! same space, each mapped onto a chunk of its page; such hits carry
//! `ScoredChunk::page_image_id` — but only if the image rank within its
//! context is below the *visible* `preview_k` (a weak image hit still adds its
//! RRF share, it just gets no preview). Ranks and scores never depend on
//! `preview_k`. Without image vectors the result is bit-identical to the
//! two-list fusion.

use std::collections::HashMap;

use super::QueryEmbedder;
use crate::db::models::ScoredChunk;
use crate::{Database, Result};

/// Textbook Reciprocal Rank Fusion constant, used by `rrf_fuse`. The hybrid
/// path takes its k from `fusion::FusionParams` (measured: 10); test-only now.
#[cfg(test)]
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
/// Unweighted reference form, kept for the fusion-math tests.
#[cfg(test)]
pub(crate) fn rrf_fuse(lists: &[Vec<i64>], k: usize, top_k: usize) -> Vec<ScoredChunk> {
    let weighted: Vec<(&[i64], f32)> = lists.iter().map(|l| (l.as_slice(), 1.0)).collect();
    rrf_fuse_weighted(&weighted, k, top_k)
}

/// [`rrf_fuse`] with a weight per list: a chunk's score is
/// `Σ w_list / (k + rank)`. Weight 1.0 everywhere is plain RRF.
pub(crate) fn rrf_fuse_weighted(lists: &[(&[i64], f32)], k: usize, top_k: usize) -> Vec<ScoredChunk> {
    let mut acc: HashMap<i64, f32> = HashMap::new();
    for (list, weight) in lists {
        for (i, &chunk_id) in list.iter().enumerate() {
            let rank = i + 1; // 1-based
            *acc.entry(chunk_id).or_insert(0.0) += weight / (k + rank) as f32;
        }
    }
    let mut fused: Vec<ScoredChunk> = acc
        .into_iter()
        .map(|(chunk_id, score)| ScoredChunk { chunk_id, score, page_image_id: None })
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
                                page_image_id: None,
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

    /// Hybrid retrieval: Reciprocal Rank Fusion of the per-context vector list,
    /// the per-context FTS5/BM25 keyword list and — where page-image vectors
    /// exist — the per-context page-image list (mapped onto chunks). Fusion
    /// happens **per context (per embedding space)** and only on ranks — no
    /// cross-space score comparison — then the RRF scores accumulate globally
    /// across all (model, context) pairs before the final top-`k` cut. The RRF
    /// score is placed in `ScoredChunk::score`. Parameters from
    /// `FusionParams::current()`. The page-image preview threshold is `top_k`
    /// (use [`Self::retrieve_hybrid_with_preview`] when `top_k` is a rerank
    /// candidate count larger than what the user finally sees).
    pub fn retrieve_hybrid_with(
        &self,
        context_ids: &[i64],
        query_by_model: &HashMap<i64, Vec<f32>>,
        raw_query: &str,
        top_k: usize,
        doc_ids: Option<&[i64]>,
    ) -> Result<Vec<ScoredChunk>> {
        self.retrieve_hybrid_with_preview(context_ids, query_by_model, raw_query, top_k, top_k, doc_ids)
    }

    /// [`Self::retrieve_hybrid_with`] with `top_k` = how many hits to return
    /// (a rerank candidate count N) and `preview_k` = the FINAL visible count:
    /// only chunks reached by a page image whose rank in the image list is
    /// below `preview_k` carry `page_image_id`. Ranks/scores are the same for
    /// every `preview_k`.
    pub fn retrieve_hybrid_with_preview(
        &self,
        context_ids: &[i64],
        query_by_model: &HashMap<i64, Vec<f32>>,
        raw_query: &str,
        top_k: usize,
        preview_k: usize,
        doc_ids: Option<&[i64]>,
    ) -> Result<Vec<ScoredChunk>> {
        let fusion = super::fusion::FusionParams::current();
        self.retrieve_hybrid_with_params(context_ids, query_by_model, raw_query, top_k, preview_k, doc_ids, &fusion)
    }

    /// [`Self::retrieve_hybrid_with_preview`] with explicit fusion parameters.
    #[allow(clippy::too_many_arguments)]
    pub fn retrieve_hybrid_with_params(
        &self,
        context_ids: &[i64],
        query_by_model: &HashMap<i64, Vec<f32>>,
        raw_query: &str,
        top_k: usize,
        preview_k: usize,
        doc_ids: Option<&[i64]>,
        fusion: &super::fusion::FusionParams,
    ) -> Result<Vec<ScoredChunk>> {
        let n = hybrid_fanout(top_k);
        let keyword_query = super::fusion::keyword_query(raw_query, fusion.drop_stopwords);
        // Accumulate RRF scores globally across contexts/spaces.
        let mut acc: HashMap<i64, f32> = HashMap::new();
        // Chunk → best-ranked page image that contributed to it, only among image
        // ranks < `preview_k` (weaker hits fuse but get no preview).
        let mut page_image_of: HashMap<i64, i64> = HashMap::new();
        for (model_id, ctxs) in self.contexts_by_model(context_ids)? {
            let Some(qvec) = query_by_model.get(&model_id) else {
                continue;
            };
            for cid in ctxs {
                // Vector list (best-first) restricted to this one context/space.
                // The SAME `doc_ids` filter is applied to ALL lists below, so
                // they are scoped identically before RRF fusion — a leak here
                // would surface out-of-scope chunks with valid provenance (the
                // `is_omitted` leak class).
                let vec_hits = self.search_context(cid, qvec, n, doc_ids)?;
                let vec_ranked: Vec<i64> = vec_hits.iter().map(|h| h.chunk_id).collect();
                // FTS list (best-first) for the same context, same file scope.
                let fts_ranked: Vec<i64> = self
                    .keyword_search_context(cid, &keyword_query, n, doc_ids)?
                    .into_iter()
                    .map(|(id, _rank)| id)
                    .collect();
                // Page-image list (best-first, already mapped onto chunks), only
                // vectors of this context's model — same space as `qvec`.
                let img_ranked = if fusion.img {
                    self.image_ranked_chunks(cid, model_id, qvec, n, doc_ids)?
                } else {
                    Vec::new()
                };
                let img_ids: Vec<i64> = img_ranked.iter().map(|(c, _)| *c).collect();
                // Fuse the rank lists for this context, then fold the
                // per-context RRF scores into the global accumulator. An empty
                // image list is left out entirely (bit-identical to before).
                let mut lists = vec![(vec_ranked.as_slice(), 1.0), (fts_ranked.as_slice(), fusion.fts_weight)];
                if !img_ids.is_empty() {
                    lists.push((img_ids.as_slice(), 1.0));
                }
                for sc in rrf_fuse_weighted(&lists, fusion.k, n) {
                    *acc.entry(sc.chunk_id).or_insert(0.0) += sc.score;
                }
                for (chunk_id, page_image_id) in img_ranked.into_iter().take(preview_k) {
                    page_image_of.entry(chunk_id).or_insert(page_image_id);
                }
            }
        }
        let mut merged: Vec<ScoredChunk> = acc
            .into_iter()
            .map(|(chunk_id, score)| ScoredChunk {
                chunk_id,
                score,
                page_image_id: page_image_of.get(&chunk_id).copied(),
            })
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
        self.retrieve_hybrid_batch_preview(context_ids, queries_by_row, raw_queries, top_k, top_k, doc_ids)
    }

    /// [`Self::retrieve_hybrid_batch`] with a separate visible `preview_k`
    /// (see [`Self::retrieve_hybrid_with_preview`]).
    pub fn retrieve_hybrid_batch_preview(
        &self,
        context_ids: &[i64],
        queries_by_row: &[HashMap<i64, Vec<f32>>],
        raw_queries: &[String],
        top_k: usize,
        preview_k: usize,
        doc_ids: Option<&[i64]>,
    ) -> Result<Vec<Vec<ScoredChunk>>> {
        let mut out = Vec::with_capacity(queries_by_row.len());
        for (row_idx, query_by_model) in queries_by_row.iter().enumerate() {
            let raw = raw_queries.get(row_idx).map(String::as_str).unwrap_or("");
            out.push(self.retrieve_hybrid_with_preview(context_ids, query_by_model, raw, top_k, preview_k, doc_ids)?);
        }
        Ok(out)
    }

    /// Cosine-score an explicit set of chunk ids against `query_by_model`
    /// (TOOL_CALLS_V2_PLAN AP4: soft re-ranking for `find_citing_chunks`, whose
    /// candidates come from a reverse `chunk_refs` lookup rather than a
    /// context-wide scan). Each chunk is scored in its OWN embedding space —
    /// looked up via its context's model — so this never compares across
    /// spaces; a chunk whose context has no entry in `query_by_model`, or whose
    /// stored vector is missing, is scored `0.0` (excluded from nothing, just
    /// sorts last). Not a top-k scan: callers with only a handful of candidate
    /// ids (a reverse-ref result set, not a whole context) use this instead of
    /// `retrieve_with`.
    pub fn score_chunks_by_ids(
        &self,
        chunk_ids: &[i64],
        query_by_model: &HashMap<i64, Vec<f32>>,
    ) -> Result<HashMap<i64, f32>> {
        let chunks = self.chunks_by_ids(chunk_ids)?;
        let mut out = HashMap::with_capacity(chunks.len());
        for c in chunks {
            let score = self
                .context(c.context_id)?
                .and_then(|ctx| ctx.embedding_model_id)
                .and_then(|model_id| query_by_model.get(&model_id))
                .and_then(|qvec| self.embedding_vector(c.id).ok().flatten().map(|v| (qvec, v)))
                .map(|(qvec, v)| crate::db::embeddings::cosine(qvec, &v))
                .unwrap_or(0.0);
            out.insert(c.id, score);
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
                let q = super::prefix::apply_task_prefix(&model, super::prefix::EmbedRole::Query, query);
                query_by_model.insert(model_id, embedder.embed_query(&model, &q)?);
            }
        }
        self.retrieve_with(context_ids, &query_by_model, top_k, None)
    }
}
