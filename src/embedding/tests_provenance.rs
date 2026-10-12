//! Fusion provenance (`provenance.rs`, `db/scored.rs`): traced RRF is
//! bit-identical to the untraced reference, ranks are 1-based list positions,
//! shares add up to the score (also across contexts in the hybrid path), and
//! the preview rule needs an image rank within 30 % of `preview_k` (rounded
//! up), whatever the text ranks.

use std::collections::HashMap;

use super::fusion::FusionParams;
use super::provenance::{rrf_fuse_traced, shows_page_preview, ListKind};
use super::retrieval::{hybrid_fanout, rrf_fuse_weighted};
use super::tests::{context_with_vectors, model};
use crate::db::models::{HitProvenance, ListHit, ScoredChunk};
use crate::Database;

const K: usize = 10;

/// Provenance of chunk `id` after fusing a vector, keyword and image list.
fn fused(vector: &[i64], keyword: &[i64], image: &[i64], id: i64) -> ScoredChunk {
    let lists = [
        (Some(ListKind::Vector), vector, 1.0),
        (Some(ListKind::Keyword), keyword, 1.0),
        (Some(ListKind::Image), image, 1.0),
    ];
    rrf_fuse_traced(&lists, K, 1000).into_iter().find(|s| s.chunk_id == id).unwrap()
}

/// A best-first list of `len` filler ids (1000+) with `id` at 1-based `rank`.
fn list_with(id: i64, rank: usize, len: usize) -> Vec<i64> {
    let mut l: Vec<i64> = (0..len as i64).map(|i| 1000 + i).collect();
    l.insert(rank - 1, id);
    l
}

#[test]
fn traced_fusion_is_bit_identical_to_the_reference_and_records_ranks() {
    let v = vec![5, 3, 9, 1, 7];
    let t = vec![9, 2, 5];
    let i = vec![7, 5];
    let reference = rrf_fuse_weighted(&[(&v, 1.0), (&t, 0.7), (&i, 1.0)], K, 10);
    let traced = rrf_fuse_traced(
        &[(Some(ListKind::Vector), &v, 1.0), (Some(ListKind::Keyword), &t, 0.7), (Some(ListKind::Image), &i, 1.0)],
        K,
        10,
    );
    let key = |x: &[ScoredChunk]| x.iter().map(|s| (s.chunk_id, s.score.to_bits())).collect::<Vec<_>>();
    assert_eq!(key(&traced), key(&reference));
    let five = traced.iter().find(|s| s.chunk_id == 5).unwrap();
    assert_eq!(five.provenance.vector.map(|h| h.rank), Some(1));
    assert_eq!(five.provenance.keyword.map(|h| h.rank), Some(3));
    assert_eq!(five.provenance.image.map(|h| h.rank), Some(2));
    assert_eq!(five.provenance.keyword.unwrap().share, 0.7 / 13.0);
    for s in &traced {
        // Same summation order as the fusion → exactly the score.
        assert_eq!(s.provenance.total_share().to_bits(), s.score.to_bits(), "chunk {}", s.chunk_id);
    }
    let three = traced.iter().find(|s| s.chunk_id == 3).unwrap();
    assert_eq!((three.provenance.keyword, three.provenance.image), (None, None));
    // Untagged lists count for the score only.
    let untagged = rrf_fuse_traced(&[(None, &v, 1.0)], K, 10);
    assert!(untagged.iter().all(|s| s.provenance.is_empty() && s.score > 0.0));
}

#[test]
fn image_first_vector_fortieth_shows_a_preview() {
    let hit = fused(&list_with(1, 40, 45), &[], &list_with(1, 1, 5), 1);
    let f = hit.provenance.image_fraction().unwrap();
    assert!((f - (1.0 / 11.0) / (1.0 / 11.0 + 1.0 / 50.0)).abs() < 1e-6);
    assert!(f > 0.8);
    assert!(shows_page_preview(&hit.provenance, 10));
}

#[test]
fn vector_first_image_eighth_shows_no_preview_at_top_k_10() {
    let hit = fused(&list_with(1, 1, 5), &[], &list_with(1, 8, 10), 1);
    assert!(!shows_page_preview(&hit.provenance, 10), "image rank 8 > 3");
    assert!(shows_page_preview(&hit.provenance, 27), "≤ 9 at preview_k 27");
}

#[test]
fn image_only_hit_needs_the_rank_too() {
    // No text hit at all: 100 % image share, but image rank 12 > 3 at top_k 10.
    let hit = fused(&[], &[], &list_with(1, 12, 20), 1);
    assert_eq!(hit.provenance.image_fraction(), Some(1.0));
    assert!(!shows_page_preview(&hit.provenance, 10));
    assert!(shows_page_preview(&hit.provenance, 40), "≤ 12 at preview_k 40 (limit inclusive)");
}

