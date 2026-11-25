//! Property tests (proptest) for the norm-reference parser: rules that must
//! hold for every text, not just the table cases in `tests.rs`. Case count per
//! test from `PROPTEST_CASES` (stage 2 default here: 32; stage 3: 2000), see
//! `handbuch/arbeitsablauf.md` "Teststufen". Counterexamples are stored in
//! `core/proptest-regressions/refs/` (checked in) and get a named example test.
//!
//! Rules (from the module and doc comments in `mod.rs`):
//! (11) `parse_refs`: no panic (also numbers far beyond `u32`); every `ref_key`
//!      once ("de-duplicated by `(ref_key)`"), ordered by first appearance
//!      (`start` never falls), spans valid char-boundary ranges of the text.
//! (12) `expand_range`: at most `MAX_RANGE_EXPANSION` parts; above it exactly
//!      start and end ("only the start and end ref are emitted"); the start
//!      number always first; every part's number inside the range.
//! (13) `equivalent_ref_keys`: the input first, never duplicates, and each key
//!      of the result yields the same set ("both directions").

use std::collections::HashSet;

use proptest::prelude::*;

use super::{equivalent_ref_keys, expand_range, parse_refs, MAX_RANGE_EXPANSION};

/// Cases per property: `PROPTEST_CASES` if set, else 32 (stage 2 stays fast).
fn config() -> ProptestConfig {
    let mut c = ProptestConfig::default();
    if std::env::var("PROPTEST_CASES").is_err() {
        c.cases = 32;
    }
    c
}

/// Pieces of real citations plus noise, so that ranges, enumerations, long
/// forms and Kürzel bindings all occur.
const PIECES: &[&str] = &[
    "§", "§§", " ", " ", "25", "13", "13c", "a", "7", "521", "99999999999999999999", "4294967296", "bis", "-", "–",
    ",", " und ", "Abs.", "Absatz 1", "Satz 2", "Nr. 3", "KWG", "GwG", "CRR", "DORA", "Art.", "Artikel", "Article",
    " der Verordnung (EU) Nr. 575/2013", " des Kreditwesengesetzes", "Verordnung (EU) 2022/2554", "AT 4.3.2", "BTO 1.1",
    "dieses Artikels", "ä", "€", "\n", ".",
];

fn ref_text(max: usize) -> impl Strategy<Value = String> {
    prop_oneof![
        4 => prop::collection::vec(prop::sample::select(PIECES), 0..max).prop_map(|v| v.concat()),
        1 => any::<String>(),
    ]
}

fn number() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => (0u32..60).prop_map(|n| n.to_string()),
        1 => any::<u32>().prop_map(|n| n.to_string()),
        1 => "[0-9]{1,25}",
    ]
}

fn letter() -> impl Strategy<Value = String> {
    prop_oneof![Just(String::new()), "[a-z]", Just("Z".to_string())]
}

proptest! {
    #![proptest_config(config())]

    /// Rule (11).
    #[test]
    fn rule_11_parse_refs_unique_ordered_valid(t in ref_text(40)) {
        let refs = parse_refs(&t);
        let mut seen = HashSet::new();
        let mut prev = 0;
        for r in &refs {
            prop_assert!(seen.insert(r.ref_key.clone()), "{} twice in {:?}", r.ref_key, t);
            prop_assert!(r.start >= prev, "not in order of appearance in {:?}", t);
            prop_assert!(r.start < r.end && r.end <= t.len(), "bad span {}..{} in {:?}", r.start, r.end, t);
            prop_assert!(t.is_char_boundary(r.start) && t.is_char_boundary(r.end));
            prev = r.start;
        }
    }

    /// Rule (12).
    #[test]
    fn rule_12_range_expansion_is_capped(sn in number(), sl in letter(), en in number(), el in letter()) {
        let parts = expand_range(&sn, &sl, &en, &el);
        prop_assert!(!parts.is_empty() && parts.len() <= MAX_RANGE_EXPANSION, "{} parts", parts.len());
        let (Ok(s), Ok(e)) = (sn.parse::<u32>(), en.parse::<u32>()) else {
            prop_assert_eq!(parts, vec![(sn.clone(), sl.clone())]);
            return Ok(());
        };
        prop_assert_eq!(parts[0].0.parse::<u32>().ok(), Some(s), "start missing: {:?}", parts);
        for (n, _) in &parts {
            let n: u32 = n.parse().unwrap();
            prop_assert!(n == s || (s <= n && n <= e), "{} outside {}..={}", n, s, e);
        }
        if e > s && (e - s) as usize >= MAX_RANGE_EXPANSION {
            prop_assert_eq!(parts, vec![(s.to_string(), String::new()), (e.to_string(), String::new())]);
        }
    }

    /// Rule (13).
    #[test]
    fn rule_13_equivalent_keys_symmetric(
        prefix in prop_oneof![
            prop::sample::select(vec!["CRR", "DORA", "EU:2013/575", "EU:2022/2554", "KWG", "", "crr"]).prop_map(String::from),
            ".{0,8}",
        ],
        art in ".{0,8}",
        art_shaped in any::<bool>(),
    ) {
        let key = if art_shaped { format!("{prefix}:Art.{art}") } else { format!("{prefix}{art}") };
        let eq = equivalent_ref_keys(&key);
        prop_assert_eq!(&eq[0], &key);
        let set: HashSet<&String> = eq.iter().collect();
        prop_assert_eq!(set.len(), eq.len(), "duplicates in {:?}", eq);
        for k in &eq {
            let back: HashSet<String> = equivalent_ref_keys(k).into_iter().collect();
            let want: HashSet<String> = eq.iter().cloned().collect();
            prop_assert_eq!(back, want, "alias of {:?} does not lead back", k);
        }
    }
}

proptest! {
    #![proptest_config(config())]

    /// Stage 3: rule (11) on chunk-sized texts.
    #[test]
    #[ignore = "Stufe 3"]
    fn stufe3_rule_11_long_texts(t in ref_text(1500)) {
        let refs = parse_refs(&t);
        let keys: HashSet<&String> = refs.iter().map(|r| &r.ref_key).collect();
        prop_assert_eq!(keys.len(), refs.len());
        prop_assert!(refs.windows(2).all(|w| w[0].start <= w[1].start));
    }
}
