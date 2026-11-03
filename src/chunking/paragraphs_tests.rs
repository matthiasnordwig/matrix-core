//! Paragraphs that get a vector of their own (`paragraphs.rs`).

use super::paragraphs::{paragraph_units, MAX_UNIT_CHARS};

const LONG_A: &str = "The regulatory team expects that we stand to lose a large sum in California if the utilities do not pay.";
const LONG_B: &str = "Credit exposure to the two utilities stays the largest single risk of the retail business this quarter.";

fn texts(text: &str, min_words: usize) -> Vec<&str> {
    paragraph_units(text, min_words).into_iter().map(|(s, e)| &text[s..e]).collect()
}

#[test]
fn each_long_paragraph_alone_short_ones_left_out() {
    let text = format!("Hi Greg,\n\n{LONG_A}\n\nThanks!\n\n{LONG_B}\n");
    assert_eq!(texts(&text, 12), vec![LONG_A, LONG_B]);
    assert!(texts(&text, 25).is_empty(), "below two paragraphs none");
}

#[test]
fn header_blocks_and_signatures_get_no_vector() {
    let header = "From: Anna Maier\nSent: Friday, December 29, 2000 10:23 AM\nTo: Greg Whalley, John Lavorato, Louise Kitchen, Sally Beck\nSubject: EES pre-holiday update on the regulatory and credit risk";
    let mail_head = "# EES Pre-holiday Update\n\n**From:** Lavorato, John <john.lavorato@enron.com> on behalf of the risk group of the retail business";
    let signature = "Anna Maier\nDirector, Risk Management\nEnron Energy Services\nPhone: +1 713 853 1234\nanna.maier@enron.com";
    let text = format!("{mail_head}\n\n{header}\n\n{LONG_A}\n\n{signature}\n\n{LONG_B}");
    assert_eq!(texts(&text, 5), vec![LONG_A, LONG_B]);
}

#[test]
fn a_label_line_in_running_text_is_no_header() {
    let para = format!("Regulatory: {LONG_A}\nThe rest of the regulatory section follows here.");
    let text = format!("{para}\n\n{LONG_B}");
    assert_eq!(texts(&text, 12), vec![para.as_str(), LONG_B]);
}

#[test]
fn ranges_are_byte_offsets_and_long_paragraphs_are_cut() {
    let long = "Über Strom und Gas wird viel gesagt. ".repeat(MAX_UNIT_CHARS / 30);
    let text = format!("{LONG_A}\n\n{long}");
    let units = paragraph_units(&text, 12);
    assert!(units.len() > 2, "the long paragraph is cut by sentences");
    assert!(units.iter().all(|&(s, e)| text.is_char_boundary(s) && text.is_char_boundary(e) && text[s..e].chars().count() <= MAX_UNIT_CHARS));
}
