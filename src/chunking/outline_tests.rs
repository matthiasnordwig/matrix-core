//! Tests of `outline.rs`: paths and sections of legal acts (DE "(1)" and EN "1." paragraphs),
//! numbered reports (table of contents skipped, list items not taken for headings), control
//! catalogues, Markdown, and documents without headings (no path).

use super::outline::{Outline, OutlineChunk, OutlineSection};

fn outline(chunks: &[&str]) -> Outline {
    let input: Vec<OutlineChunk> = chunks
        .iter()
        .enumerate()
        .map(|(i, t)| OutlineChunk { id: i as i64 + 1, text: t, markdown: false })
        .collect();
    Outline::build(&input)
}

const AI_ACT: [&str; 7] = [
    "(1)Zweck dieser Verordnung ist es, das Funktionieren des Binnenmarkts zu verbessern.\n(2)Diese Verordnung sollte angewandt werden.",
    "KAPITEL I\nALLGEMEINE BESTIMMUNGEN\nArtikel 1\nGegenstand\n(1)Zweck dieser Verordnung ist es.\n(2)Diese Verordnung enthält:\na)harmonisierte Vorschriften;",
    "Artikel 2\nAnwendungsbereich\n(1)Diese Verordnung gilt für Anbieter.\n21. Mai 2024.\n(2)Für KI-Systeme gilt nur Artikel 6.",
    "(3)Diese Verordnung gilt nicht für Bereiche außerhalb des Unionsrechts.\n(4)Ausnahmen.\nKAPITEL III\nHOCHRISIKO-KI-SYSTEME\nABSCHNITT 3\nPflichten\nArtikel 18",
    "Aufbewahrung der Dokumentation\n(1)Der Anbieter hält für einen Zeitraum von zehn Jahren bereit:\na)die technische Dokumentation;",
    "DE\nABl. L vom 12.7.2024\n(2)Jeder Mitgliedstaat legt die Bedingungen fest.\n(3)Anbieter, die Finanzinstitute sind, pflegen die Dokumentation.",
    "ANHANG I\nListe der Harmonisierungsrechtsvorschriften\n1.Richtlinie 2006/42/EG",
];

#[test]
fn legal_act_paths_carry_chapter_section_article_and_paragraphs() {
    let o = outline(&AI_ACT);
    assert_eq!(o.path(1), None, "recitals before the first article have no path");
    assert_eq!(o.path(2).as_deref(), Some("KAPITEL I › Artikel 1 – Gegenstand › (1)–(2)"));
    // "21. Mai 2024." is no paragraph: numbering continues with (2).
    assert_eq!(o.path(3).as_deref(), Some("KAPITEL I › Artikel 2 – Anwendungsbereich › (1)–(2)"));
    // Begins in Article 2, ends with the heading of Article 18: belongs to Article 2.
    assert_eq!(o.path(4).as_deref(), Some("KAPITEL I › Artikel 2 – Anwendungsbereich › (3)–(4)"));
    // Title line at the start of the chunk, heading at the end of the previous one.
    assert_eq!(
        o.path(5).as_deref(),
        Some("KAPITEL III › ABSCHNITT 3 › Artikel 18 – Aufbewahrung der Dokumentation › (1)")
    );
    // Page header lines are neither title nor paragraph.
    assert_eq!(
        o.path(6).as_deref(),
        Some("KAPITEL III › ABSCHNITT 3 › Artikel 18 – Aufbewahrung der Dokumentation › (2)–(3)")
    );
    // An annex resets chapter and article; its "1." items are no paragraphs.
    assert_eq!(o.path(7).as_deref(), Some("ANHANG I – Liste der Harmonisierungsrechtsvorschriften"));
}

#[test]
fn section_is_the_whole_article_and_stops_at_the_next_one() {
    let o = outline(&AI_ACT);
    assert_eq!(
        o.section(6),
        Some(OutlineSection {
            path: "KAPITEL III › ABSCHNITT 3 › Artikel 18 – Aufbewahrung der Dokumentation".into(),
            chunk_ids: vec![5, 6],
        })
    );
    // Article 2 spans chunks 3 and 4; chunk 4 holds the heading of Article 18 but no line of it.
    assert_eq!(o.section(3).unwrap().chunk_ids, vec![3, 4]);
    assert_eq!(o.section(4).unwrap().chunk_ids, vec![3, 4]);
    assert_eq!(o.section(1), None);
    assert_eq!(o.section(99), None);
}

