//! Weighted Reciprocal Rank Fusion that remembers where each hit came from,
//! and the page-image preview rule built on it.
//!
//! [`rrf_fuse_traced`] is the fusion of the hybrid path: a chunk's score is
//! `Σ weight / (k + rank)` over the lists it appears in, accumulated in list
//! order — bit-identical to the untraced `retrieval::rrf_fuse_weighted` — and
//! for each tagged list ([`ListKind`]) the chunk's 1-based rank and its
//! contribution go into `ScoredChunk::provenance`.
//!
//! Preview rule ([`shows_page_preview`]): a hit carries its page image only
//! when its rank in the image list (`provenance.image.rank`, i.e. the place of
//! its page in the image vector search) is within [`IMAGE_PREVIEW_RANK_FRACTION`]
//! of the visible `preview_k` (the final top_k, not a rerank candidate count),
//! rounded up — rank ≤ 3 at top_k 10. A page that far up in the image search
//! explains the hit whether or not the text search found the chunk too.
//! Measured with `services/examples/image_e2e.rs` against rank ≤ 1/5/top_k and
//! against an image-share rule (≥ 50 % of the fused score): rank ≤ 3 shows as
//! many previews as the share rule (≈ 2.6 per question) but 17 instead of 3–5
//! of them on the gold page — gold pages are usually found by text AND image,
//! so their image share stays below half (numbers in `handbuch/core.md`,
//! „Bildsuche in der Hybrid-Fusion"). The shares stay in the provenance (UI
//! tooltip). Ranks and scores never depend on the rule.

use std::collections::HashMap;

use crate::db::models::{HitProvenance, ListHit, ScoredChunk};

/// Fraction of the visible result count within which an image rank earns a
/// page-image preview (rounded up, at least 1).
pub const IMAGE_PREVIEW_RANK_FRACTION: f32 = 0.3;

/// The rank list a fused contribution came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListKind {
    Vector,
    Keyword,
    Image,
}

/// Whether a hit with this provenance should show its page image, given the
/// visible result count `preview_k` (see the module comment).
pub fn shows_page_preview(p: &HitProvenance, preview_k: usize) -> bool {
    let max_rank = ((preview_k as f32 * IMAGE_PREVIEW_RANK_FRACTION).ceil() as usize).max(1);
    preview_k > 0 && p.image.is_some_and(|img| (img.rank as usize) <= max_rank)
}

/// Weighted RRF over `(kind, list, weight)`, best-first lists of chunk ids.
/// Untagged lists (`None`) add to the score but not to the provenance. Sorted
/// by score desc, ties by ascending chunk id, truncated to `top_k`.
pub(crate) fn rrf_fuse_traced(lists: &[(Option<ListKind>, &[i64], f32)], k: usize, top_k: usize) -> Vec<ScoredChunk> {
    let mut acc: HashMap<i64, ScoredChunk> = HashMap::new();
    for (kind, list, weight) in lists {
        for (i, &chunk_id) in list.iter().enumerate() {
            let rank = i + 1; // 1-based
            let share = weight / (k + rank) as f32;
            let hit = acc.entry(chunk_id).or_insert_with(|| ScoredChunk::new(chunk_id, 0.0));
            hit.score += share;
            let slot = match kind {
                Some(ListKind::Vector) => &mut hit.provenance.vector,
                Some(ListKind::Keyword) => &mut hit.provenance.keyword,
                Some(ListKind::Image) => &mut hit.provenance.image,
                None => continue,
            };
            let rank = rank as u32;
            *slot = Some(match *slot {
                Some(h) => ListHit { rank: h.rank.min(rank), share: h.share + share },
                None => ListHit { rank, share },
            });
        }
    }
    let mut fused: Vec<ScoredChunk> = acc.into_values().collect();
    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.chunk_id.cmp(&b.chunk_id))
    });
    fused.truncate(top_k);
    fused
}
