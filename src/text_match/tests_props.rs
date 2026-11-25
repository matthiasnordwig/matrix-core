//! Property tests (proptest) for `text_match`: rules that must hold for every
//! input, not just the examples in `tests.rs`. Case count per test comes from
//! `PROPTEST_CASES` (stage 2 default here: 32; stage 3 / release: 2000), see
//! `handbuch/arbeitsablauf.md` "Teststufen". A counterexample proptest finds is
//! stored in `core/proptest-regressions/` (checked in) and additionally gets a
//! named example test in `tests.rs`/below that cites the rule it broke.
//!
//! Rules:
//! (a) A piece cut out of any text is found by `contains_evidence` — in the
//!     text itself and in a "PDF rendering" of it with extra line-wrap hyphens
//!     (`-` + spaces/tabs + `\n` + spaces/tabs) and extra whitespace. Derived
//!     preconditions (from `normalize`'s doc comment, not guesswork): a cut may
//!     not split a dehyphenation sequence (a quote never ends inside a line
//!     wrap); insertions may not create or break one (a hyphen followed by a
//!     newline *is* a wrap by definition); the piece must hold at least one
//!     non-blank ellipsis fragment (blank evidence is `false` by contract).
//! (b) `normalize` is idempotent on its output; the index map has one entry
//!     per output byte, each a char boundary of the input pointing at the char
//!     the output char came from, and it never decreases.
//! (c) No input makes `normalize`, `fold`, `split_ellipsis_fragments`,
//!     `normalized_contains` or `contains_evidence` panic.

use super::*;
use proptest::prelude::*;

/// Picks a position in `0..len` from a raw random number — the same
/// fixed-point formula as `proptest::sample::Index`, but with a plain `usize`
/// so a shrunk counterexample can be replayed verbatim in an example test.
fn pick(raw: usize, len: usize) -> usize {
    ((len as u128 * raw as u128) >> usize::BITS) as usize
}

/// `PROPTEST_CASES` cases (`crate::props`), many rejects allowed (rule (a) filters cuts).
fn config() -> ProptestConfig {
    let mut c = crate::props::config();
    c.max_global_rejects = 100_000;
    c
}

/// Characters that matter to the normalization rules, plus multi-byte ones.
fn tricky_char() -> impl Strategy<Value = char> {
    prop_oneof![
        4 => prop::sample::select(vec!['a', 'b', 'Z', 'ä', '€', '𝄞', '1']),
        2 => Just(' '),
        1 => Just('\t'),
        2 => Just('\n'),
        1 => Just('\r'),
        2 => Just('-'),
        2 => Just('.'),
        1 => Just('…'),
        1 => prop::sample::select(vec!['\u{a0}', '\u{2028}', '\u{85}']),
    ]
}

fn tricky_text(max: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(tricky_char(), 0..max).prop_map(|v| v.into_iter().collect())
}

/// Byte spans `[a, b)` of the dehyphenation sequences `normalize` drops:
/// `-` + `[ \t]*` + `\n` + `[ \t]*`, scanned left to right like `normalize`.
fn dehyph_spans(t: &str) -> Vec<(usize, usize)> {
    let b = t.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'-' {
            let mut j = i + 1;
            while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
                j += 1;
            }
            if j < b.len() && b[j] == b'\n' {
                j += 1;
                while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
                    j += 1;
                }
                spans.push((i, j));
                i = j;
                continue;
            }
        }
        i += 1;
    }
    spans
}

fn boundaries(t: &str) -> Vec<usize> {
    t.char_indices().map(|(i, _)| i).chain(std::iter::once(t.len())).collect()
}

/// Cuts `t[s..e]`, moving each end out of any dehyphenation sequence: the
/// start may sit at a sequence's first byte or right after it, the end only
/// before it (ending right after one would let the fragment trim eat the
/// newline, i.e. the quote ends inside the wrap).
fn cut(t: &str, i: usize, j: usize) -> &str {
    let bs = boundaries(t);
    let (mut s, mut e) = (bs[pick(i, bs.len())], bs[pick(j, bs.len())]);
    if s > e {
        std::mem::swap(&mut s, &mut e);
    }
    for (a, b) in dehyph_spans(t) {
        if a < s && s < b {
            s = b;
        }
        if a < e && e <= b {
            e = a;
        }
    }
    if s > e {
        e = s;
    }
    &t[s..e]
}