#[test]
fn english_act_numbers_paragraphs_with_a_dot() {
    let o = outline(&[
        "Article 1\nSubject matter\n1.   This Directive lays down measures.\n2.   To that end, it lays down obligations.",
        "27.12.2022     Official Journal of the European Union     L 333/80\n3.   This Directive is without prejudice.",
        "Article 2\nScope\n1.   This Directive applies to medium-sized enterprises.",
        "Article 3\nDefinitions\nFor the purposes of this Directive:\n(1) 'network' means a system;",
    ]);
    assert_eq!(o.path(1).as_deref(), Some("Article 1 – Subject matter › 1.–2."));
    assert_eq!(o.path(2).as_deref(), Some("Article 1 – Subject matter › 3."));
    assert_eq!(o.path(4).as_deref(), Some("Article 3 – Definitions › (1)"));
    assert_eq!(o.section(2).unwrap().chunk_ids, vec![1, 2]);
}

#[test]
fn two_article_lines_do_not_make_a_legal_act() {
    let o = outline(&["Artikel 1\nGegenstand\n(1)Text.", "Artikel 2\nZiel\n(1)Text."]);
    assert_eq!(o.path(1), None);
}

#[test]
fn numbered_report_skips_table_of_contents_and_list_items() {
    let o = outline(&[
        "Contents\n1. Introduction     3\n2. Incident Response     5\n2.1 Life Cycle Model     6",
        "1. Introduction\nThis document helps organizations.\n1.1. Purpose and Scope\nThe purpose is to help.",
        "2. Incident Response\nIntro text.\n2.1. Incident Response Life Cycle Model\nThe model has functions:\n1. Govern the program\n4. Analyze the gaps, and create an action plan.",
        "2.2. Roles and Responsibilities\nEveryone has a role.\n2.0 Community Profile. Doing so provides a taxonomy",
    ]);
    assert_eq!(o.path(1), None, "table of contents has page numbers");
    assert_eq!(o.path(2).as_deref(), Some("1. Introduction"));
    assert_eq!(o.path(3).as_deref(), Some("2. Incident Response"));
    assert_eq!(o.path(4).as_deref(), Some("2. Incident Response › 2.2. Roles and Responsibilities"));
    // "2.1" includes its text up to "2.2"; list items "1." / "4." are not headings.
    let s = o.section(3).unwrap();
    assert_eq!(s.path, "2. Incident Response");
    assert_eq!(s.chunk_ids, vec![3, 4], "a section includes its subsections");
}

#[test]
fn control_catalogue_has_controls_and_numbered_enhancements() {
    let o = outline(&[
        "3.1   ACCESS CONTROL\nQuick link to Access Control summary table",
        "AC-7 UNSUCCESSFUL LOGON ATTEMPTS\nControl:\na. Enforce a limit;\n1. Policy [Assignment: organization-defined]",
        "Control Enhancements:\n(1)UNSUCCESSFUL LOGON ATTEMPTS | AUTOMATIC ACCOUNT LOCK\n[Withdrawn: Incorporated into AC-7.]",
        "AC-8 SYSTEM USE NOTIFICATION\nControl: Display a notification.\n4. Users are accessing a U.S. Government system",
    ]);
    assert_eq!(o.path(2).as_deref(), Some("3.1 ACCESS CONTROL › AC-7 UNSUCCESSFUL LOGON ATTEMPTS"));
    assert_eq!(o.path(3).as_deref(), Some("3.1 ACCESS CONTROL › AC-7 UNSUCCESSFUL LOGON ATTEMPTS › (1)"));
    assert_eq!(o.path(4).as_deref(), Some("3.1 ACCESS CONTROL › AC-8 SYSTEM USE NOTIFICATION"));
    assert_eq!(o.section(3).unwrap().chunk_ids, vec![2, 3]);
}

#[test]
fn markdown_headings_only_in_markdown_chunks() {
    let chunks = [
        OutlineChunk { id: 1, text: "# Handbook\nIntro.", markdown: true },
        OutlineChunk { id: 2, text: "## Setup\n### Install\nRun it.", markdown: true },
        OutlineChunk { id: 3, text: "## Text on page 3 (OCR)\nScanned words", markdown: false },
        OutlineChunk { id: 4, text: "## Usage\nUse it.", markdown: true },
    ];
    let o = Outline::build(&chunks);
    assert_eq!(o.path(2).as_deref(), Some("Handbook › Setup › Install"));
    assert_eq!(o.path(3).as_deref(), Some("Handbook › Setup › Install"), "OCR heading is no heading");
    assert_eq!(o.path(4).as_deref(), Some("Handbook › Usage"));
    assert_eq!(o.section(1).unwrap().chunk_ids, vec![1, 2, 3, 4]);
}

