//! Tests of `structural.rs` on PDFs placed run by run (`pdf_oxide::writer::DocumentBuilder`):
//! fonts, sizes and positions as a real layout has them, where the Markdown fixtures in `tests.rs`
//! only know one size and even line spacing. Each test is one rule of the module comment: offsets
//! and order of the chunks, headings from size and weight (short, no sentence end, not a layout
//! line, not a running line), list items under an article, paragraphs from line gaps, cuts before
//! definitions, and the emergency brake for long blocks.

use pdf_oxide::writer::{DocumentBuilder, PageSize};

use super::pages::ChunkPages;
use super::structural::{chunk_pdf_structurally, DEFAULT_MAX_CHUNK_CHARS};
use crate::db::models::NewChunk;

/// One placed text run: text, x, y (PDF points, y up), bold, font size.
type Run = (String, f32, f32, bool, f32);

fn run(text: &str, x: f32, y: f32, bold: bool, size: f32) -> Run {
    (text.to_string(), x, y, bold, size)
}

/// `n` lines of body text from `y` down, 1.2 × `size` apart; `tag` keeps paragraphs apart.
fn para(tag: &str, n: usize, y: f32, size: f32) -> Vec<Run> {
    (0..n).map(|i| run(&format!("{tag} Zeile {i:02} mit genug Text, damit die Zeile breit genug wird"), 72.0, y - i as f32 * size * 1.2, false, size)).collect()
}

fn pdf(pages: &[Vec<Run>]) -> Vec<u8> {
    let mut doc = DocumentBuilder::new();
    for runs in pages {
        let mut page = doc.page(PageSize::A4);
        for (text, x, y, bold, size) in runs {
            page = page.font(if *bold { "Helvetica-Bold" } else { "Helvetica" }, *size).at(*x, *y).text(text);
        }
        page.done();
    }
    doc.build().expect("pdf")
}

/// Chunks of `pages`, checked for the rules every result keeps: `chunk_index` 0, 1, 2 …;
/// `char_end − char_start` is the text length; each chunk starts where the one before ended
/// (one skipped whitespace byte after an emergency cut); every page change starts a line.
fn chunks(pages: &[Vec<Run>]) -> Vec<NewChunk> {
    let cs = chunk_pdf_structurally(&pdf(pages), 1, 2, None).unwrap();
    let mut prev_end = 0;
    for (i, c) in cs.iter().enumerate() {
        assert_eq!(c.chunk_index, i as i64);
        assert_eq!((c.char_end - c.char_start) as usize, c.text.len(), "chunk {i}");
        assert!(i == 0 || (prev_end..=prev_end + 1).contains(&c.char_start), "chunk {i} starts at {} after {prev_end}", c.char_start);
        prev_end = c.char_end;
        let m: serde_json::Value = serde_json::from_str(&c.metadata).unwrap();
        for b in m["page_breaks"].as_array().expect("page_breaks") {
            let o = b[0].as_u64().unwrap() as usize;
            assert!(o > 0 && o < c.text.len() && c.text.as_bytes()[o - 1] == b'\n', "chunk {i}: page change at {o} is no line start");
        }
    }
    cs
}

fn meta(c: &NewChunk, key: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(&c.metadata).unwrap();
    v[key].as_str().unwrap_or_default().to_string()
}

fn titles(cs: &[NewChunk]) -> Vec<String> {
    cs.iter().map(|c| meta(c, "title")).collect()
}

/// Body paragraph, then `line` after a gap, then another paragraph: the title of the chunk after it.
fn title_after(line: Vec<Run>, size: f32) -> String {
    let mut page = para("Vorher", 4, 780.0, size);
    page.extend(line);
    page.extend(para("Danach", 4, 640.0, size));
    let cs = chunks(&[page]);
    cs.last().map(|c| meta(c, "title")).unwrap_or_default()
}

#[test]
fn a_larger_line_is_a_heading_by_the_documents_own_size() {
    // Body at 10 pt: 12.5 pt is large here, though below 1.1 × 12 pt.
    assert_eq!(title_after(vec![run("Größerer Titel", 72.0, 700.0, false, 12.5)], 10.0), "Größerer Titel");
    assert_eq!(title_after(vec![run("Gleich großer Satzanfang", 72.0, 700.0, false, 10.0)], 10.0), "");
}

#[test]
fn a_short_bold_line_without_a_sentence_end_is_a_heading() {
    assert_eq!(title_after(vec![run("Kurzer Titel", 72.0, 700.0, true, 12.0)], 12.0), "Kurzer Titel");
    let one_bold_word = vec![run("Ein ", 72.0, 700.0, false, 12.0), run("fettes", 95.0, 700.0, true, 12.0), run(" Wort hier", 133.0, 700.0, false, 12.0)];
    assert_eq!(title_after(one_bold_word, 12.0), "", "one bold word makes no heading");
    for end in [".", ",", ":", ";"] {
        assert_eq!(title_after(vec![run(&format!("Fetter Satz{end}"), 72.0, 700.0, true, 12.0)], 12.0), "", "{end}");
    }
    let long = "Eine sehr lange fette Zeile ohne Satzende, die mehr als achtzig Zeichen lang ist und weitergeht";
    assert!(long.len() >= 80);
    assert_eq!(title_after(vec![run(long, 72.0, 700.0, true, 9.0)], 12.0), "");
}

#[test]
fn a_layout_line_with_a_wide_gap_is_never_a_heading() {
    assert_eq!(title_after(vec![run("Kapitelanfang", 72.0, 700.0, true, 12.0), run("17", 400.0, 700.0, true, 12.0)], 12.0), "");
    // A gap of 20 pt (under three font sizes) is no layout gap.
    assert_eq!(title_after(vec![run("Teil", 72.0, 700.0, true, 12.0), run("Zwei", 118.0, 700.0, true, 12.0)], 12.0), "TeilZwei");
}

