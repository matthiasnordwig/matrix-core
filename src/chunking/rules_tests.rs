//! Document kinds and the line-marker chunker on small texts of each kind.

use super::kind::{detect_kind, is_plausible, DocKind};
use super::rules::{chunk_by_rules, max_chars};
use crate::db::models::NewChunk;

fn path(c: &NewChunk) -> String {
    let m: serde_json::Value = serde_json::from_str(&c.metadata).unwrap();
    m["heading_path"].as_str().unwrap().to_string()
}

fn assert_verbatim(t: &str, cs: &[NewChunk]) {
    let mut prev = 0;
    for (i, c) in cs.iter().enumerate() {
        assert_eq!(c.chunk_index, i as i64);
        assert_eq!(&t[c.char_start as usize..c.char_end as usize], c.text);
        assert!(c.char_start >= prev);
        prev = c.char_end;
    }
}

fn kind(text: &str, file: &str) -> DocKind {
    detect_kind(text, file).kind
}

fn prose(n: usize) -> String {
    "Der Tag begann still, und niemand im Haus sprach ein Wort, bis es Abend wurde und die Lampen brannten. ".repeat(n)
}

fn law() -> String {
    let mut t = String::from("Gesetz über die Beispiele\n\nTeil 1\n\n");
    for i in 1..=6 {
        t += &format!("§ {i} Regelung {i}\n\n(1) Wer ein Beispiel gibt, muss es prüfen und die Prüfung festhalten.\n\n(2) Das gilt auch für Muster, die öffentlich zugänglich sind und bleiben.\n\n");
    }
    t
}

#[test]
fn law_is_cut_at_paragraphs_with_its_path() {
    let t = law();
    assert_eq!(kind(&t, "gesetz.txt"), DocKind::Legal);
    let cs = chunk_by_rules(&t, 1, 2, DocKind::Legal);
    assert_verbatim(&t, &cs);
    assert_eq!(cs.len(), 6, "{:?}", cs.iter().map(path).collect::<Vec<_>>());
    assert!(cs[0].text.contains("Teil 1\n\n§ 1"), "a bare part heading stays with its first paragraph");
    assert_eq!(path(&cs[0]), "Teil 1 › § 1 Regelung 1");
    assert_eq!(path(&cs[5]), "Teil 1 › § 6 Regelung 6");
}

#[test]
fn long_paragraph_of_law_is_cut_at_its_subsections() {
    let body = (1..=40).map(|i| format!("({i}) {}\n", "Wer das tut, muss es sagen. ".repeat(20))).collect::<String>();
    let t = format!("§ 1 Lang\n\n{body}§ 2 Kurz und ohne jede weitere Bestimmung, die hier noch etwas regeln wollte oder sollte.\n");
    let cs = chunk_by_rules(&t, 1, 2, DocKind::Legal);
    assert_verbatim(&t, &cs);
    assert!(cs.len() > 2);
    assert!(cs.iter().all(|c| c.text.chars().count() <= max_chars(DocKind::Legal)));
    assert!(cs[1].text.starts_with('('), "cut at a subsection: {}", &cs[1].text[..20]);
}

#[test]
fn minutes_are_cut_at_agenda_items() {
    let t = format!(
        "Protokoll der Sitzung\n\nAnwesend: alle\n\nTOP 1 Begrüßung\n\n{a}\n\nTOP 2 Haushalt\n\n{a}\n\nTOP 3 Termine\n\n{a}\n",
        a = prose(4)
    );
    assert_eq!(kind(&t, "sitzung.docx"), DocKind::Minutes);
    let cs = chunk_by_rules(&t, 1, 2, DocKind::Minutes);
    assert_verbatim(&t, &cs);
    assert_eq!(cs.iter().map(path).filter(|p| p.starts_with("TOP")).count(), 3);
}

#[test]
fn transcript_keeps_short_turns_together_and_names_speakers() {
    let turn = |who: &str| format!("{who}: {}\n\n", "Das sehe ich anders, und zwar aus gutem Grund, wie ich gleich ausführen werde. ".repeat(3));
    let t = (0..10).map(|i| turn(if i % 2 == 0 { "Anna Maier" } else { "Ben Vogt" })).collect::<String>();
    assert_eq!(kind(&t, "interview.txt"), DocKind::Transcript);
    let cs = chunk_by_rules(&t, 1, 2, DocKind::Transcript);
    assert_verbatim(&t, &cs);
    assert!(cs.len() < 10);
    assert!(["Anna Maier", "Ben Vogt"].contains(&path(&cs[0]).as_str()));
}

#[test]
fn chat_export_starts_anew_after_a_pause() {
    let t = "[12.03.24, 14:22] Anna: Hallo\n[12.03.24, 14:25] Ben: Hi\n[12.03.24, 18:00] Anna: Später noch?\n[13.03.24, 09:00] Ben: Morgen\n".repeat(1);
    assert_eq!(kind(&t, "chat.txt"), DocKind::Chat);
    let cs = chunk_by_rules(&t, 1, 2, DocKind::Chat);
    assert_verbatim(&t, &cs);
    assert_eq!(cs.len(), 3);
    assert!(cs[0].text.ends_with("Ben: Hi"));
}

