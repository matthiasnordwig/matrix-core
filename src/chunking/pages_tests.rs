//! Tests for `pages.rs` (page changes inside a chunk) and the pages the
//! structural chunker records for a paragraph over a page break.

use super::pages::ChunkPages;
use super::structural::chunk_pdf_structurally;

/// A chunk of two lines on page 43 and one on page 44, as the chunker builds it.
fn over_break() -> (String, ChunkPages) {
    let mut text = String::new();
    let mut pages = ChunkPages::default();
    for (line, page) in [("(1) Erste Zeile auf Seite 43.", 43), ("b) zweite Zeile auf Seite 43;", 43), ("c) erste Zeile auf Seite 44.", 44)] {
        pages.line(text.len(), page);
        text.push_str(line);
        text.push('\n');
    }
    (text, pages)
}

fn stamped(pages: &ChunkPages) -> String {
    let mut meta = serde_json::Map::new();
    pages.stamp(&mut meta);
    serde_json::Value::Object(meta).to_string()
}

#[test]
fn a_chunk_over_a_page_break_keeps_both_pages() {
    let (text, pages) = over_break();
    let meta = stamped(&pages);
    assert_eq!(meta, r#"{"page":43,"page_breaks":[[60,44]]}"#);
    let read = ChunkPages::from_metadata(&meta).unwrap();
    assert_eq!(read, pages);
    assert_eq!((read.range(), read.label()), ((43, 44), "pp. 43\u{2013}44".to_string()));
    assert_eq!(read.mark(&text), "(1) Erste Zeile auf Seite 43.\nb) zweite Zeile auf Seite 43;\n[p. 44]\nc) erste Zeile auf Seite 44.\n");
}

#[test]
fn a_one_page_chunk_has_no_breaks_and_is_unchanged() {
    let pages = ChunkPages::from_metadata(r#"{"page":7,"file":"a.pdf"}"#).unwrap();
    assert_eq!((pages.range(), pages.label()), ((7, 7), "p. 7".to_string()));
    assert_eq!(pages.mark("Text.\n"), "Text.\n");
    assert_eq!(stamped(&pages), r#"{"page":7}"#);
    assert_eq!(ChunkPages::from_metadata(r#"{"file":"a.md"}"#), None, "no page, no pages");
}

#[test]
fn a_quote_is_located_on_its_page_or_over_the_break() {
    let (text, pages) = over_break();
    let finds = |q: &'static str| move |part: &str| part.contains(q);
    assert_eq!(pages.locate(&text, finds("Erste Zeile")), Some((43, 43)), "quote on page 1 of the chunk");
    assert_eq!(pages.locate(&text, finds("c) erste Zeile")), Some((44, 44)), "quote on page 2 of the chunk");
    assert_eq!(pages.locate(&text, finds("Seite 43;\nc) erste")), Some((43, 44)), "quote over the break");
    assert_eq!(pages.locate(&text, finds("nicht da")), None);
}

#[test]
fn offsets_that_do_not_start_a_line_are_ignored() {
    // Text changed after chunking: the recorded break no longer starts a line.
    let pages = ChunkPages::from_metadata(r#"{"page":3,"page_breaks":[[2,4],[0,9],["x",5]]}"#).unwrap();
    assert_eq!(pages.mark("abcdef"), "abcdef");
    assert_eq!(pages.locate("abcdef", |p| p.contains("cd")), Some((3, 3)));
}

#[test]
fn split_and_append_keep_the_pages_of_each_part() {
    let (text, pages) = over_break();
    let cut = text.find("b)").unwrap();
    let (head, tail) = pages.split_at(cut, 0);
    assert_eq!(head.range(), (43, 43));
    assert_eq!(tail.mark(&text[cut..]), "b) zweite Zeile auf Seite 43;\n[p. 44]\nc) erste Zeile auf Seite 44.\n");
    // Cut right before the page change, with whitespace skipped: the rest starts on page 44.
    let at = text.find("c)").unwrap() - 1;
    assert_eq!(pages.split_at(at, 1).1.range(), (44, 44));

    let mut merged = head.clone();
    merged.append(&tail, cut);
    assert_eq!(merged, pages);
}

#[test]
fn the_structural_chunker_records_the_page_break_inside_a_paragraph() {
    // One paragraph of short lines (no blank line, no heading, under max_chunk_chars) flows onto page 2.
    let lines: Vec<String> = (1..=70).map(|i| format!("Zeile {i} weiter")).collect();
    let mut pdf = pdf_oxide::api::Pdf::from_markdown(&lines.join("\n")).expect("markdown -> pdf");
    let bytes = pdf.to_bytes().expect("pdf -> bytes");
    let chunks = chunk_pdf_structurally(&bytes, 1, 1, None).unwrap();
    let spanning: Vec<_> = chunks
        .iter()
        .filter_map(|c| ChunkPages::from_metadata(&c.metadata).map(|p| (c, p)))
        .filter(|(_, p)| p.range().0 != p.range().1)
        .collect();
    assert!(!spanning.is_empty(), "a chunk should run over the page break: {:?}", chunks.iter().map(|c| &c.metadata).collect::<Vec<_>>());
    let (chunk, pages) = &spanning[0];
    assert_eq!(pages.range(), (1, 2));
    let marked = pages.mark(&chunk.text);
    let after = marked.split("[p. 2]\n").nth(1).expect("marker in the text");
    let first_on_2 = after.lines().next().unwrap();
    let last_on_1 = marked.split("\n[p. 2]").next().unwrap().lines().last().unwrap();
    let n = |l: &str| l.split_whitespace().nth(1).unwrap().parse::<usize>().unwrap();
    assert_eq!(n(first_on_2), n(last_on_1) + 1, "the marker sits between two consecutive lines");
}