#[test]
fn a_bold_line_on_a_few_of_many_pages_stays_a_heading() {
    let pages: Vec<Vec<Run>> = (0..16)
        .map(|p| {
            let mut page = if p % 6 == 0 { vec![run("Begründung", 72.0, 800.0, true, 12.0)] } else { Vec::new() };
            page.extend(para(&format!("Blatt{p}"), 4, 770.0, 12.0));
            page
        })
        .collect();
    assert!(titles(&chunks(&pages)).contains(&"Begründung".to_string()), "3 of 16 pages is no running line");
}

#[test]
fn a_new_page_starts_a_paragraph_even_at_the_same_height() {
    let page1 = para("Erste", 4, 754.0, 12.0); // last line at y = 700
    let mut page2 = vec![run("Neuer Teil", 72.0, 700.0, true, 12.0)];
    page2.extend(para("Zweite", 4, 670.0, 12.0));
    assert_eq!(titles(&chunks(&[page1, page2])).last().unwrap(), "Neuer Teil");
}

#[test]
fn a_list_item_under_an_article_extends_its_section_and_keeps_the_title() {
    let mut page = vec![run("Artikel 5 Pflichten", 72.0, 800.0, false, 12.0)];
    page.extend(para("Erster", 4, 780.0, 12.0));
    page.push(run("(a) Die Anbieter sorgen dafür", 72.0, 700.0, false, 12.0));
    page.extend(para("Zweiter", 4, 685.0, 12.0));
    let cs = chunks(&[page]);
    let last = cs.last().unwrap();
    assert_eq!((meta(last, "section"), meta(last, "title")), ("Artikel 5 (a)".to_string(), "Pflichten".to_string()));
}

#[test]
fn a_wrapped_line_opening_with_a_number_does_not_cut_the_paragraph() {
    let mut page = para("Absatz", 3, 780.0, 12.0);
    page.push(run("2024 kam die Wende, und alles lief in dieser Zeile einfach weiter so", 72.0, 780.0 - 3.0 * 14.4, false, 12.0));
    page.extend(para("Rest", 2, 780.0 - 4.0 * 14.4, 12.0));
    let cs = chunks(&[page]);
    assert_eq!(cs.len(), 1, "{:?}", cs.iter().map(|c| &c.text).collect::<Vec<_>>());
    // After a gap the same line is a list item and cuts.
    let mut page = para("Absatz", 3, 780.0, 12.0);
    page.push(run("2024 kam die Wende, und alles lief in dieser Zeile einfach weiter so", 72.0, 700.0, false, 12.0));
    page.extend(para("Rest", 3, 685.0, 12.0));
    assert_eq!(chunks(&[page]).len(), 2);
}

#[test]
fn only_definitions_and_list_items_cut_a_running_paragraph() {
    assert_eq!(chunks(&[para("Lang", 15, 800.0, 12.0)]).len(), 1, "a long paragraph without marks stays whole");
    let mut page = para("Vorher", 4, 800.0, 12.0);
    page.push(run("Anbieter bezeichnet eine natürliche oder juristische Person, die etwas tut", 72.0, 800.0 - 4.0 * 14.4, false, 12.0));
    page.extend(para("Danach", 3, 800.0 - 5.0 * 14.4, 12.0));
    let cs = chunks(&[page]);
    assert_eq!(cs.len(), 2);
    assert!(cs[1].text.starts_with("Anbieter bezeichnet"));
}

/// `n` sentence lines from `y` down, spilling over to new pages after 48 lines each.
fn block(n: usize) -> Vec<Vec<Run>> {
    let line = |i: usize| format!("Zeile {i:03} des langen Blocks ohne jede Gliederung geht hier weiter.");
    let mut pages: Vec<Vec<Run>> = Vec::new();
    for i in 0..n {
        if i % 48 == 0 {
            pages.push(Vec::new());
        }
        pages.last_mut().unwrap().push(run(&line(i), 72.0, 800.0 - (i % 48) as f32 * 14.4, false, 12.0));
    }
    pages
}

#[test]
fn the_emergency_brake_cuts_long_blocks_near_the_limit_only() {
    let max = DEFAULT_MAX_CHUNK_CHARS;
    let cs = chunks(&block(90));
    assert!(cs.len() > 1);
    for (i, c) in cs.iter().enumerate() {
        let n = c.text.len();
        assert!(n <= 2 * max + 80, "chunk {i} has {n}");
        assert!(i + 1 == cs.len() || n >= max, "chunk {i} has only {n}");
    }
    assert!(cs.iter().any(|c| ChunkPages::from_metadata(&c.metadata).unwrap().range().0 > 1), "a cut chunk starts on page 2");
    assert_eq!(chunks(&block(35)).len(), 1, "under twice the limit the block stays whole");
    let one_sentence = vec![(0..50).map(|i| run(&format!("ohne Satzende geht es in Zeile {i:02} weiter und immer weiter und weiter"), 72.0, 800.0 - i as f32 * 14.4, false, 12.0)).collect()];
    let cs = chunks(&one_sentence);
    assert_eq!(cs.len(), 1, "a block without a sentence end has nothing to cut at");
}

#[test]
fn a_short_last_block_joins_the_chunk_before() {
    let mut page = para("Vorher", 4, 780.0, 12.0);
    page.push(run("Schluss", 72.0, 700.0, true, 12.0));
    page.extend(para("Kurz", 1, 685.0, 12.0));
    let cs = chunks(&[page]);
    assert_eq!(cs.len(), 1);
    assert!(cs[0].text.ends_with("Kurz Zeile 00 mit genug Text, damit die Zeile breit genug wird"));
}
