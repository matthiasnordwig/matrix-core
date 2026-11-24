//! Tests of `slide_steps.rs`: a slide built up over three pages is one run kept at its last page; a changed
//! number, a corrected word or a dropped point splits it; ordinary pages and pages sharing only a header
//! never join.

use super::*;

fn pages(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|t| t.to_string()).collect()
}

const S1: &str = "Results of the survey\nMost readers use the app daily";
const S2: &str = "Results of the survey\nMost readers use the app daily\nTwo thirds read on the phone";
const S3: &str = "Results of the survey\nMost readers use the app daily\nTwo thirds read on the phone\n5 % never open it";

#[test]
fn three_build_steps_become_one_run_kept_at_the_full_slide() {
    let got = step_runs(&pages(&["Title page of the talk, March 2026", S1, S2, S3, "Methods\nWe asked 200 readers in three cities"]));
    assert_eq!(got, vec![StepRun { first: 1, last: 3, kept: 3 }]);
}

#[test]
fn a_changed_number_keeps_the_pages_apart() {
    // The slide rule calls them the same slide (all but one word shared); the number is not on the kept page.
    let next = S3.replace("5 %", "6 %");
    assert!(same_words(&words(S3), &words(&next)) == Some(true));
    assert!(step_runs(&pages(&[S3, &next])).is_empty());
    // A single digit counts too (the slide rule ignores one-letter words).
    assert!(step_runs(&pages(&["Budget 2026 plan\nTeam of 4 people", "Budget 2026 plan\nTeam of 5 people\nStart in May"])).is_empty());
}

#[test]
fn a_corrected_word_or_a_dropped_point_keeps_the_pages_apart() {
    let corrected = S2.replace("thirds", "third");
    assert!(step_runs(&pages(&[S2, &corrected])).is_empty());
    // A later page without an earlier point: the earlier page holds a word the kept one lacks.
    let dropped = "Results of the survey\nTwo thirds read on the phone\nMost readers like it";
    assert!(step_runs(&pages(&[S2, dropped])).is_empty());
}

#[test]
fn a_repeated_word_must_stand_as_often_on_the_kept_page() {
    assert!(covers("one two two three", "two two"));
    assert!(!covers("one two three", "two two"));
}

#[test]
fn ordinary_pages_and_a_shared_header_do_not_join() {
    let a = "Wikilokal | Story\nInspiration from location based discovery\nConference 2026";
    let b = "Wikilokal | Advantages\nWorks on web and Android in many languages\nConference 2026";
    assert!(step_runs(&pages(&[a, b])).is_empty());
    let text = |i: usize| format!("Page {i} of the report tells part {i} of the story with its own sentences number {i}");
    assert!(step_runs(&(1..=5).map(text).collect::<Vec<_>>()).is_empty());
}

#[test]
fn pages_without_enough_words_never_join() {
    assert!(step_runs(&pages(&["", "", "Agenda", "Agenda"])).is_empty());
}

#[test]
fn two_built_up_slides_in_a_row_are_two_runs() {
    let t1 = "Plan for spring\nFirst we read the files";
    let t2 = "Plan for spring\nFirst we read the files\nThen we sort them by date";
    let got = step_runs(&pages(&[S1, S2, t1, t2]));
    assert_eq!(got, vec![StepRun { first: 0, last: 1, kept: 1 }, StepRun { first: 2, last: 3, kept: 3 }]);
}

#[test]
fn an_identical_repeated_page_joins_and_the_later_one_is_kept() {
    assert_eq!(step_runs(&pages(&[S2, S2])), vec![StepRun { first: 0, last: 1, kept: 1 }]);
}

/// A PDF with one page per text (lines 30 pt apart, a 24 pt title then 14 pt body), as a slide export writes
/// them.
fn slide_pdf(slides: &[&str]) -> Vec<u8> {
    let mut w = pdf_oxide::writer::PdfWriter::new();
    for text in slides {
        let mut page = w.add_page(720.0, 405.0);
        for (i, line) in text.lines().enumerate() {
            let size = if i == 0 { 24.0 } else { 14.0 };
            page.add_text(line, 40.0, 360.0 - 30.0 * i as f32, "Helvetica", size);
        }
        page.finish();
    }
    w.finish().unwrap()
}

#[test]
fn the_chunker_keeps_the_full_slide_once_with_the_page_range_of_its_steps() {
    use super::super::pages::ChunkPages;
    use super::super::structural::chunk_pdf_structurally;
    let pdf = slide_pdf(&["Readers in 2026\nA survey among readers of the app", S1, S2, S3, "Methods\nWe asked 200 readers in three cities"]);
    let chunks = chunk_pdf_structurally(&pdf, 1, 1, None).unwrap();
    let all: String = chunks.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(all.matches("Most readers use the app daily").count(), 1, "the steps are left out: {all}");
    let unit = chunks.iter().find(|c| c.text.contains("never open it")).expect("the full slide");
    // Short slides flow into one chunk; the unit is the page change to 2 and at the same offset to 4.
    let meta: serde_json::Value = serde_json::from_str(&unit.metadata).unwrap();
    let at = unit.text.find("Results of the survey").unwrap() as u64;
    let breaks = meta["page_breaks"].as_array().unwrap();
    assert!(breaks.windows(2).any(|w| w[0] == serde_json::json!([at, 2]) && w[1] == serde_json::json!([at, 4])), "{meta}");
    let pages = ChunkPages::from_metadata(&unit.metadata).unwrap();
    assert_eq!(pages.label(), "pp. 1\u{2013}5");
    // The text and a quote from it stand on the kept page; the step pages get no mark of their own.
    assert_eq!(pages.locate(&unit.text, |part| part.contains("Two thirds read")), Some((4, 4)));
    let marked = pages.mark(&unit.text);
    assert!(marked.contains("[p. 4]\nResults of the survey") && !marked.contains("[p. 2]"), "{marked}");
    // A slide whose number changed stays its own unit.
    let changed = S3.replace("5 %", "6 %");
    let pdf = slide_pdf(&[S3, &changed]);
    let all: String = chunk_pdf_structurally(&pdf, 1, 1, None).unwrap().iter().map(|c| c.text.clone()).collect();
    assert!(all.contains("5 % never") && all.contains("6 % never"), "{all}");
}
