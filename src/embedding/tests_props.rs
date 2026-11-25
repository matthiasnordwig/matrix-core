//! Property tests (proptest) for the pure parts of retrieval: fusion, rerank
//! merge, keyword query, page-start carry-over and the vision resize. Case
//! count per test from `PROPTEST_CASES` (stage 2 default here: 32; stage 3:
//! 2000), see `handbuch/arbeitsablauf.md` "Teststufen". Counterexamples are
//! stored in `core/proptest-regressions/embedding/` (checked in) and get a
//! named example test at the end of this file.
//!
//! Rules (from the doc comments at each function):
//! (14) `rrf_fuse_traced_with` with factor 1 is bit-identical to
//!      `rrf_fuse_weighted`; at most `top_k` hits, no chunk twice, scores
//!      descending, ties by ascending chunk id.
//! (15) `rank_merge`: `min(top_k, n)` distinct indices, scores descending,
//!      ties keep the earlier index; NaN scores never panic and rank last.
//! (16) `keyword_query` twice is once; a non-empty query stays non-empty;
//!      without `drop_stopwords` unchanged. `FusionParams::parse` never panics
//!      and keeps every value in its documented range (`w` 0–2, `xw`/`iw`
//!      0–1, `k` 1–1000), also for `NaN`/`inf`.
//! (17) `repair_with_page_starts`: the text is the line-wise
//!      `repair_mojibake`; as many starts as given, non-decreasing, within
//!      the text; a start at a line start maps to that line's repaired start.
//! (18) `target_size`: sides are non-zero multiples of `pool * patch`; the
//!      patch count fits `max_patches` unless the image is degenerate thin
//!      (one side got the single strip); `patchify` + `pad_to` give
//!      `target` patches of `patch² * 3` values in [0, 1].

use proptest::prelude::*;

use super::fusion::{keyword_query, FusionParams};
use super::page_select::repair_with_page_starts;
use super::provenance::{rrf_fuse_traced_with, ListKind};
use super::rerank::rank_merge;
use super::retrieval::rrf_fuse_weighted;
use super::vision_prep::{max_patches, pad_to, patchify, positions, target_size};
use crate::chunking::text_repair::repair_mojibake;

fn score() -> impl Strategy<Value = f32> {
    prop_oneof![
        4 => prop::sample::select(vec![0.0f32, -0.0, 0.5, 1.0, -1.0, 2.5]),
        4 => -10.0f32..10.0,
        1 => Just(f32::NAN),
        1 => Just(f32::INFINITY),
        1 => Just(f32::NEG_INFINITY),
    ]
}

