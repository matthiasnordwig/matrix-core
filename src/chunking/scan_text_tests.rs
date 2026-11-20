//! Tests for `scan_text.rs`: scanned pages as one continuous text (cleaning, joining, headings
//! from the row boxes, page starts, chunk view, run rule, marker line).

use super::scan_text::*;

fn pl(text: &str, gap: bool) -> PageLine {
    PageLine { text: text.to_string(), gap, indent: false, heading: false }
}

fn row(text: &str, left: f64, top: f64, width: f64, height: f64) -> ScanRow {
    ScanRow { text: text.into(), left, right: left + width, top, bottom: top + height, height, ..Default::default() }
}

#[test]
fn page_numbers_are_recognised_in_their_usual_forms() {
    for t in ["16", "— 16 —", "[3]", "xii", "- 7 -"] {
        assert!(is_page_number(t), "{t}");
    }
    for t in ["Seite 3 von 9", "und", "16 Uhr", "Im", "12345", "—"] {
        assert!(!is_page_number(t), "{t}");
    }
}

#[test]
fn running_heads_go_page_numbers_go_text_stays() {
    let w = ["Anna", "Berta", "Carl", "Dora", "Emil", "Fritz", "Gustav", "Heinrich"];
    let mut pages: Vec<(i64, Vec<PageLine>)> = (0..8)
        .map(|p| (p as i64, vec![pl(&format!("Drucksache 7/{p}12"), false), pl(&format!("Text {}", w[p]), true), pl(&format!("weiter {}.", w[p]), false), pl(&p.to_string(), true)]))
        .collect();
    let r = clean_pages(&mut pages);
    assert_eq!((r.page_numbers, r.running_lines), (8, 8));
    assert!(pages.iter().enumerate().all(|(p, (_, l))| l.len() == 2 && l[0].text == format!("Text {}", w[p]) && !l[0].gap));
}

#[test]
fn a_line_on_few_pages_is_no_running_line() {
    let w = ["Anna", "Berta", "Carl", "Dora", "Emil", "Fritz", "Gustav", "Heinrich", "Ida", "Julius", "Karl", "Ludwig"];
    let mut pages: Vec<(i64, Vec<PageLine>)> =
        (0..12).map(|p| (p as i64, vec![pl(if p < 2 { "Kapitel" } else { "anders" }, true), pl(&format!("Satz {}.", w[p]), false)])).collect();
    let r = clean_pages(&mut pages);
    assert_eq!(r.running_lines, 10, "only the 10× repeated edge line goes");
    assert_eq!(pages[0].1[0].text, "Kapitel");
}

#[test]
fn hyphens_at_line_ends_are_joined_by_the_next_word() {
    assert_eq!(join_kind("die Kündi-", "gung des Vertrags"), Join::Dehyphen);
    assert_eq!(join_kind("neuer=", "dings empor"), Join::Dehyphen);
    assert_eq!(join_kind("Ein-", "und Ausgang"), Join::Space);
    assert_eq!(join_kind("Nord-", "Amerika"), Join::Glue);
    assert_eq!(join_kind("Seite 3 -", "weiter"), Join::Space);
    assert_eq!(join_kind("ein Satz.", "Der nächste"), Join::Space);
}

#[test]
fn a_paragraph_runs_over_the_page_break_and_keeps_its_pages() {
    let pages = vec![
        (3, vec![pl("Erster Absatz endet.", true), pl("Der zweite beginnt mit der Kündi-", true)]),
        (4, vec![pl("gung und geht weiter.", false), pl("Neuer Absatz.", true)]),
    ];
    let a = assemble(&pages);
    assert_eq!(chunk_view(&a.stored), "Erster Absatz endet.\n\nDer zweite beginnt mit der Kündigung und geht weiter.\n\nNeuer Absatz.");
    assert!(a.stored.contains("Kündigung\nund geht"), "the page break is the only single newline");
    assert_eq!((a.dehyphen, a.dehyphen_over_page, a.paragraphs, a.breaks_in_paragraph), (1, 1, 3, 1));
    let at = a.stored.find("und geht").unwrap();
    assert_eq!(a.page_starts, vec![(0, 3), (at, 4)]);
    let pages = pages_of_range(&a.page_starts, 22, a.stored.len());
    assert_eq!(pages.range(), (3, 4));
    assert_eq!(pages.page_at(at - 22), Some(4));
}

#[test]
fn a_new_page_that_starts_a_paragraph_keeps_the_blank_line() {
    let a = assemble(&[(1, vec![pl("Ende.", true)]), (2, vec![pl("Anfang.", true)])]);
    assert_eq!(a.stored, "Ende.\n\nAnfang.");
    assert_eq!(a.page_starts, vec![(0, 1), (7, 2)]);
}

