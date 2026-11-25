//! Property tests (proptest) for the prompt-prefix reuse plan. Case count from `crate::props`
//! (`PROPTEST_CASES`, stage 2: 32), see `handbuch/arbeitsablauf.md` "Teststufen".
//!
//! Rules (from the doc comments in `prefix_reuse.rs`):
//! (29) `plan`: at least one prompt token is always read again; what is kept is a shared
//!      beginning of `cached` and `next`; a model with memory that cannot be cut back
//!      (`partial`) is only cut when nothing of `cached` is dropped, else it restores a saved
//!      state at or below the shared part. `retain`: sorted, distinct, a subset of the input,
//!      at most `MAX_CHECKPOINTS`, keeps the earliest, the pinned and the latest position.

use proptest::prelude::*;

use super::prefix_reuse::{plan, retain, shared_len, Reuse, MAX_CHECKPOINTS};

proptest! {
    #![proptest_config(crate::props::config())]

    /// Rule (29).
    #[test]
    fn rule_29_plan_and_retain(
        cached in prop::collection::vec(0u8..3, 0..20),
        next in prop::collection::vec(0u8..3, 0..20),
        partial in any::<bool>(),
        checkpoints in prop::collection::vec(0usize..25, 0..6),
        positions in prop::collection::vec(0usize..50, 0..10),
        pin in prop::option::of(0usize..10),
    ) {
        let shared = shared_len(&cached, &next);
        match plan(&cached, &next, partial, &checkpoints) {
            Reuse::Fresh => {}
            Reuse::Truncate { keep } => {
                prop_assert!(keep > 0 && keep <= shared && keep < next.len());
                prop_assert!(!partial || keep == cached.len(), "partial memory cut at {} of {}", keep, cached.len());
            }
            Reuse::Restore { keep } => {
                prop_assert!(partial && keep > 0 && keep <= shared && keep < next.len());
                prop_assert!(checkpoints.contains(&keep));
            }
        }

        // `pinned` is one of the positions (this call's turn start), or none.
        let pinned = pin.and_then(|i| positions.get(i).copied());
        let kept = retain(positions.clone(), pinned);
        prop_assert!(kept.windows(2).all(|w| w[0] < w[1]) && kept.len() <= MAX_CHECKPOINTS);
        prop_assert!(kept.iter().all(|p| positions.contains(p)));
        if let (Some(&min), Some(&max)) = (positions.iter().min(), positions.iter().max()) {
            prop_assert!(kept.contains(&min) && kept.contains(&max), "{:?} → {:?}", positions, kept);
        }
        if let Some(p) = pinned {
            prop_assert!(kept.contains(&p));
        }
    }
}