/// Rule (15), shared by the property and the regression tests.
fn check_rank_merge(scores: &[f32], top_k: usize) -> Result<(), TestCaseError> {
    let idx = rank_merge(scores, top_k);
    prop_assert_eq!(idx.len(), top_k.min(scores.len()));
    let mut seen = std::collections::HashSet::new();
    for &i in &idx {
        prop_assert!(i < scores.len() && seen.insert(i), "bad or repeated index {} in {:?}", i, idx);
    }
    // NaN ranks last: as if it were -inf.
    let key = |i: usize| if scores[i].is_nan() { f32::NEG_INFINITY } else { scores[i] };
    for w in idx.windows(2) {
        let (a, b) = (key(w[0]), key(w[1]));
        prop_assert!(a > b || (a == b && w[0] < w[1]), "order broken at {:?} for {:?}", w, scores);
    }
    // Nothing left out beats a kept one.
    if let Some(&last) = idx.last() {
        for i in (0..scores.len()).filter(|i| !seen.contains(i)) {
            prop_assert!(key(i) < key(last) || (key(i) == key(last) && i > last), "{} left out of {:?}", i, idx);
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::props::config())]

    /// Rule (14).
    #[test]
    fn rule_14_traced_fusion_matches_plain_and_is_ordered(
        lists in prop::collection::vec((prop::collection::vec(0i64..25, 0..12), prop_oneof![Just(1.0f32), 0.0f32..2.0]), 0..4),
        k in 0usize..80,
        top_k in 0usize..30,
    ) {
        let plain_in: Vec<(&[i64], f32)> = lists.iter().map(|(l, w)| (l.as_slice(), *w)).collect();
        let kinds = [Some(ListKind::Vector), Some(ListKind::Keyword), Some(ListKind::Image), None];
        let traced_in: Vec<(Option<ListKind>, &[i64], f32)> =
            lists.iter().enumerate().map(|(i, (l, w))| (kinds[i % 4], l.as_slice(), *w)).collect();
        let plain = rrf_fuse_weighted(&plain_in, k, top_k);
        let traced = rrf_fuse_traced_with(&traced_in, k, top_k, |_, _| 1.0);
        prop_assert_eq!(plain.len(), traced.len());
        for (p, t) in plain.iter().zip(&traced) {
            prop_assert_eq!(p.chunk_id, t.chunk_id);
            prop_assert_eq!(p.score.to_bits(), t.score.to_bits());
        }
        prop_assert!(traced.len() <= top_k);
        let ids: std::collections::HashSet<i64> = traced.iter().map(|h| h.chunk_id).collect();
        prop_assert_eq!(ids.len(), traced.len());
        for w in traced.windows(2) {
            prop_assert!(w[0].score > w[1].score || (w[0].score == w[1].score && w[0].chunk_id < w[1].chunk_id));
        }
    }

    /// Rule (15).
    #[test]
    fn rule_15_rank_merge_ordered_and_nan_safe(scores in prop::collection::vec(score(), 0..40), top_k in 0usize..45) {
        check_rank_merge(&scores, top_k)?;
    }

    /// Rule (16).
    #[test]
    fn rule_16_keyword_query_and_params(
        q in prop_oneof![
            prop::collection::vec(prop::sample::select(vec!["der", "Die", "und", "KWG", "§", "25a", "the", "of", "Art.", "über", " ", "  ", "\t", "x"]), 0..10).prop_map(|v| v.join(" ")),
            any::<String>(),
        ],
        drop in any::<bool>(),
        spec in prop_oneof![
            prop::collection::vec((prop::sample::select(vec!["w", "xw", "iw", "k", "stop", "img", "scan", "?"]),
                prop::sample::select(vec!["NaN", "inf", "-inf", "-1", "0", "1", "0.5", "3", "5000", "x", ""])), 0..6)
                .prop_map(|v| v.iter().map(|(a, b)| format!("{a}={b}")).collect::<Vec<_>>().join(",")),
            any::<String>(),
        ],
    ) {
        let once = keyword_query(&q, drop);
        prop_assert_eq!(keyword_query(&once, drop), once.clone(), "not idempotent for {:?}", q);
        prop_assert!(q.is_empty() || !once.is_empty());
        if !drop {
            prop_assert_eq!(&once, &q);
        }
        let p = FusionParams::parse(&spec);
        prop_assert!((0.0..=2.0).contains(&p.fts_weight), "w = {} from {:?}", p.fts_weight, spec);
        prop_assert!((0.0..=1.0).contains(&p.cross_lang_fts), "xw = {} from {:?}", p.cross_lang_fts, spec);
        prop_assert!((0.0..=1.0).contains(&p.img_no_keyword), "iw = {} from {:?}", p.img_no_keyword, spec);
        prop_assert!((1..=1000).contains(&p.k), "k = {} from {:?}", p.k, spec);
    }

    /// Rule (17).
    #[test]
    fn rule_17_page_starts_carry_over(
        raw in prop_oneof![
            "[a-zâ€‰Â\u{ad}Ã¤ \n]{0,40}",
            any::<String>(),
        ],
        starts in prop::collection::vec(0usize..60, 0..6),
    ) {
        let (text, mapped) = repair_with_page_starts(&raw, &starts);
        let want = raw.split('\n').map(repair_mojibake).collect::<Vec<_>>().join("\n");
        prop_assert_eq!(&text, &want);
        prop_assert_eq!(mapped.len(), starts.len());
        prop_assert!(mapped.windows(2).all(|w| w[0] <= w[1]), "falls: {:?}", mapped);
        prop_assert!(mapped.iter().all(|&m| m <= text.len()));
        // A start at a line start, with no earlier start beyond it, maps to that line's repaired start.
        let line_starts: Vec<usize> = std::iter::once(0).chain(raw.match_indices('\n').map(|(i, _)| i + 1)).collect();
        let rep_starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
        for (k, &s) in starts.iter().enumerate() {
            let rising = starts[..k].iter().all(|&e| e <= s);
            if let Some(line) = line_starts.iter().position(|&l| l == s).filter(|_| rising && s < raw.len()) {
                prop_assert_eq!(mapped[k], rep_starts[line], "start {} of {:?}", s, raw);
            }
        }
    }

    /// Rule (18).
    #[test]
    fn rule_18_vision_resize_and_patches(
        h in 0usize..20_000,
        w in 0usize..20_000,
        patch in 1usize..20,
        pool in 1usize..5,
        soft in 1usize..300,
        small in (1usize..4, 1usize..4, 1usize..5),
    ) {
        let maxp = max_patches(soft, pool);
        let (th, tw) = target_size(h, w, patch, maxp, pool);
        let side = pool * patch;
        prop_assert!(th > 0 && tw > 0 && th % side == 0 && tw % side == 0, "({}, {}) for {}x{} side {}", th, tw, h, w, side);
        let thin = th == side || tw == side;
        prop_assert!(thin || (th / patch) * (tw / patch) <= maxp, "{} patches > {}", (th / patch) * (tw / patch), maxp);

        let (rows, cols, p) = small;
        let rgb: Vec<u8> = (0..rows * p * cols * p * 3).map(|i| (i * 37 % 256) as u8).collect();
        let mut patches = patchify(&rgb, rows * p, cols * p, p);
        let mut pos = positions(rows, cols);
        let target = rows * cols + 2;
        pad_to(&mut patches, &mut pos, p * p * 3, target);
        prop_assert_eq!(patches.len(), target * p * p * 3);
        prop_assert_eq!(pos.len(), target * 2);
        prop_assert!(patches.iter().all(|v| (0.0..=1.0).contains(v)));
    }
}

// Counterexamples proptest found, replayed verbatim (seeds in
// `core/proptest-regressions/embedding/tests_props.txt`).

/// Rule (15): a NaN score compared "equal" to everything, so the earlier NaN
/// candidate took the only place instead of the scored one.
#[test]
fn regression_rule_15_nan_score_does_not_win() {
    assert_eq!(rank_merge(&[f32::NAN, 0.0], 1), vec![1]);
    check_rank_merge(&[f32::NAN, 0.0], 1).unwrap();
}

/// Rule (16): `NaN` parses as an `f32` and passes `clamp` unchanged, so
/// `MATRIX_HYBRID_FUSION=w=NaN` made every keyword score NaN.
#[test]
fn regression_rule_16_nan_weight_keeps_default() {
    let p = FusionParams::parse("w=NaN,xw=NaN,iw=inf");
    assert_eq!(p.fts_weight, FusionParams::default().fts_weight);
    assert_eq!(p.cross_lang_fts, FusionParams::default().cross_lang_fts);
    assert_eq!(p.img_no_keyword, 1.0);
}
