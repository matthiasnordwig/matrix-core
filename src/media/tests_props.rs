//! Property tests (proptest) for the media text and the speaker clustering. Case count from
//! `crate::props` (`PROPTEST_CASES`, stage 2: 32), see `handbuch/arbeitsablauf.md` "Teststufen".
//! Counterexamples: `core/proptest-regressions/media/` (checked in) plus a named example test.
//!
//! Rules (from the doc comments in `text.rs` and `diarize_cluster.rs`):
//! (22) `render`: each placed range holds exactly its section's trimmed body ("a passage is
//!      exactly its body, so `text[start..end] == body`"), in the given order, on char boundaries.
//! (23) `cluster`: one label per vector, labels 0, 1, … in order of first appearance, at most
//!      `MAX_SPEAKERS` of them; no panic for zero vectors (n kept ≤ 40: the merge is O(n²)).

use proptest::prelude::*;

use super::diarize_cluster::{cluster, MAX_SPEAKERS};
use super::text::{render, Section};
use crate::db::models::SegmentKind;

fn section() -> impl Strategy<Value = Section> {
    (
        prop::sample::select(vec![SegmentKind::Speech, SegmentKind::Subtitle, SegmentKind::Slide, SegmentKind::Scene, SegmentKind::Sound]),
        any::<i64>(),
        any::<i64>(),
        prop::collection::vec(".{0,6}", 0..3),
        prop_oneof![".{0,30}", "[ \n\ta-zä€#]{0,30}"],
        any::<bool>(),
    )
        .prop_map(|(kind, t0_ms, t1_ms, tags, body, generated)| Section { kind, t0_ms, t1_ms, tags, body, generated })
}

proptest! {
    #![proptest_config(crate::props::config())]

    /// Rule (22).
    #[test]
    fn rule_22_render_places_each_body_exactly(
        name in ".{0,12}",
        duration in prop_oneof![0i64..3_600_000, 3_600_000i64..i64::MAX],
        sections in prop::collection::vec(section(), 0..6),
    ) {
        let r = render(&name, duration, &sections);
        prop_assert_eq!(r.placed.len(), sections.len());
        let mut prev = 0;
        for (p, s) in r.placed.iter().zip(&sections) {
            prop_assert!(prev <= p.start && p.start <= p.end && p.end <= r.text.len());
            prop_assert!(r.text.is_char_boundary(p.start) && r.text.is_char_boundary(p.end));
            prop_assert_eq!(&r.text[p.start..p.end], s.body.trim());
            prev = p.end;
        }
    }

    /// Rule (23).
    #[test]
    fn rule_23_cluster_labels_in_order_of_appearance(
        vectors in prop::collection::vec(
            prop_oneof![
                4 => prop::collection::vec(-1.0f32..1.0, 4),
                1 => Just(vec![0.0f32; 4]),
                1 => prop::sample::select(vec![vec![1.0f32, 0.0, 0.0, 0.0], vec![0.0f32, 1.0, 0.0, 0.0]]),
            ],
            0..40,
        ),
        min_group in 0usize..6,
    ) {
        let labels = cluster(&vectors, min_group);
        prop_assert_eq!(labels.len(), vectors.len());
        let mut next = 0;
        for &l in &labels {
            prop_assert!(l <= next, "label {} before {} appeared: {:?}", l, next, labels);
            if l == next {
                next += 1;
            }
        }
        prop_assert!(next <= MAX_SPEAKERS);
    }
}
