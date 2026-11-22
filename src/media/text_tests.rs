//! `text.rs`: clock format, headings per kind, body ranges that are exact slices, times only in headings.

use super::*;

fn sec(kind: SegmentKind, t0: i64, t1: i64, tags: &[&str], body: &str) -> Section {
    Section { kind, t0_ms: t0, t1_ms: t1, tags: tags.iter().map(|t| t.to_string()).collect(), body: body.into(), generated: false }
}

#[test]
fn clock_switches_to_hours_for_long_files() {
    assert_eq!(clock(750_999, false), "12:30");
    assert_eq!(clock(5_000, false), "0:05");
    assert_eq!(clock(3_725_000, true), "1:02:05");
    assert_eq!(clock(5_000, true), "0:00:05");
    assert_eq!(clock(-1, false), "0:00");
}

#[test]
fn headings_name_kind_time_and_who() {
    let long = false;
    assert_eq!(heading(&sec(SegmentKind::Speech, 750_000, 842_000, &["Speaker 2"], "x"), long), "12:30–14:02 · Speaker 2");
    assert_eq!(heading(&sec(SegmentKind::Speech, 0, 1_000, &[], "x"), long), "0:00–0:01");
    assert_eq!(heading(&sec(SegmentKind::Subtitle, 0, 42_000, &["Subtitles (en)"], "x"), long), "0:00–0:42 · Subtitles (en)");
    assert_eq!(heading(&sec(SegmentKind::Slide, 748_000, 820_000, &[], "x"), long), "On screen 12:28–13:40 (slide, OCR)");
    assert_eq!(heading(&sec(SegmentKind::Scene, 190_000, 225_000, &[], "x"), long), "Scene 3:10–3:45 (AI description)");
    assert_eq!(heading(&sec(SegmentKind::Sound, 300_000, 320_000, &[], "applause,\n cheering"), long), "Sounds 5:00–5:20: applause, cheering (AI)");
}

#[test]
fn bodies_are_exact_slices_and_carry_no_times() {
    let sections = [
        sec(SegmentKind::Slide, 0, 9_000, &[], "Agenda\n1. Ziele"),
        sec(SegmentKind::Speech, 1_000, 9_000, &["Anna", "Ben"], "  Anna: Guten Morgen.\nBen: Hallo.  "),
    ];
    let r = render("Vortrag\n2026.mp4", 3_700_000, &sections);
    assert!(r.text.starts_with("# Vortrag 2026.mp4\n\n### On screen 0:00:00–0:00:09 (slide, OCR)\nAgenda\n1. Ziele\n"), "{}", r.text);
    assert!(r.text.contains("\n## 0:00:01–0:00:09 · Anna, Ben\nAnna: Guten Morgen.\nBen: Hallo.\n"), "{}", r.text);
    assert_eq!(&r.text[r.placed[0].start..r.placed[0].end], "Agenda\n1. Ziele");
    assert_eq!(&r.text[r.placed[1].start..r.placed[1].end], "Anna: Guten Morgen.\nBen: Hallo.");
    assert_eq!(r.placed[1].heading, "0:00:01–0:00:09 · Anna, Ben");
    for p in &r.placed {
        assert!(!r.text[p.start..p.end].contains("0:0"), "no time in a passage");
    }
}