#[test]
fn every_page_start_is_a_line_start_also_when_a_word_is_glued_over_the_break() {
    // "Nord-" + "Amerika" glued, the new page's line has no space: the break goes to the last
    // space before it, so page 2 starts at "Nord-Amerika".
    let a = assemble(&[(1, vec![pl("Er reiste nach Nord-", true)]), (2, vec![pl("Amerika", false), pl("Neu.", true)])]);
    assert_eq!(a.stored, "Er reiste nach\nNord-Amerika\n\nNeu.");
    assert_eq!(a.page_starts, vec![(0, 1), (15, 2)]);
    for (o, _) in &a.page_starts {
        assert!(*o == 0 || a.stored.as_bytes()[o - 1] == b'\n', "offset {o}");
    }
    assert_eq!(chunk_view(&a.stored), "Er reiste nach Nord-Amerika\n\nNeu.");
}

#[test]
fn chunk_view_keeps_the_length_and_only_lone_newlines_change() {
    for s in ["a\nb\n\nc\nd", "\nx\n", "## Kopf\n\nText über\nzwei Seiten.\n\n\nRest", "", "ä\nö"] {
        let v = chunk_view(s);
        assert_eq!(v.len(), s.len(), "{s:?}");
        assert_eq!(v.matches("\n\n").count(), s.matches("\n\n").count(), "{s:?}");
    }
    assert_eq!(chunk_view("a\nb\n\nc\nd"), "a b\n\nc d");
    assert_eq!(chunk_view("x\n\n\ny"), "x\n\n\ny");
}

#[test]
fn pages_of_range_relative_to_the_chunk() {
    let starts = [(0, 5), (40, 6), (90, 7)];
    let p = pages_of_range(&starts, 50, 120);
    assert_eq!((p.range(), p.page_at(0), p.page_at(40)), ((6, 7), Some(6), Some(7)));
    let p = pages_of_range(&starts, 0, 40);
    assert_eq!(p.range(), (5, 5), "a start at the range end belongs to the next chunk");
}

#[test]
fn outline_numbers_start_headings_only_on_short_lines() {
    for t in ["§ 5 Kündigung", "3.2 Ziele", "II. Teil", "Artikel 12", "4. Ergebnisse"] {
        assert!(outline_start(t), "{t}");
    }
    for t in ["1913 kam er", "Das Schiff", "§§"] {
        assert!(!outline_start(t), "{t}");
    }
}

#[test]
fn headings_and_indents_from_the_boxes() {
    let h = 0.02;
    let rows = vec![
        row("DER HEIZER", 0.35, 0.05, 0.3, 0.04),                                // tall
        row("Als der sechzehnjährige Karl Roßmann, der von", 0.1, 0.13, 0.8, h), // full line
        row("seinen armen Eltern nach Amerika geschickt", 0.1, 0.155, 0.8, h),
        row("worden war, sah er die Statue.", 0.1, 0.18, 0.5, h),
        row("Ein neuer Absatz beginnt eingerückt und", 0.14, 0.205, 0.76, h), // indent
        row("läuft hier weiter.", 0.1, 0.23, 0.4, h),
        row("Zweiter Teil", 0.4, 0.28, 0.2, h), // short, gaps around
        row("Text danach beginnt hier und ist lang genug.", 0.1, 0.34, 0.8, h),
    ];
    let lines = lines_from_rows(&rows, h);
    let flags: Vec<(bool, bool)> = lines.iter().map(|l| (l.heading, l.para_start())).collect();
    assert_eq!(flags, vec![(true, true), (false, true), (false, false), (false, false), (false, true), (false, false), (true, true), (false, true)]);
    // The body height comes from all pages together.
    let by_page = page_lines(&[(1, rows.clone())]);
    assert_eq!(by_page[0].1, lines);
}

#[test]
fn a_sentence_line_with_gaps_is_no_heading() {
    let h = 0.02;
    let rows = vec![row("Oben.", 0.1, 0.1, 0.8, h), row("Er ging.", 0.1, 0.2, 0.2, h), row("Unten.", 0.1, 0.3, 0.8, h)];
    assert!(!is_heading(&rows, 1, h, 0.8));
    // A body line whose box came out tall, without space above: no heading.
    let rows = vec![row("eine Zeile", 0.1, 0.1, 0.8, h), row("die nächste Zeile ohne Abstand", 0.1, 0.12, 0.8, 1.5 * h)];
    assert!(!is_heading(&rows, 1, h, 0.8));
}

#[test]
fn headings_become_markdown_lines_of_their_own() {
    let mut head = pl("Zweiter Teil", true);
    head.heading = true;
    let a = assemble(&[(1, vec![pl("Ende.", false), head, pl("weiter im Text", false)])]);
    assert_eq!(a.stored, "Ende.\n\n## Zweiter Teil\n\nweiter im Text");
    assert_eq!((a.headings, a.paragraphs), (1, 3));
}

