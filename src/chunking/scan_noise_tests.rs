//! Tests for `scan_noise.rs`: recognition noise in place of punctuation and line-end hyphens.

use super::scan_noise::*;
use super::scan_text::{assemble, clean_pages, PageLine};

fn pl(text: &str, gap: bool) -> PageLine {
    PageLine { text: text.to_string(), gap, indent: false, heading: false }
}

/// Plain German lines, so the noise letters stay far below the share of a script in use.
fn filler() -> Vec<PageLine> {
    let line = "Der Onkel sah den Heizer an und sagte nichts, weil er bemerkt hatte, dass Karl schon zu viel geredet hatte.";
    (0..12).map(|i| pl(line, i == 0)).collect()
}

fn repaired(lines: Vec<PageLine>) -> String {
    let mut pages = vec![(1, filler()), (2, lines)];
    let mut extra = clean_pages(&mut pages);
    extra.running_examples.clear();
    assemble(&pages[1..]).stored
}

#[test]
fn noise_at_a_line_end_before_a_lower_case_word_is_a_hyphen() {
    // The i before the run is noise too: "bemerkt" occurs in the text, "beimerkt" does not.
    assert_eq!(repaired(vec![pl("Ich habe es so beiại", true), pl("merkt, aber gut.", false)]), "Ich habe es so bemerkt, aber gut.");
    assert_eq!(repaired(vec![pl("am Tisch des OberạẠạ", true), pl("kassiers stand er.", false)]), "am Tisch des Oberkassiers stand er.");
}

#[test]
fn noise_between_two_words_of_the_text_is_a_space() {
    // "sagte" and "Heizer" are words of the text, "Heizersagte" is none: a guillemet, no hyphen.
    assert_eq!(repaired(vec![pl("Das war der Heizeriạ", true), pl("sagte er laut.", false)]), "Das war der Heizer sagte er laut.");
    assert_eq!(repaired(vec![pl("Das ist zu argiạẠạUnd er schlug zu.", true)]), "Das ist zu arg Und er schlug zu.");
    assert_eq!(repaired(vec![pl("fragte er nicht?ạJa, das", true)]), "fragte er nicht? Ja, das");
    assert_eq!(repaired(vec![pl("ạIch werde sofort gehen.", true), pl("Und nun Karl", true)]), "Ich werde sofort gehen.\n\nUnd nun Karl");
}

#[test]
fn an_i_of_the_word_stays_when_the_text_has_the_word() {
    let mut lines = filler();
    lines.push(pl("Er war frei und froh, so frei wie nie.", true));
    lines.push(pl("Nun war er freiạẠạ", true));
    let mut pages = vec![(1, lines)];
    clean_pages(&mut pages);
    assert_eq!(pages[0].1.last().unwrap().text, "Nun war er frei");
}

#[test]
fn vietnamese_text_is_left_alone() {
    let mut pages = vec![(1, vec![pl("Tôi đã ở lại đây một ngày, rồi đi học tiếng Việt.", true); 3])];
    let r = clean_pages(&mut pages);
    assert_eq!(r.noise_runs, 0);
    assert_eq!(pages[0].1[0].text, "Tôi đã ở lại đây một ngày, rồi đi học tiếng Việt.");
    assert!(!is_foreign(["Tôi đã ở lại đây một ngày"]));
    assert!(is_foreign(["Ein langer deutscher Satz mit vielen Wörtern und genau einem Fehler hierạ am Ende des Satzes, der noch weiter läuft und läuft."]));
}
