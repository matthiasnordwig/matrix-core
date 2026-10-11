//! The page-image rank list of hybrid retrieval: page-image vectors of ONE
//! context, scored against the query vector of that context's model, each hit
//! mapped onto a chunk of its page so citations, grid refs, rerank and the
//! tool loop keep working with chunk ids.
//!
//! Invariants (same as the vector list): only vectors with the context's
//! `embedding_model_id` and the query's length are compared (never across
//! embedding spaces); the `doc_ids` scope applies; omitted chunks and chunks
//! without a vector of this model are never chosen. Mapping (BILDSUCHE_PLAN
//! AP0.4, measured best): of the chunks of the hit's page
//! (`page_select::chunks_for_page`), the one whose vector is most similar to
//! the query. A chunk reached by several page images keeps its best rank;
//! a page without a usable chunk drops out.

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::db::embeddings::cosine;
use crate::db::page_images::ChunkSpanRow;
use crate::{Database, Result};

use super::page_select::{chunks_for_page, ChunkSpan};

impl Database {
    /// Best-first `(chunk_id, page_image_id)` list of up to `n` entries for
    /// one context. Chunk positions are loaded at most once per document, and
    /// only for documents that have an image hit.
    pub(crate) fn image_ranked_chunks(
        &self,
        context_id: i64,
        embedding_model_id: i64,
        qvec: &[f32],
        n: usize,
        doc_ids: Option<&[i64]>,
    ) -> Result<Vec<(i64, i64)>> {
        if matches!(doc_ids, Some(d) if d.is_empty()) {
            return Ok(Vec::new());
        }
        let mut hits: Vec<(f32, crate::db::models::StoredPageImageVector)> = self
            .scan_page_image_vectors(context_id, embedding_model_id)?
            .into_iter()
            .filter(|v| v.vector.len() == qvec.len())
            .filter(|v| doc_ids.is_none_or(|d| d.contains(&v.document_id)))
            .map(|v| (cosine(qvec, &v.vector), v))
            .collect();
        if hits.is_empty() {
            return Ok(Vec::new());
        }
        hits.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.1.page_image_id.cmp(&b.1.page_image_id))
        });
        hits.truncate(n);

        let mut docs: HashMap<i64, Vec<ChunkSpan>> = HashMap::new();
        let mut chunk_score: HashMap<i64, Option<f32>> = HashMap::new();
        let mut out: Vec<(i64, i64)> = Vec::new();
        for (_, img) in &hits {
            if let Entry::Vacant(slot) = docs.entry(img.document_id) {
                let rows: Vec<ChunkSpanRow> =
                    self.chunk_spans_with_vectors(img.document_id, embedding_model_id, qvec.len())?;
                let mut spans = Vec::with_capacity(rows.len());
                for (span, omitted, vector) in rows {
                    let score = vector.filter(|_| !omitted).map(|v| cosine(qvec, &v));
                    chunk_score.insert(span.chunk_id, score);
                    spans.push(span);
                }
                slot.insert(spans);
            }
            let range = img.char_start.zip(img.char_end);
            let best = chunks_for_page(&docs[&img.document_id], img.page, range)
                .into_iter()
                .filter_map(|id| chunk_score.get(&id).copied().flatten().map(|s| (id, s)))
                .fold(None::<(i64, f32)>, |acc, (id, s)| match acc {
                    Some((_, bs)) if bs >= s => acc,
                    _ => Some((id, s)),
                });
            if let Some((chunk_id, _)) = best
                && !out.iter().any(|(c, _)| *c == chunk_id)
            {
                out.push((chunk_id, img.page_image_id));
            }
        }
        Ok(out)
    }
}
