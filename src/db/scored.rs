//! Retrieval hit types: [`ScoredChunk`] and its fusion provenance
//! ([`HitProvenance`], [`ListHit`]). Re-exported from `models.rs` (so they stay
//! `models::ScoredChunk` etc. like every other domain type); a file of their
//! own only because `models.rs` sits at its line cap.
//!
//! Provenance is filled by the hybrid fusion (`embedding/retrieval.rs`,
//! `embedding/provenance.rs`): for each of the three rank lists (vector,
//! keyword, page image) the chunk's best 1-based rank and its summed RRF
//! contribution (`share`). The shares add up to the fusion score; they stay
//! valid after a reranker replaces `score`, which is why the image fraction
//! is computed from the shares, not from `score`. Pure vector retrieval
//! leaves the provenance empty. Serialised fields are omitted when empty, so
//! JSON of non-hybrid hits is unchanged.

use serde::{Deserialize, Serialize};

/// A scored retrieval hit returned by a cosine scan or the hybrid fusion.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScoredChunk {
    pub chunk_id: i64,
    pub score: f32,
    /// Set when the page-image rank list is the main reason for the hit
    /// (hybrid retrieval, image fraction of the fused score ≥ the preview
    /// threshold): the best-ranked page image mapped onto this chunk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_image_id: Option<i64>,
    /// Ranks and RRF contributions per fused list (empty outside the hybrid path).
    #[serde(default, skip_serializing_if = "HitProvenance::is_empty")]
    pub provenance: HitProvenance,
}

impl ScoredChunk {
    /// A hit without page image and provenance.
    pub fn new(chunk_id: i64, score: f32) -> Self {
        Self { chunk_id, score, ..Default::default() }
    }
}

/// One list's part of a fused hit: 1-based rank in that list and the RRF
/// contribution `weight / (k + rank)` it added to the score.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ListHit {
    pub rank: u32,
    pub share: f32,
}

impl ListHit {
    /// Combine two contributions of the same list (several contexts): best
    /// rank, summed share.
    fn merge(a: Option<ListHit>, b: Option<ListHit>) -> Option<ListHit> {
        match (a, b) {
            (Some(x), Some(y)) => Some(ListHit { rank: x.rank.min(y.rank), share: x.share + y.share }),
            (x, None) => x,
            (None, y) => y,
        }
    }
}

/// Which fused lists reached a hit, and how much each contributed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct HitProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<ListHit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyword: Option<ListHit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ListHit>,
}

impl HitProvenance {
    pub fn is_empty(&self) -> bool {
        self.vector.is_none() && self.keyword.is_none() && self.image.is_none()
    }

    /// Fold another context's provenance of the same chunk into this one.
    pub fn absorb(&mut self, other: &HitProvenance) {
        self.vector = ListHit::merge(self.vector, other.vector);
        self.keyword = ListHit::merge(self.keyword, other.keyword);
        self.image = ListHit::merge(self.image, other.image);
    }

    /// Sum of all list contributions (= the fusion score), summed in list
    /// order vector → keyword → image like the fusion itself.
    pub fn total_share(&self) -> f32 {
        [self.vector, self.keyword, self.image].iter().flatten().fold(0.0, |acc, h| acc + h.share)
    }

    /// Image contribution as a fraction of [`Self::total_share`]; `None`
    /// without an image contribution or with a zero total.
    pub fn image_fraction(&self) -> Option<f32> {
        let img = self.image?;
        let total = self.total_share();
        (total > 0.0).then(|| img.share / total)
    }
}
