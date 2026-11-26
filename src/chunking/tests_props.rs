//! Property tests (proptest) for the chunkers: rules that must hold for every
//! text, not just the examples in the `*_tests.rs` files. Case count per test
//! comes from `PROPTEST_CASES` (stage 2 default here: 32; stage 3: 2000), see
//! `handbuch/arbeitsablauf.md` "Teststufen". Counterexamples are stored in
//! `core/proptest-regressions/chunking/` (checked in) and get a named example
//! test at the end of this file.
//!
//! Rules (each from the module or doc comment named in brackets):
//! (1) `split_sentences`: no panic; `text[byte_start..byte_end] == text`,
//!     ascending, non-overlapping, `index` 0..n ("Offsets are byte offsets into
//!     the original string so callers can slice it directly").
//! (2) `split_segments`: `raw[byte_start..byte_end] == text`, starts strictly
//!     rising, no `\n` inside a segment ("broken at every newline AND every
//!     sentence end", "Byte offsets are absolute").
//! (3) `build_windows`: windows cover all segments without a gap, window starts
//!     strictly rise (the loop ends), each window stays within `window_tokens`
//!     unless it is a single segment; any `overlap_ratio` (also NaN) is safe.
//!     The next window starts as far back as the overlap needs and no further:
//!     its segments up to the previous window's end hold at least
//!     `window_tokens × overlap_ratio` (clamped to 0..0.95) tokens unless it is
//!     only one segment past the previous start, and one segment fewer would hold
//!     less. `prepare_windows` is `build_windows` over `split_segments`.
//! (4) `assemble`: any starts/leave-outs (also out of range): no panic, spans
//!     ascending, `chunk_index` sequential; without leave-outs the chunks
//!     partition the segments from segment 0 on ("gapless by construction").
//! (5) `chunk_markdown_structurally`, `chunk_by_rules` (every kind it handles),
//!     `chunk_mail`: chunks are verbatim, ascending, non-overlapping, non-empty
//!     slices (markdown.rs: "`text[char_start..char_end] == chunk.text` holds
//!     exactly"; rules.rs and mail.rs: "as in `markdown.rs`"), and together they
//!     hold every non-whitespace character of the text (nothing is lost: a
//!     preamble before the first mark, a short last unit). A kind without a
//!     table yields nothing from `chunk_by_rules`.
//! (6) `split_long_text`: ranges inside `[start, end)`, on char boundaries,
//!     ascending, each at most `max_chars` chars ("into ranges of at most
//!     `max_chars`").
//! (7) `ChunkPages`: `from_metadata(stamp(p)) == p`; `mark`/`locate` never panic
//!     for any recorded offsets ("A recorded offset that does not start a line
//!     of `text` is ignored").
//! (8) `repair_mojibake`: no panic; pure ASCII stays unchanged (every repair
//!     target is non-ASCII).
//! (9) `parse_scan_marker(scan_marker(a, b)) == Some((a, b))` for pages ≥ 0;
//!     `chunk_view` keeps the byte length and only turns lone `\n` into spaces.
//! (10) `step_runs`: runs ascending and disjoint, `first < last`, the kept page
//!     inside the run, and every other page of the run covered by the kept one.
//! (11) `ChunkPages::split_at(at, skip)`: every byte keeps its page — the head
//!     for bytes before `at` (and ends on the page of byte `at - 1`), the tail
//!     for bytes from `at + skip` on.
//! (12) `split_sentences` on sentences of a known form (words with numbers,
//!     abbreviations and versions inside, a run of terminators, closing marks)
//!     returns exactly those sentences ("Absorb consecutive terminators and
//!     closing punctuation").
//! (13) `repair_mojibake`: every character of the target ranges (U+0080–U+017F,
//!     U+2000–U+2FFF), written as UTF-8 and read as Windows-1252, comes back
//!     (soft hyphen dropped, typographic spaces as a space).

use proptest::prelude::*;
use serde_json::{Map, Value};