#[test]
fn image_rank_decides_whatever_the_text_found() {
    let p = |img: Option<u32>, vec_rank: u32| HitProvenance {
        vector: Some(ListHit { rank: vec_rank, share: 1.0 / (10 + vec_rank) as f32 }),
        keyword: None,
        image: img.map(|rank| ListHit { rank, share: 1.0 / (10 + rank) as f32 }),
    };
    assert!(shows_page_preview(&p(Some(3), 1), 10), "text rank 1 doesn't block an image rank 3");
    assert!(!shows_page_preview(&p(Some(4), 40), 10), "image rank 4 > 3 even with weak text");
    assert!(shows_page_preview(&p(Some(1), 1), 1), "at least rank 1");
    assert!(!shows_page_preview(&p(Some(1), 1), 0), "preview_k 0 = none");
    assert!(!shows_page_preview(&p(None, 1), 10), "no image contribution");
}

#[test]
fn absorb_keeps_best_rank_and_sums_shares() {
    let mut a = HitProvenance { vector: Some(ListHit { rank: 4, share: 0.1 }), keyword: None, image: None };
    let b = HitProvenance {
        vector: Some(ListHit { rank: 2, share: 0.2 }),
        keyword: Some(ListHit { rank: 7, share: 0.05 }),
        image: None,
    };
    a.absorb(&b);
    assert_eq!(a.vector, Some(ListHit { rank: 2, share: 0.1 + 0.2 }));
    assert_eq!(a.keyword, Some(ListHit { rank: 7, share: 0.05 }));
    assert_eq!(a.image, None);
}

#[test]
fn empty_provenance_is_left_out_of_json_and_old_json_still_loads() {
    let plain = serde_json::to_string(&ScoredChunk::new(3, 0.5)).unwrap();
    assert_eq!(plain, r#"{"chunk_id":3,"score":0.5}"#);
    let old: ScoredChunk = serde_json::from_str(r#"{"chunk_id":3,"score":0.5}"#).unwrap();
    assert!(old.provenance.is_empty() && old.page_image_id.is_none());
    let mut hit = ScoredChunk::new(3, 0.5);
    hit.provenance.image = Some(ListHit { rank: 1, share: 0.5 });
    let v: serde_json::Value = serde_json::to_value(&hit).unwrap();
    assert_eq!(v["provenance"]["image"]["rank"], 1);
    assert!(v["provenance"].get("vector").is_none());
}

#[test]
fn hybrid_hits_carry_list_ranks_and_shares_summing_to_the_score() {
    let db = Database::open_in_memory().unwrap();
    let ma = model(&db, "a", 3);
    let mb = model(&db, "b", 3);
    let a = context_with_vectors(&db, "A", ma.id, 3, &[vec![1.0, 0.0, 0.0], vec![0.6, 0.8, 0.0], vec![0.0, 1.0, 0.0]]);
    let _ = context_with_vectors(&db, "B", mb.id, 3, &[vec![0.0, 1.0, 0.0], vec![1.0, 0.0, 0.0]]);
    let ctxs: Vec<i64> = db.list_contexts().unwrap().iter().map(|c| c.id).collect();
    let q = HashMap::from([(ma.id, vec![1.0, 0.0, 0.0]), (mb.id, vec![1.0, 0.0, 0.0])]);
    let hits = db.retrieve_hybrid_with(&ctxs, &q, "abschnitt chunk", 10, None).unwrap();
    assert!(!hits.is_empty());
    for h in &hits {
        assert!(!h.provenance.is_empty() && h.provenance.image.is_none());
        assert!((h.provenance.total_share() - h.score).abs() < 1e-6, "chunk {}", h.chunk_id);
    }
    // Vector ranks are the 1-based positions in the context's own vector list.
    let ctx_a = db.chunk(a[0]).unwrap().unwrap().context_id;
    let n = hybrid_fanout(10);
    let order = db.search_context(ctx_a, &q[&ma.id], n, None).unwrap();
    for (i, sc) in order.iter().enumerate() {
        let h = hits.iter().find(|h| h.chunk_id == sc.chunk_id).unwrap();
        assert_eq!(h.provenance.vector.unwrap().rank as usize, i + 1);
        assert_eq!(h.provenance.vector.unwrap().share, 1.0 / (FusionParams::default().k + i + 1) as f32);
    }
    // Pure vector retrieval leaves provenance empty.
    assert!(db.retrieve_with(&ctxs, &q, 10, None).unwrap().iter().all(|h| h.provenance.is_empty()));
}