#[test]
fn code_is_cut_at_definitions_with_their_comments() {
    let f = |n: usize| format!("/// Does {n}.\npub fn f{n}() -> u32 {{\n{}    {n}\n}}\n\n", "    let a = 1;\n".repeat(12));
    let t = format!("use std::fmt;\n\n{}{}", f(1), f(2));
    assert_eq!(kind(&t, "src/lib.rs"), DocKind::Code);
    let cs = chunk_by_rules(&t, 1, 2, DocKind::Code);
    assert_verbatim(&t, &cs);
    let last = cs.last().unwrap();
    assert!(last.text.starts_with("/// Does 2."), "{}", last.text);
    assert!(path(last).contains("fn f2"));
}

#[test]
fn book_is_cut_at_chapters_then_paragraphs() {
    let para = format!("{}\n\n", prose(5));
    let chapter = |i: usize| format!("Kapitel {i}\n\n{}", para.repeat(30));
    let t = (1..=4).map(chapter).collect::<String>();
    assert_eq!(kind(&t, "roman.txt"), DocKind::Book);
    let cs = chunk_by_rules(&t, 1, 2, DocKind::Book);
    assert_verbatim(&t, &cs);
    assert!(cs.iter().all(|c| c.text.chars().count() <= max_chars(DocKind::Book)));
    assert!(cs.iter().all(|c| c.text.ends_with('.')), "cut inside a paragraph");
    assert_eq!(path(&cs[cs.len() - 1]), "Kapitel 4");
}

#[test]
fn headings_alone_make_a_report_steps_a_manual() {
    let report = format!("# Lage\n\n{a}\n\n## Markt\n\n{a}\n\n## Risiken\n\n{a}\n\n## Ausblick\n\n{a}\n", a = prose(2));
    assert_eq!(kind(&report, "bericht.docx"), DocKind::Report);
    let manual = "# Einbau\n\nAchtung: Strom abschalten.\n\n1. Gehäuse öffnen\n2. Karte einsetzen\n3. Schraube anziehen\n4. Gehäuse schließen\n5. Strom einschalten\n6. Licht prüfen\n";
    assert_eq!(kind(manual, "anleitung.md"), DocKind::Manual);
}

#[test]
fn slides_and_forms_are_told_apart() {
    let slides = "# Slide 1: Start\n\nA\n\n# Slide 2: Mitte\n\nB\n\n# Slide 3: Ende\n\nC\n";
    assert_eq!(kind(slides, "deck.pptx"), DocKind::Slides);
    let form = "Rechnung\n\nNummer: 123\nDatum: 01.02.2026\nBetrag: 12,50 EUR\nIBAN: DE00\nEmpfänger: Muster GmbH\n";
    assert_eq!(kind(form, "beleg.pdf"), DocKind::Form);
}

#[test]
fn plain_text_without_marks_is_other() {
    assert_eq!(kind("Ein kurzer Text ohne jede Gliederung.\n\nNoch ein Absatz.\n", "notiz.txt"), DocKind::Other);
}

#[test]
fn a_report_citing_paragraphs_inside_sentences_is_no_law() {
    let t = format!("# Prüfung\n\n{}Nach § 5 gilt das. Auch § 6 und § 7 sind einschlägig.\n\n## Teil\n\ntext\n\n## Ende\n\ntext\n", prose(2));
    assert_eq!(kind(&t, "bericht.md"), DocKind::Report);
}

#[test]
fn name_round_trip_covers_every_kind() {
    for k in DocKind::ALL {
        assert_eq!(DocKind::from_name(k.name()), Some(k));
    }
}

#[test]
fn plausibility_rejects_one_huge_block_and_crumbs() {
    let huge = prose(100);
    let cs = chunk_by_rules(&format!("§ 1 Alles\n\n{huge}"), 1, 2, DocKind::Legal);
    assert!(is_plausible(&format!("§ 1 Alles\n\n{huge}"), &cs, max_chars(DocKind::Legal)));
    let one = NewChunk {
        context_id: 1, document_id: 2, chunk_index: 0, char_start: 0, char_end: huge.len() as i64,
        text: huge.clone(), signature: None, is_omitted: false, metadata: "{}".into(),
    };
    assert!(!is_plausible(&huge, std::slice::from_ref(&one), 3000));
    let crumbs: Vec<NewChunk> = (0..30).map(|i| NewChunk { chunk_index: i, text: "ab".into(), ..one.clone() }).collect();
    assert!(!is_plausible(&"ab ".repeat(30), &crumbs, 3000));
}

#[test]
fn a_licence_with_numbered_sections_at_the_end_of_a_book_is_no_law() {
    let chapters = (1..=4).map(|i| format!("Chapter {i}\n\n{}\n\n", prose(60))).collect::<String>();
    let licence = (1..=6).map(|i| format!("Section {i}. Terms\n\nYou may copy this.\n\n")).collect::<String>();
    let filler = format!("{}\n\n", prose(5)).repeat(40);
    assert_eq!(kind(&format!("{chapters}{filler}{filler}{licence}"), "book.txt"), DocKind::Book);
}