use super::kind::DocKind;
use super::markdown::{chunk_markdown_structurally, split_long_text};
use super::mail::chunk_mail;
use super::pages::ChunkPages;
use super::rules::{chunk_by_rules, handles};
use super::scan_text::{chunk_view, parse_scan_marker, scan_marker};
use super::slide_steps::{covers, step_runs};
use super::text_repair::repair_mojibake;
use super::{assemble, build_windows, prepare_windows, split_segments, split_sentences, LlmChunkResponse, StartItem};
use crate::db::models::NewChunk;

/// Pieces that drive the chunkers' rules: sentence ends, abbreviations,
/// numbers, line and paragraph breaks, headings, fences, list items and the
/// line marks of every kind `rules.rs` handles, plus multi-byte text.
const PIECES: &[&str] = &[
    "Wort", "satz", "ä", "€", "𝄞", "日本", "x", "Dr.", "z.B.", "3.14", "A.", " ", " ", " ", "  ", "\t",
    ".", ". ", "!", "? ", "…", "\"", ")", "»", "\n", "\n", "\n\n", "\r\n", " \n", "# ", "## ", "### Titel\n",
    "```\n", "~~~\n", "- ", "* ", "1. ", "2) ", "> ", "§ 3 ", "Art. 5 ", "(1) ", "Teil 2\n", "TOP 1 ",
    "Anna Muster: ", "Kapitel 3\n", "[01.02.2024, 10:00] ", "[01.02.2024, 11:15] ", "fn x() {}\n", "def f():\n",
    "**From:** a@b.de  \n", "**Date:** 2000-09-06 04:46\n", "**To:** c@d.de\n", "-----Original Message-----\n",
    "Von: X\n", "Gesendet: 1.2.2020 10:00\n", "An: Y\n", "Betreff: Z\n", "On Mon, Bob wrote:\n",
];

fn doc_text(max_pieces: usize) -> impl Strategy<Value = String> {
    prop_oneof![
        4 => prop::collection::vec(prop::sample::select(PIECES), 0..max_pieces).prop_map(|v| v.concat()),
        1 => any::<String>(),
    ]
}

fn mail_text(max_pieces: usize) -> impl Strategy<Value = String> {
    doc_text(max_pieces).prop_map(|body| format!("# Betreff\n\n**From:** a@example.com  \n**Date:** 2000-09-06\n\n{body}"))
}

/// Rule (5) for any chunker's output.
fn check_verbatim(text: &str, chunks: &[NewChunk]) -> Result<(), TestCaseError> {
    let mut prev_end = 0usize;
    for (i, c) in chunks.iter().enumerate() {
        let (s, e) = (c.char_start as usize, c.char_end as usize);
        prop_assert_eq!(c.chunk_index, i as i64);
        prop_assert!(s < e, "empty or reversed chunk {}..{} in {:?}", s, e, text);
        prop_assert!(e <= text.len() && text.is_char_boundary(s) && text.is_char_boundary(e), "bad span {}..{} in {:?}", s, e, text);
        prop_assert_eq!(&text[s..e], c.text.as_str(), "chunk {} not verbatim in {:?}", i, text);
        prop_assert!(s >= prev_end, "chunk {} overlaps the one before in {:?}", i, text);
        prev_end = e;
    }
    Ok(())
}

/// Kinds `chunk_by_rules` has a table for.
const RULE_KINDS: [DocKind; 6] = [DocKind::Legal, DocKind::Minutes, DocKind::Transcript, DocKind::Book, DocKind::Chat, DocKind::Code];

/// Rule (5), second half: the chunks hold every non-whitespace character of the text.
fn check_complete(text: &str, chunks: &[NewChunk]) -> Result<(), TestCaseError> {
    let solid = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let kept: String = chunks.iter().map(|c| solid(&c.text)).collect();
    prop_assert_eq!(kept, solid(text), "text lost in {:?}", text);
    Ok(())
}