/// One insertion into the original text: a line-wrap hyphen or extra
/// whitespace, at a char boundary.
#[derive(Debug, Clone)]
enum Insert {
    Wrap { pre: String, post: String },
    Space(String),
}

fn insert() -> impl Strategy<Value = (usize, Insert)> {
    let ws_tab = prop::collection::vec(prop::sample::select(vec![' ', '\t']), 0..3)
        .prop_map(|v| v.into_iter().collect::<String>());
    let wrap = (ws_tab.clone(), ws_tab).prop_map(|(pre, post)| Insert::Wrap { pre, post });
    let space = prop::collection::vec(prop::sample::select(vec![' ', '\t', '\n']), 1..3)
        .prop_map(|v| Insert::Space(v.into_iter().collect()));
    (any::<usize>(), prop_oneof![wrap, space])
}

/// Whether inserting `ins` at byte `p` of `t` leaves `normalize(t)` unchanged
/// by the documented rules (see the module comment for why each case counts).
fn neutral(t: &str, p: usize, ins: &Insert) -> bool {
    let spans = dehyph_spans(t);
    let prev = t[..p].chars().next_back();
    let next = t[p..].chars().next();
    match ins {
        // Inside a wrap it would break that wrap; before a space/tab the
        // wrap's trailing `[ \t]*` would swallow it and join two words.
        Insert::Wrap { .. } => {
            !spans.iter().any(|&(a, b)| a < p && p < b) && !matches!(next, Some(' ' | '\t'))
        }
        Insert::Space(ws) => {
            // Only extends an existing whitespace run.
            let touches_ws = prev.is_some_and(char::is_whitespace) || next.is_some_and(char::is_whitespace);
            // Not inside or right after a wrap (a newline there is not eaten).
            let in_wrap = spans.iter().any(|&(a, b)| a < p && p <= b);
            // A newline after `-[ \t]*` would create a new wrap.
            let makes_wrap = ws.contains('\n') && t[..p].trim_end_matches([' ', '\t']).ends_with('-');
            touches_ws && !in_wrap && !makes_wrap
        }
    }
}

/// The "PDF rendering": applies every neutral insertion (one per position).
fn render(t: &str, inserts: &[(usize, Insert)]) -> String {
    let bs = boundaries(t);
    let mut at: Vec<(usize, &Insert)> = Vec::new();
    for (i, ins) in inserts {
        let p = bs[pick(*i, bs.len())];
        if !at.iter().any(|&(q, _)| q == p) && neutral(t, p, ins) {
            at.push((p, ins));
        }
    }
    at.sort_by_key(|&(p, _)| std::cmp::Reverse(p));
    let mut h = t.to_string();
    for (p, ins) in at {
        let s = match ins {
            Insert::Wrap { pre, post } => format!("-{pre}\n{post}"),
            Insert::Space(ws) => ws.clone(),
        };
        h.insert_str(p, &s);
    }
    h
}

/// Rule (a), shared by the stage-2 and stage-3 variants.
fn check_rule_a(t: &str, i: usize, j: usize, inserts: &[(usize, Insert)]) -> Result<(), TestCaseError> {
    let piece = cut(t, i, j);
    prop_assume!(!split_ellipsis_fragments(piece).is_empty());
    prop_assert!(contains_evidence(t, piece), "piece {piece:?} not found in its own text {t:?}");
    let h = render(t, inserts);
    prop_assert_eq!(normalize(&h).0, normalize(t).0, "rendering changed the normalized text: {:?}", h);
    prop_assert!(contains_evidence(&h, piece), "piece {piece:?} not found in rendering {h:?} of {t:?}");
    Ok(())
}

