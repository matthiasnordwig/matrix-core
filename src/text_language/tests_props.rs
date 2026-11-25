//! Property tests (proptest) for the language detection. Case count from `crate::props`
//! (`PROPTEST_CASES`, stage 2: 32), see `handbuch/arbeitsablauf.md` "Teststufen".
//!
//! Rule (28): `passage_language`, `query_language` and `document_language` never panic — the
//! document samples cut the text at `len / 2` and `len * 3 / 4`, moved back to a char boundary —
//! and return `None` or one of the seven codes.

use proptest::prelude::*;

use super::{document_language, passage_language, query_language};

const CODES: [&str; 7] = ["de", "en", "fr", "es", "it", "nl", "pt"];

fn words() -> impl Strategy<Value = String> {
    let piece = prop::sample::select(vec![
        "der ", "the ", "le ", "el ", "il ", "het ", "não ", "è ", "für ", "ä", "€", "日本", "🙂", " ", "\n", "a@b.de ", "https://x.de/a ",
    ]);
    prop_oneof![
        4 => prop::collection::vec(piece, 0..40).prop_map(|v| v.concat()),
        1 => any::<String>(),
    ]
}

proptest! {
    #![proptest_config(crate::props::config())]

    /// Rule (28), on short texts and on long ones (repeated past the 16 KB samples).
    #[test]
    fn rule_28_language_never_panics(t in words(), repeat in prop_oneof![Just(1usize), 300usize..1500]) {
        let t = t.repeat(repeat);
        for lang in [passage_language(&t), query_language(&t), document_language(&t)] {
            prop_assert!(lang.is_none_or(|l| CODES.contains(&l)), "{:?}", lang);
        }
    }
}