/// Rule (6), shared by the stage-2 and stage-3 variants.
fn check_split_long_text(text: &str, a: usize, b: usize, max_chars: usize) -> Result<(), TestCaseError> {
    let bs: Vec<usize> = text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).collect();
    let (mut s, mut e) = (bs[a % bs.len()], bs[b % bs.len()]);
    if s > e {
        std::mem::swap(&mut s, &mut e);
    }
    let ranges = split_long_text(text, s, e, max_chars);
    let mut prev = s;
    for &(rs, re) in &ranges {
        prop_assert!(prev <= rs && rs <= re && re <= e, "range {}..{} outside {}..{} or not ascending in {:?}", rs, re, s, e, text);
        prop_assert!(text.is_char_boundary(rs) && text.is_char_boundary(re));
        let n = text[rs..re].chars().count();
        prop_assert!(n <= max_chars, "range {}..{} has {} > {} chars in {:?}", rs, re, n, max_chars, text);
        prev = re;
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::props::config())]

    /// Rules (1) and (2).
    #[test]
    fn rule_1_2_sentences_and_segments_are_verbatim(t in doc_text(60)) {
        let sentences = split_sentences(&t);
        let mut prev = 0;
        for (i, s) in sentences.iter().enumerate() {
            prop_assert_eq!(s.index, i);
            prop_assert!(prev <= s.byte_start && s.byte_start < s.byte_end, "sentence {} at {}..{} in {:?}", i, s.byte_start, s.byte_end, t);
            prop_assert_eq!(&t[s.byte_start..s.byte_end], s.text.as_str());
            prev = s.byte_end;
        }
        let segments = split_segments(&t);
        let mut prev: Option<usize> = None;
        for (i, s) in segments.iter().enumerate() {
            prop_assert_eq!(s.index, i);
            prop_assert!(prev.is_none_or(|p| p < s.byte_start), "segment starts do not rise in {:?}", t);
            prop_assert_eq!(&t[s.byte_start..s.byte_end], s.text.as_str());
            prop_assert!(!s.text.contains('\n'), "segment {} holds a newline in {:?}", i, t);
            prev = Some(s.byte_start);
        }
    }

    /// Rule (3).
    #[test]
    fn rule_3_windows_cover_all_segments(
        t in doc_text(80),
        // A budget of 0 (no windows) in every fifth case, not only by chance once in 40.
        window_tokens in prop_oneof![1 => Just(0usize), 4 => 1usize..40],
        overlap in prop_oneof![-1.0f64..2.0, Just(f64::NAN), Just(f64::INFINITY)],
    ) {
        let segments = split_segments(&t);
        let windows = build_windows(&segments, window_tokens, overlap);
        let shape = |ws: &[super::Window]| ws.iter().map(|w| (w.start_segment, w.end_segment, w.text.clone())).collect::<Vec<_>>();
        prop_assert_eq!(shape(&prepare_windows(&t, window_tokens, overlap)), shape(&windows));
        if segments.is_empty() || window_tokens == 0 {
            prop_assert!(windows.is_empty());
            return Ok(());
        }
        prop_assert_eq!(windows[0].start_segment, 0);
        prop_assert_eq!(windows.last().unwrap().end_segment, segments.len() - 1);
        for (k, w) in windows.iter().enumerate() {
            prop_assert_eq!(w.idx, k);
            prop_assert!(w.start_segment <= w.end_segment);
            prop_assert_eq!(w.byte_start, segments[w.start_segment].byte_start);
            prop_assert_eq!(w.byte_end, segments[w.end_segment].byte_end);
            let tokens: usize = segments[w.start_segment..=w.end_segment]
                .iter()
                .map(|s| crate::token_estimate::estimate_tokens(&s.text))
                .sum();
            prop_assert!(w.start_segment == w.end_segment || tokens <= window_tokens, "window {} has {} > {} tokens", k, tokens, window_tokens);
            if k > 0 {
                let before = &windows[k - 1];
                prop_assert!(w.start_segment > before.start_segment, "window starts do not rise");
                prop_assert!(w.start_segment <= before.end_segment + 1, "gap between windows {} and {}", k - 1, k);
                let wanted = ((window_tokens as f64) * overlap.clamp(0.0, 0.95)) as usize;
                let back = |from: usize| -> usize { segments.get(from..=before.end_segment).map_or(0, |s| s.iter().map(|s| crate::token_estimate::estimate_tokens(&s.text)).sum()) };
                let s = w.start_segment;
                prop_assert!(s == before.start_segment + 1 || back(s) >= wanted, "window {} overlaps {} < {} tokens", k, back(s), wanted);
                prop_assert!(s > before.end_segment || back(s + 1) < wanted, "window {} starts further back than {} tokens need", k, wanted);
            }
        }
    }

    /// Rule (4).
    #[test]
    fn rule_4_assemble_any_starts(
        t in doc_text(60),
        starts in prop::collection::vec(prop_oneof![0usize..30, any::<usize>()], 0..10),
        leave_out in prop::collection::vec(prop_oneof![0usize..30, any::<usize>()], 0..6),
        use_leave_out in any::<bool>(),
    ) {
        let response = LlmChunkResponse {
            starts: starts.iter().map(|&i| StartItem::Index(i)).collect(),
            leave_out: if use_leave_out { leave_out } else { Vec::new() },
        };
        let chunks = assemble(&t, 1, 2, std::slice::from_ref(&response));
        let segments = split_segments(&t);
        let mut prev_end = 0i64;
        for (i, c) in chunks.iter().enumerate() {
            prop_assert_eq!(c.chunk_index, i as i64);
            prop_assert!(prev_end <= c.char_start && c.char_start <= c.char_end, "spans not ascending in {:?}", t);
            prev_end = c.char_end;
        }
        if response.leave_out.is_empty() && !segments.is_empty() {
            let seg_of_start = |b: i64| segments.iter().position(|s| s.byte_start as i64 == b);
            let seg_of_end = |b: i64| segments.iter().rposition(|s| s.byte_end as i64 == b);
            prop_assert_eq!(seg_of_start(chunks[0].char_start), Some(0));
            prop_assert_eq!(seg_of_end(chunks.last().unwrap().char_end), Some(segments.len() - 1));
            for w in chunks.windows(2) {
                let end = seg_of_end(w[0].char_end).unwrap();
                prop_assert_eq!(seg_of_start(w[1].char_start), Some(end + 1), "gap or overlap in {:?}", t);
            }
            let distinct: std::collections::BTreeSet<usize> =
                starts.iter().copied().filter(|&i| i < segments.len()).chain(std::iter::once(0)).collect();
            prop_assert_eq!(chunks.len(), distinct.len());
        }
    }

    /// Rule (5): markdown with any `max_chars` (0 = default), all rule kinds, mail.
    #[test]
    fn rule_5_chunkers_cut_verbatim_slices(
        t in doc_text(80),
        m in mail_text(60),
        max_chars in prop_oneof![0usize..1, 1usize..60, 60usize..400],
    ) {
        check_verbatim(&t, &chunk_markdown_structurally(&t, 1, 2, max_chars))?;
        check_complete(&t, &chunk_markdown_structurally(&t, 1, 2, max_chars))?;
        for kind in DocKind::ALL {
            let chunks = chunk_by_rules(&t, 1, 2, kind);
            prop_assert_eq!(handles(kind), RULE_KINDS.contains(&kind));
            if !handles(kind) {
                prop_assert!(chunks.is_empty(), "{:?} has no table but got chunks", kind);
                continue;
            }
            check_verbatim(&t, &chunks)?;
            check_complete(&t, &chunks)?;
        }
        for text in [&t, &m] {
            let chunks = chunk_mail(text, 1, 2);
            check_verbatim(text, &chunks)?;
            check_complete(text, &chunks)?;
        }
    }

    /// Rule (6).
    #[test]
    fn rule_6_split_long_text_respects_max(
        t in doc_text(80),
        a in any::<usize>(),
        b in any::<usize>(),
        max_chars in 1usize..40,
    ) {
        check_split_long_text(&t, a, b, max_chars)?;
    }

    /// Rule (7): round trip of the page metadata, and no panic in `mark`/`locate`.
    #[test]
    fn rule_7_chunk_pages_round_trip_and_never_panic(
        t in doc_text(40),
        lines in prop::collection::vec((0usize..200, 1i64..50), 1..8),
        raw_breaks in prop::collection::vec((any::<u64>(), any::<i64>()), 0..6),
        first in any::<i64>(),
        needle in "[a-zä ]{0,4}",
    ) {
        let mut sorted = lines.clone();
        sorted.sort_by_key(|&(o, _)| o);
        let mut pages = ChunkPages::default();
        pages.line(0, sorted[0].1);
        for &(o, p) in &sorted[1..] {
            pages.line(o, p);
        }
        let mut meta = Map::new();
        pages.stamp(&mut meta);
        prop_assert_eq!(ChunkPages::from_metadata(&Value::Object(meta).to_string()), Some(pages.clone()));
        let _ = pages.mark(&t);
        let _ = pages.locate(&t, |s| s.contains(needle.as_str()));

        let json = serde_json::json!({ "page": first, "page_breaks": raw_breaks.iter().map(|(o, p)| serde_json::json!([o, p])).collect::<Vec<_>>() });
        if let Some(read) = ChunkPages::from_metadata(&json.to_string()) {
            let _ = read.mark(&t);
            let _ = read.locate(&t, |s| s.contains(needle.as_str()));
            let _ = read.label();
        }
    }

    /// Rules (8) and (9).
    #[test]
    fn rule_8_9_repair_scan_marker_chunk_view(
        t in prop_oneof![doc_text(40), "[â€‰Â\u{ad}Ã¤¼½ ‰a-z\n]{0,30}"],
        ascii in "[ -~\t\n]{0,60}",
        a in 0i64..i64::MAX,
        b in 0i64..i64::MAX,
    ) {
        let _ = repair_mojibake(&t);
        prop_assert_eq!(repair_mojibake(&ascii), ascii.clone());
        prop_assert_eq!(parse_scan_marker(&scan_marker(a, b)), Some((a, b)));
        prop_assert_eq!(parse_scan_marker(&scan_marker(a, a)), Some((a, a)));
        let view = chunk_view(&t);
        prop_assert_eq!(view.len(), t.len());
        for (i, (x, y)) in t.bytes().zip(view.bytes()).enumerate() {
            if x != y {
                let lone = x == b'\n' && y == b' '
                    && (i == 0 || t.as_bytes()[i - 1] != b'\n')
                    && t.as_bytes().get(i + 1) != Some(&b'\n');
                prop_assert!(lone, "byte {} changed {:?} → {:?} in {:?}", i, x as char, y as char, t);
            }
        }
    }

    /// Rule (10): pages built from a small vocabulary so that runs form.
    #[test]
    fn rule_10_step_runs_lose_nothing(
        pages in prop::collection::vec(prop::collection::vec(prop::sample::select(vec!["Umsatz", "Ziel", "Plan", "5", "6", "Team", "neu"]), 0..8), 0..10),
    ) {
        let pages: Vec<String> = pages.into_iter().map(|w| w.join(" ")).collect();
        let runs = step_runs(&pages);
        let mut prev_last: Option<usize> = None;
        for r in &runs {
            prop_assert!(r.first < r.last && r.last < pages.len());
            prop_assert!(prev_last.is_none_or(|p| p < r.first), "runs overlap: {:?}", runs);
            prop_assert!(r.first <= r.kept && r.kept <= r.last);
            for p in r.first..=r.last {
                prop_assert!(p == r.kept || covers(&pages[r.kept], &pages[p]), "page {} not covered by {} in {:?}", p, r.kept, pages);
            }
            prev_last = Some(r.last);
        }
    }

    /// Rule (11).
    #[test]
    fn rule_11_split_at_keeps_the_page_of_every_byte(
        lines in prop::collection::vec((0usize..200, 1i64..50), 1..8),
        at in prop_oneof![(0usize..220).prop_map(Err), (0usize..8).prop_map(Ok)],
        skip in 0usize..6,
    ) {
        let mut sorted = lines.clone();
        sorted.sort_by_key(|&(o, _)| o);
        // Half the cuts fall exactly on a page change.
        let at = match at {
            Ok(k) => sorted[k % sorted.len()].0,
            Err(raw) => raw,
        };
        let mut pages = ChunkPages::default();
        pages.line(0, sorted[0].1);
        for &(o, p) in &sorted[1..] {
            pages.line(o, p);
        }
        let (head, tail) = pages.split_at(at, skip);
        for x in 0..at {
            prop_assert_eq!(head.page_at(x), pages.page_at(x), "head byte {} of {:?} cut at {}", x, pages, at);
        }
        if at > 0 {
            prop_assert_eq!(Some(head.range().1), pages.page_at(at - 1), "head of {:?} cut at {} ends on another page", pages, at);
        }
        for y in 0..250 {
            prop_assert_eq!(tail.page_at(y), pages.page_at(at + skip + y), "tail byte {} of {:?} cut at {}+{}", y, pages, at, skip);
        }
    }

    /// Rule (12).
    #[test]
    fn rule_12_sentences_of_a_known_form_split_exactly(
        sentences in prop::collection::vec((
            prop::collection::vec(prop::sample::select(vec!["Haus", "lief", "2020", "Dr. Weber", "e.g. this", "v1.5", "Ärger"]), 1..5),
            prop::sample::select(vec![".", "!", "?", "?!", "...", "…", "!!"]),
            prop::sample::select(vec!["", "\"", ")", "»", "’", "\")"]),
        ), 1..6),
    ) {
        let parts: Vec<String> = sentences.iter().map(|(words, end, close)| format!("{}{end}{close}", words.join(" "))).collect();
        let text = parts.join(" ");
        let got: Vec<String> = split_sentences(&text).into_iter().map(|s| s.text).collect();
        prop_assert_eq!(got, parts);
    }
}