proptest! {
    #![proptest_config(config())]

    /// Rule (a): a cut piece is found in its text and in a PDF rendering of it.
    #[test]
    fn rule_a_cut_piece_is_found(
        t in tricky_text(60),
        i in any::<usize>(),
        j in any::<usize>(),
        inserts in prop::collection::vec(insert(), 0..8),
    ) {
        check_rule_a(&t, i, j, &inserts)?;
    }

    /// Rule (b): idempotent, index map complete, valid and never decreasing.
    #[test]
    fn rule_b_normalize_idempotent_and_map_valid(t in tricky_text(60)) {
        let (n, map) = normalize(&t);
        prop_assert_eq!(&normalize(&n).0, &n, "not idempotent for {:?}", t);
        prop_assert_eq!(map.len(), n.len());
        prop_assert!(map.windows(2).all(|w| w[0] <= w[1]), "map decreases: {:?}", map);
        for (k, c) in n.char_indices() {
            let src = map[k];
            prop_assert!(src < t.len() && t.is_char_boundary(src), "bad index {} in map for {:?}", src, t);
            let orig = t[src..].chars().next().unwrap();
            if c == ' ' {
                prop_assert!(orig.is_whitespace(), "space maps to {:?} in {:?}", orig, t);
            } else {
                prop_assert_eq!(orig, c);
            }
        }
    }

    /// Rule (c): nothing panics, for tricky and for arbitrary Unicode text.
    #[test]
    fn rule_c_no_panic(
        a in prop_oneof![tricky_text(60), any::<String>()],
        b in prop_oneof![tricky_text(30), any::<String>()],
    ) {
        let _ = normalize(&a);
        let _ = fold(&a);
        let _ = split_ellipsis_fragments(&b);
        let _ = normalized_contains(&a, &b);
        let _ = contains_evidence(&a, &b);
    }
}

proptest! {
    #![proptest_config(config())]

    /// Stage 3: rule (a) on long texts (whole chunk size) with many wraps.
    #[test]
    #[ignore = "Stufe 3"]
    fn stufe3_rule_a_long_texts(
        t in tricky_text(1500),
        i in any::<usize>(),
        j in any::<usize>(),
        inserts in prop::collection::vec(insert(), 0..100),
    ) {
        check_rule_a(&t, i, j, &inserts)?;
    }
}

// Counterexamples proptest found, replayed verbatim (seeds in
// `core/proptest-regressions/text_match/tests_props.txt`). Fixed 2026-11-25.

/// Rule (b): a wrap between two whitespace runs left two spaces, and
/// normalizing again collapsed them to one.
#[test]
fn regression_rule_b_wrap_between_whitespace_runs() {
    let (n, map) = normalize(" -\n\n");
    assert_eq!(n, " ");
    assert_eq!(map, vec![0]);
    assert_eq!(normalize(&n).0, n);
}

/// Rule (a): the same double space made a rendering with an extra wrap after
/// whitespace normalize differently from the original text.
#[test]
fn regression_rule_a_rendering_wrap_after_whitespace() {
    let t = "aaaaaaaaa\n..aaä\n-\u{2028}  ä…-\r\u{a0}\t\n…\u{2028}\n\nZ";
    let inserts = [
        (16313889202339990204, Insert::Wrap { pre: "\t\t".into(), post: "".into() }),
        (10966217829813856579, Insert::Space("\t".into())),
        (14462103361231129290, Insert::Space(" ".into())),
        (16530780736489645020, Insert::Wrap { pre: "\t".into(), post: "".into() }),
        (15996946016681379975, Insert::Space(" \n".into())),
        (17887295089758151683, Insert::Wrap { pre: "".into(), post: "  ".into() }),
    ];
    check_rule_a(t, 9049423071177060008, 6058720764189875769, &inserts).unwrap();
}

/// Rule (a): a quote ending in a line wrap plus a blank line was trimmed to
/// "ä-", which the dehyphenated text does not contain.
#[test]
fn regression_rule_a_piece_ending_in_wrap() {
    let t = "a\t𝄞\u{2028}.. €Z 1\r……… -.Z𝄞b€\n-€a.\r ………ä𝄞\n\t …ä-\t\n \n\n\r.\r  \u{a0}\r1.\u{2028}";
    assert!(contains_evidence(t, "ä𝄞\n\t …ä-\t\n \n"));
}

/// Rule (a), same fix: a wrap right before an ellipsis, and a fragment that is
/// nothing but a wrap (blank, skipped).
#[test]
fn wrap_before_ellipsis_and_bare_wrap_fragment() {
    assert!(contains_evidence("Datenschutzbeauftragte", "Datenschutz-\n… beauftragte"));
    assert_eq!(split_ellipsis_fragments("x...-\n...y"), vec!["x", "y"]);
    assert!(contains_evidence("x...-\n...y", "x...-\n...y"));
}

/// Rule (a), stage 3: a fragment made only of two wraps ("-\n-\n") between
/// ellipses normalized to nothing and failed the whole quote; it is blank now.
#[test]
fn regression_rule_a_fragment_of_only_wraps() {
    let t = format!("{}… -\n-\n…{}", "a".repeat(455), "a".repeat(73));
    check_rule_a(&t, 15899992093383979192, 0, &[]).unwrap();
}
