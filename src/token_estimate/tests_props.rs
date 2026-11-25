//! Property tests (proptest) for the token estimate. Case count from `crate::props`
//! (`PROPTEST_CASES`, stage 2: 32), see `handbuch/arbeitsablauf.md` "Teststufen".
//! Counterexamples: `core/proptest-regressions/token_estimate/` (checked in) plus a named test.
//!
//! Rules:
//! (26) `estimate_tokens` never panics and is ≥ 1 ("Estimated tokens of `text` (≥ 1)").
//! (27) A beginning of a text never costs more than the whole text (user decision 2026-11-25:
//!      the budget a text reserves must not shrink when text is appended).

use proptest::prelude::*;

use super::estimate_tokens;

/// Text that mixes the scripts and the English markers that switch the Latin word price.
fn mixed_text() -> impl Strategy<Value = String> {
    let piece = prop::sample::select(vec![
        "the", "and", "with", "there", "th", "Donaudampfschifffahrt", "Größenordnung", "Wort", "x", " ", " ", "  ", "\n",
        "12", "3", ".", ",", "—", "名前", "имя", "اسم", "नाम", "한국", "€", "🙂",
    ]);
    prop_oneof![
        4 => prop::collection::vec(piece, 0..30).prop_map(|v| v.concat()),
        1 => any::<String>(),
    ]
}

/// Rule (27) at every char boundary of `t`.
fn check_monotone(t: &str) -> Result<(), TestCaseError> {
    let full = estimate_tokens(t);
    for (i, _) in t.char_indices() {
        let head = estimate_tokens(&t[..i]);
        prop_assert!(head <= full, "beginning {:?} costs {} > {} of {:?}", &t[..i], head, full, t);
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::props::config())]

    /// Rules (26) and (27).
    #[test]
    fn rule_26_27_at_least_one_and_monotone(t in mixed_text()) {
        prop_assert!(estimate_tokens(&t) >= 1);
        check_monotone(&t)?;
    }
}

// Counterexamples proptest found, replayed verbatim (seeds in
// `core/proptest-regressions/token_estimate/tests_props.txt`).

/// Rule (27): appending "the" made the text English, which prices its long German word lower,
/// so the whole cost 6 tokens and its beginning "Donaudampfschifff" 7.
#[test]
fn regression_rule_27_english_marker_appended() {
    check_monotone("Donaudampfschifffahrt the").unwrap();
}