proptest! {
    #![proptest_config(crate::props::config())]

    /// Stage 3: rules (5) and (6) on long texts, where `chunk_by_rules` and
    /// `chunk_mail` reach their size cuts too.
    #[test]
    #[ignore = "Stufe 3"]
    fn stufe3_rule_5_6_long_texts(
        t in doc_text(3000),
        a in any::<usize>(),
        b in any::<usize>(),
        max_chars in 1usize..400,
    ) {
        check_verbatim(&t, &chunk_markdown_structurally(&t, 1, 2, max_chars))?;
        for kind in RULE_KINDS {
            let chunks = chunk_by_rules(&t, 1, 2, kind);
            check_verbatim(&t, &chunks)?;
            check_complete(&t, &chunks)?;
        }
        check_verbatim(&t, &chunk_mail(&t, 1, 2))?;
        check_split_long_text(&t, a, b, max_chars)?;
    }
}

/// Rule (13): exhaustive over the target ranges, so a plain test.
#[test]
fn rule_13_every_target_char_read_as_cp1252_comes_back() {
    // Windows-1252 0x80–0x9F; the five undefined slots pass through as C1 controls.
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}',
        '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    let as_cp1252 = |b: u8| if b >= 0xA0 { char::from(b) } else { HIGH[usize::from(b - 0x80)] };
    for c in ('\u{80}'..='\u{17F}').chain('\u{2000}'..='\u{2FFF}') {
        let mut buf = [0u8; 4];
        let garbled: String = c.encode_utf8(&mut buf).bytes().map(as_cp1252).collect();
        let want = match c {
            '\u{AD}' => String::new(),
            '\u{2002}' | '\u{2003}' | '\u{2007}' | '\u{2009}' | '\u{200A}' | '\u{202F}' => " ".to_string(),
            _ => c.to_string(),
        };
        assert_eq!(repair_mojibake(&format!(" {garbled} ")), format!(" {want} "), "U+{:04X} read as {garbled:?}", c as u32);
    }
}

// Counterexamples proptest found, replayed verbatim (seeds in
// `core/proptest-regressions/chunking/tests_props.txt`).

/// Rules (5) and (6): a range that opens with whitespace (an indented line)
/// and is followed by a piece too long to join was cut right after the
/// whitespace; trimming that left an empty chunk `0..0`.
#[test]
fn regression_rule_5_indented_long_line_gives_no_empty_chunk() {
    let t = " WortAnna Muster: ";
    let chunks = chunk_markdown_structurally(t, 1, 2, 1);
    check_verbatim(t, &chunks).unwrap();
    assert!(split_long_text(t, 0, t.len(), 1).iter().all(|&(s, e)| s < e));
}
