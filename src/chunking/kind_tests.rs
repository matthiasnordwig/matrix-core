//! Tests of `kind.rs` beside the per-kind examples in `rules_tests.rs`: which part of a long text
//! is read, shares per line and density per 10 000 characters, the book rules, returning speakers,
//! the lead a kind needs, and the plausibility check of a chunker's result.

use super::*;

fn prose(n: usize) -> String {
    "Der Tag begann still, und niemand im Haus sprach ein Wort, bis es Abend wurde und die Lampen brannten. ".repeat(n)
}

fn kind(text: &str) -> DocKind {
    detect_kind(text, "dokument.txt").kind
}

#[test]
fn a_long_text_is_read_at_its_start_middle_and_end() {
    let text: String = (0..100_000).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
    let n = text.len();
    assert_eq!(sample(&text), [&text[..SAMPLE_HEAD], &text[n / 2..n / 2 + SAMPLE_TAIL], &text[n - SAMPLE_TAIL..]]);
    let short = &text[..SAMPLE_HEAD + 2 * SAMPLE_TAIL];
    assert_eq!(sample(short), [short]);
}

#[test]
fn marks_in_the_middle_or_at_the_end_of_a_long_text_count() {
    let filler = format!("{}\n\n", prose(5)).repeat(70);
    let law: String = (1..=8).map(|i| format!("§ {i} Regelung\n\nWer prüft, hält es fest.\n\n")).collect();
    assert_eq!(kind(&format!("{filler}{law}{filler}")), DocKind::Legal, "law only in the middle");
    let talk: String = (0..150).map(|i| format!("{}: Das sehe ich anders.\n", if i % 2 == 0 { "Anna" } else { "Ben" })).collect();
    assert_eq!(kind(&format!("{filler}{filler}{talk}")), DocKind::Transcript, "speakers only at the end");
}

#[test]
fn a_few_marked_lines_among_many_lines_of_prose_give_no_kind() {
    let lines: String = (0..100).map(|i| format!("Zeile {i} eines Briefes ohne jede Gliederung.\n")).collect();
    let t = format!("[12.03.24, 14:22] Anna: Hallo\n[12.03.24, 14:25] Ben: Hi\n[12.03.24, 14:26] Anna: Gut\n{lines}");
    assert_eq!(kind(&t), DocKind::Other);
}

#[test]
fn time_marks_make_a_transcript() {
    let t: String = (0..20).map(|i| format!("[{}:{:02}] Wir gehen die Punkte der Sitzung der Reihe nach durch.\n", i / 6, i * 10 % 60)).collect();
    assert_eq!(kind(&t), DocKind::Transcript);
}

#[test]
fn a_book_keeps_its_chapters_beside_numbered_headings() {
    let chapter = |i: usize| format!("Kapitel {i}\n\n{i}.1 Ein Abschnitt\n\n{}", format!("{}\n\n", prose(5)).repeat(20));
    assert_eq!(kind(&(1..=4).map(chapter).collect::<String>()), DocKind::Book);
}

#[test]
fn runs_of_blank_lines_do_not_shorten_the_paragraphs_of_a_book() {
    assert_eq!(kind(&format!("{}\n\n\n\n", prose(4)).repeat(100)), DocKind::Book);
}

#[test]
fn length_alone_or_long_paragraphs_alone_make_no_book() {
    assert_eq!(kind(&format!("{}\n\n", prose(1)).repeat(400)), DocKind::Other, "long, but short paragraphs");
    assert_eq!(kind(&format!("{}\n\n", prose(6)).repeat(3)), DocKind::Other, "long paragraphs, but short");
}

const STEPS: &str = "1. Gehäuse öffnen\n2. Karte einsetzen\n3. Schraube anziehen\n4. Gehäuse schließen\n5. Strom einschalten\n";

#[test]
fn a_manual_with_few_steps_per_text_is_told_by_its_warnings() {
    let warn = "Achtung: Strom abschalten.\n";
    let t = format!("{}\n\n{warn}{STEPS}6. Licht prüfen\n{warn}{warn}", prose(80));
    assert_eq!(kind(&t), DocKind::Manual);
}

#[test]
fn a_long_report_with_a_few_numbered_lines_is_no_manual() {
    assert_eq!(kind(&format!("{}\n\n{STEPS}", prose(200))), DocKind::Report);
}

#[test]
fn labels_that_do_not_come_back_are_no_speakers() {
    let labels = ["Projekt", "Kunde", "Ort", "Beginn", "Ende", "Leitung", "Team", "Budget", "Status", "Risiko", "Ziel", "Phase"];
    let mut t: String = labels.iter().map(|l| format!("{l}: Alpha und Beta\n")).collect();
    t += "Projekt: Gamma\nKunde: Delta\n";
    assert_ne!(kind(&t), DocKind::Transcript);
}

#[test]
fn two_kinds_alike_give_other() {
    let t = format!("TOP 1 Begrüßung\nTOP 2 Haushalt\nTOP 3 Termine\n{STEPS}");
    let d = detect_kind(&t, "x.txt");
    assert_eq!(d.kind, DocKind::Other, "{}", d.reason);
}

fn chunk(text: &str) -> NewChunk {
    NewChunk {
        context_id: 1, document_id: 2, chunk_index: 0, char_start: 0, char_end: text.len() as i64,
        text: text.to_string(), signature: None, is_omitted: false, metadata: "{}".into(),
    }
}

#[test]
fn plausibility_keeps_short_complete_results_and_rejects_lost_text() {
    let text = prose(3);
    assert!(is_plausible(&text, &[chunk(&text)], 3000), "one short chunk with the whole text");
    let half: String = text.chars().take(text.chars().count() / 2).collect();
    assert!(!is_plausible(&text, &[chunk(&half)], 3000), "half the text lost");
    let mut parts: Vec<NewChunk> = (0..25).map(|_| chunk(&"w".repeat(100))).collect();
    parts.push(chunk("kurz"));
    let whole: String = parts.iter().map(|c| c.text.clone()).collect::<Vec<_>>().join(" ");
    assert!(is_plausible(&whole, &parts, 3000), "one short chunk among many full ones is no crumbs");
    let few: Vec<NewChunk> = (0..5).map(|_| chunk(&"w".repeat(20))).collect();
    assert!(is_plausible(&"wwwwwwwwwwwwwwwwwwww ".repeat(5), &few, 3000), "too few chunks to call them crumbs");
}