#[test]
fn a_run_breaks_at_text_layer_rest_and_at_column_rows() {
    let plain = vec![row("Fließtext auf der Seite", 0.1, 0.1, 0.8, 0.02)];
    let columns = vec![row("links | rechts", 0.1, 0.1, 0.8, 0.02)];
    assert!(continues_run(&plain, "", &plain));
    assert!(continues_run(&plain, "  17 \n", &plain), "a page number in the text layer is no rest");
    assert!(!continues_run(&plain, "Stempel: Eingang", &plain));
    assert!(!continues_run(&plain, "", &columns));
    assert!(!continues_run(&columns, "", &plain));
}

#[test]
fn marker_lines_old_and_run_form() {
    assert_eq!(scan_marker(5, 5), "## Text on page 5 (OCR)");
    assert_eq!(scan_marker(5, 9), "## Text on pages 5\u{2013}9 (OCR)");
    assert_eq!(parse_scan_marker(&scan_marker(5, 9)), Some((5, 9)));
    assert_eq!(parse_scan_marker("## Text on page 12 (OCR)"), Some((12, 12)));
    assert_eq!(parse_scan_marker("## Text on pages 3-4 (OCR)"), Some((3, 4)));
    assert!(!is_scan_marker("## Text in image (OCR)"));
    assert!(!is_scan_marker("## Text on page 5"));
}

#[test]
fn only_a_block_of_running_text_is_a_text_block() {
    // Letter: 12 rows on one left edge (first lines indented), rows 0.03 high.
    let letter: Vec<ScanRow> = (0..12).map(|i| row("Zeile Text", if i % 4 == 0 { 0.13 } else { 0.1 }, 0.1 + i as f64 * 0.04, 0.7, 0.03)).collect();
    assert!(is_text_block(&letter));
    // Diagram: 12 labels scattered over the picture.
    let lefts = [0.05, 0.4, 0.75, 0.2, 0.6, 0.1, 0.5, 0.85, 0.3, 0.65, 0.15, 0.55];
    let diagram: Vec<ScanRow> = lefts.iter().enumerate().map(|(i, &l)| row("Label", l, 0.05 + i as f64 * 0.07, 0.1, 0.03)).collect();
    assert!(!is_text_block(&diagram));
    // Sign: a few centred words.
    let sign: Vec<ScanRow> = (0..3).map(|i| row("STOP", 0.4, 0.3 + i as f64 * 0.1, 0.2, 0.06)).collect();
    assert!(!is_text_block(&sign));
    assert!(!is_text_block(&[]));
}

#[test]
fn right_to_left_text_has_its_start_edge_on_the_right() {
    // Arabic rows right-aligned at 0.9, first line of each paragraph indented from the right.
    let rows: Vec<ScanRow> = (0..10)
        .map(|i| {
            let right = if i % 5 == 0 { 0.85 } else { 0.9 };
            row("هذا نص عربي طويل", right - 0.7, 0.1 + i as f64 * 0.04, 0.7, 0.03)
        })
        .collect();
    assert!(is_text_block(&rows));
    let lines = lines_from_rows(&rows, 0.03);
    assert!(lines[0].indent && lines[5].indent && !lines[1].indent, "{lines:?}");
    // The same geometry mirrored with Latin letters is left to right: no indent at 0 and 5 from the left.
    let ltr: Vec<ScanRow> = rows.iter().map(|r| ScanRow { text: "Text".into(), ..r.clone() }).collect();
    assert!(!lines_from_rows(&ltr, 0.03)[0].indent);
}

#[test]
fn a_block_start_opens_a_paragraph_without_a_gap_and_a_title_is_a_heading() {
    // Tightly set rows (no gap anywhere): only the engine's block starts and its title separate them.
    let mut rows: Vec<ScanRow> = (0..5).map(|i| row(&format!("Zeile {i}"), 0.1, 0.1 + i as f64 * 0.03, 0.7, 0.03)).collect();
    rows[0].kind = RowKind::Title;
    rows[0].block_start = true; // first row of a page: never a gap above it
    rows[2].block_start = true;
    rows[3].kind = RowKind::ListItem;
    rows[3].block_start = true;
    let lines = lines_from_rows(&rows, 0.03);
    let gaps: Vec<bool> = lines.iter().map(|l| l.gap).collect();
    assert_eq!(gaps, vec![false, false, true, true, false]);
    assert!(lines[0].heading && !lines[1].heading && !lines[3].heading, "{lines:?}");
    let a = assemble(&[(1, lines)]);
    assert_eq!(a.stored, "## Zeile 0\n\nZeile 1\n\nZeile 2\n\nZeile 3 Zeile 4");
}

#[test]
fn without_kind_and_block_start_the_rows_read_as_before() {
    let rows: Vec<ScanRow> = (0..4).map(|i| row(&format!("Zeile {i}"), 0.1, 0.1 + i as f64 * 0.03, 0.7, 0.03)).collect();
    assert!(lines_from_rows(&rows, 0.03).iter().all(|l| !l.gap));
    assert_eq!(ScanRow::default().kind, RowKind::Text);
}