#[test]
fn plain_prose_has_no_path() {
    let o = outline(&["Die Prüfung ergab Mängel.\nDer Hof empfiehlt Folgendes.", "Weitere Ausführungen."]);
    assert_eq!(o.path(1), None);
    assert_eq!(o.section(2), None);
}

#[test]
fn group_words_rank_title_above_chapter_and_subsection_below_section() {
    let o = outline(&[
        "TITEL I\nAllgemeines\nKAPITEL 1\nGrundsätze\nABSCHNITT 1\nZiele\nUNTERABSCHNITT 1\nEinzelnes",
        "Artikel 1\nGegenstand\n(1) Diese Verordnung regelt das.",
        "Artikel 2\nZiel\n(1) Text.\nArtikel 3\nInkrafttreten\n(1) Text.",
    ]);
    assert_eq!(o.path(2).as_deref(), Some("TITEL I › KAPITEL 1 › ABSCHNITT 1 › UNTERABSCHNITT 1 › Artikel 1 – Gegenstand › (1)"));
}

#[test]
fn each_exclusion_alone_keeps_a_numbered_line_from_being_a_heading() {
    let long_words = "Einleitungsabschnittsüberschriftsverlängerung Beispielfallsammlung Sonderregelungsverzeichnisse Anhangtabellen";
    for line in [
        "2 Ergebnisse".to_string(),
        format!("2. {long_words}"),
        "2. einleitung folgt".into(),
        "2. Ergebnisse folgen:".into(),
        "2. Ergebnisse [Entwurf]".into(),
        "2. Ergebnisse. Weiter geht es".into(),
        "2. Eins Zwei Drei Vier Fünf Sechs Sieben Acht Neun Zehn Elf Zwölf Dreizehn".into(),
    ] {
        let o = outline(&["1. Einleitung\nText eins.", &format!("{line}\nText zwei.")]);
        assert_eq!(o.path(2).as_deref(), Some("1. Einleitung"), "{line}");
    }
    assert!(format!("2. {long_words}").chars().count() > 100);
}

#[test]
fn paragraphs_count_one_by_one_in_one_style() {
    let rest = ["Artikel 2\nZiel\n(1) Text.", "Artikel 3\nEnde\n(1) Text."];
    for (body, want) in [("(1) Eins.\n(3) Drei.", "(1)"), ("(1) Eins.\n2. Zwei.", "(1)"), ("(1) Eins.\n(2) Zwei.", "(1)–(2)")] {
        let first = format!("Artikel 1\nTitel\n{body}");
        let o = outline(&[&first, rest[0], rest[1]]);
        assert_eq!(o.path(1), Some(format!("Artikel 1 – Titel › {want}")), "{body}");
    }
}

/// Found while closing the mutation gaps: "3.1" continues "2.2" one level up, but it stayed
/// under "2." (same depth as "2.2"), so its path read "2. Zwei › 3.1 Weiter" and section 2 ran on
/// into chapter 3.
#[test]
fn regression_rule_outline_3_1_after_2_2_leaves_section_2() {
    let chunks = |last: &str| vec!["2. Zwei\nText.".to_string(), "2.1 Eins\nText.".into(), "2.2 Zwei\nText.".into(), format!("{last}\nText.")];
    let o = outline(&chunks("3.1 Weiter").iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(o.path(4).as_deref(), Some("3.1 Weiter"));
    let o = outline(&chunks("3.2 Falsch").iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(o.path(4).as_deref(), Some("2. Zwei › 2.2 Zwei"));
}

#[test]
fn a_line_after_an_article_is_its_title_only_when_it_reads_like_one() {
    let rest = ["Artikel 2\nZiel\n(1) Text.", "Artikel 3\nEnde\n(1) Text."];
    for (first, want) in [("Artikel 1\n(1) Der Text gilt.", "Artikel 1 › (1)"), ("Artikel 1\na) erster Punkt", "Artikel 1")] {
        let o = outline(&[first, rest[0], rest[1]]);
        assert_eq!(o.path(1).as_deref(), Some(want), "{first}");
    }
}
