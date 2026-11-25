//! Tests for `page_select.rs`: the graphics-page rule on synthetic pages, the
//! page-offset carry-over through the text repair, and page → chunk mapping.

use super::page_select::*;

fn white(w: usize, h: usize) -> Vec<u8> {
    vec![255u8; w * h * 4]
}

fn fill(buf: &mut [u8], w: usize, x0: usize, x1: usize, y0: usize, y1: usize, rgb: [u8; 3]) {
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) * 4;
            buf[i..i + 3].copy_from_slice(&rgb);
        }
    }
}

fn input<'a>(rgba: &'a [u8], w: usize, h: usize, chars: &'a [BoxPt], paths: &'a [PathInfo], images: &'a [BoxPt]) -> PageInput<'a> {
    // 1 px = 1 pt keeps the coordinates readable.
    PageInput { rgba, width: w, height: h, page_w: w as f32, page_h: h as f32, chars, paths, images, glyphs_as_boxes: false }
}

#[test]
fn blank_page_is_no_graphics_page() {
    let buf = white(100, 100);
    let s = page_stats(&input(&buf, 100, 100, &[], &[], &[]));
    assert_eq!(s.solid, 0.0);
    assert!(!s.is_graphics_page());
}

#[test]
fn solid_bar_outside_text_makes_a_graphics_page_but_not_inside_text() {
    let mut buf = white(100, 100);
    // A 10×10 dark bar: 8×8 interior pixels = 0.0064 ≥ 0.0005.
    fill(&mut buf, 100, 40, 50, 40, 50, [20, 20, 20]);
    let s = page_stats(&input(&buf, 100, 100, &[], &[], &[]));
    assert!((s.solid - 0.0064).abs() < 1e-9, "{}", s.solid);
    assert!(s.is_graphics_page());
    // The same bar under a character box (PDF coords: y from the bottom) is text.
    let chars = [BoxPt { x: 40.0, y: 50.0, width: 10.0, height: 10.0 }];
    let s = page_stats(&input(&buf, 100, 100, &chars, &[], &[]));
    assert_eq!(s.solid, 0.0);
    assert!(!s.is_graphics_page());
}

#[test]
fn without_system_fonts_solid_is_not_measured_but_curves_still_count() {
    let mut buf = white(100, 100);
    // Glyph boxes drawn past the character boxes look like a solid bar.
    fill(&mut buf, 100, 40, 50, 40, 50, [20, 20, 20]);
    let boxes = PageInput { glyphs_as_boxes: true, ..input(&buf, 100, 100, &[], &[], &[]) };
    let s = page_stats(&boxes);
    assert_eq!(s.solid, 0.0);
    assert!(!s.is_graphics_page());
    let pie = [PathInfo { bbox: BoxPt { x: 10.0, y: 10.0, width: 30.0, height: 30.0 }, ops: vec![PathOp::CurveTo; 24], dark_fill: true }];
    assert!(page_stats(&PageInput { glyphs_as_boxes: true, ..input(&buf, 100, 100, &[], &pie, &[]) }).is_graphics_page());
}

#[test]
fn thin_rule_erodes_away() {
    let mut buf = white(100, 100);
    fill(&mut buf, 100, 10, 90, 50, 52, [0, 0, 0]); // 2 px high: no full 3×3 block
    let s = page_stats(&input(&buf, 100, 100, &[], &[], &[]));
    assert_eq!(s.solid, 0.0);
}

#[test]
fn diagonals_curves_and_images_trigger_the_rule() {
    let buf = white(100, 100);
    let b = BoxPt { x: 10.0, y: 10.0, width: 30.0, height: 30.0 };
    let zigzag: Vec<PathOp> = std::iter::once(PathOp::MoveTo(10.0, 10.0))
        .chain((1..=10).map(|i| PathOp::LineTo(10.0 + i as f32, 10.0 + (i % 2) as f32 * 5.0)))
        .collect();
    let paths = [PathInfo { bbox: b, ops: zigzag, dark_fill: false }];
    let s = page_stats(&input(&buf, 100, 100, &[], &paths, &[]));
    assert_eq!(s.diag, 10);
    assert!(s.is_graphics_page());

    let curves = [PathInfo { bbox: b, ops: vec![PathOp::CurveTo; 20], dark_fill: false }];
    assert!(page_stats(&input(&buf, 100, 100, &[], &curves, &[])).is_graphics_page());
    let few = [PathInfo { bbox: b, ops: vec![PathOp::CurveTo; 19], dark_fill: false }];
    assert!(!page_stats(&input(&buf, 100, 100, &[], &few, &[])).is_graphics_page());

    // A path whose box covers ≥ 90 % of the page (background) is ignored.
    let bg = [PathInfo { bbox: BoxPt { x: 0.0, y: 0.0, width: 100.0, height: 100.0 }, ops: vec![PathOp::CurveTo; 50], dark_fill: true }];
    assert_eq!(page_stats(&input(&buf, 100, 100, &[], &bg, &[])).curves, 0);

    let img = [BoxPt { x: 0.0, y: 0.0, width: 50.0, height: 30.0 }];
    let s = page_stats(&input(&buf, 100, 100, &[], &[], &img));
    assert!((s.img_area - 0.15).abs() < 1e-9);
    assert!(s.is_graphics_page());
}

#[test]
fn axis_parallel_lines_and_rectangles_are_not_diagonal() {
    let ops = [
        PathOp::MoveTo(0.0, 0.0),
        PathOp::LineTo(10.0, 0.0),
        PathOp::LineTo(10.0, 0.2),
        PathOp::Other,
        PathOp::CurveTo,
        PathOp::LineTo(20.0, 20.0),
    ];
    assert_eq!(count_curves_and_diagonals(&ops), (1, 1));
}

#[test]
fn degenerate_inputs_do_not_panic() {
    let s = page_stats(&input(&[], 0, 0, &[], &[], &[]));
    assert!(!s.is_graphics_page());
    let buf = white(10, 10);
    let far = [BoxPt { x: 500.0, y: -50.0, width: 5.0, height: 5.0 }];
    let paths = [PathInfo { bbox: far[0], ops: vec![PathOp::CurveTo], dark_fill: false }];
    page_stats(&input(&buf, 10, 10, &far, &paths, &far));
}

fn ingest_repair(text: &str) -> String {
    text.split('\n').map(crate::chunking::text_repair::repair_mojibake).collect::<Vec<_>>().join("\n")
}

#[test]
fn repair_is_byte_identical_to_the_ingest_and_keeps_page_starts() {
    let p1 = "Seite eins\nâ€‰Die Anstalt\n";
    let p2 = "durchÂ\u{AD}zuführen\n\n";
    let p3 = "Ende";
    let raw = format!("{p1}{p2}{p3}");
    let starts = [0, p1.len(), p1.len() + p2.len()];
    let (rep, mapped) = repair_with_page_starts(&raw, &starts);
    assert_eq!(rep, ingest_repair(&raw));
    assert_eq!(mapped[0], 0);
    assert!(rep[mapped[1]..].starts_with("durchzuführen"));
    assert!(rep[mapped[2]..].starts_with('\n') || rep[mapped[2]..].starts_with("Ende"));
    assert_eq!(&rep[mapped[2]..], &ingest_repair(&format!("{p2}{p3}"))[ingest_repair(p2).len()..]);
}

#[test]
fn page_start_inside_a_line_rounds_to_the_line_start() {
    let raw = "abc def\nghi";
    let (rep, starts) = repair_with_page_starts(raw, &[0, 4, 7, 8, 99]);
    assert_eq!(rep, raw);
    // 4 is inside "abc def" → its start; 7 is the newline → line end; 8 exact.
    assert_eq!(starts, vec![0, 0, 7, 8, 11]);
}

#[test]
fn page_ranges_cover_the_text() {
    assert_eq!(page_ranges(&[0, 5, 5, 9], 12), vec![(0, 5), (5, 5), (5, 9), (9, 12)]);
    assert_eq!(page_ranges(&[], 3), Vec::<(usize, usize)>::new());
}

fn span(id: i64, start: i64, end: i64, page: Option<i64>) -> ChunkSpan {
    ChunkSpan { chunk_id: id, char_start: start, char_end: end, page, last_page: None }
}

#[test]
fn structural_chunks_cover_start_page_to_next_start_page() {
    let spans = [span(1, 0, 10, Some(1)), span(2, 0, 10, Some(3)), span(3, 0, 10, Some(3)), span(4, 0, 10, Some(6))];
    assert_eq!(chunks_for_page(&spans, 1, None), vec![1]);
    assert_eq!(chunks_for_page(&spans, 2, None), vec![1]);
    // Page 3: chunk 1 may still run onto it; 2 and 3 start there.
    assert_eq!(chunks_for_page(&spans, 3, None), vec![1, 2, 3]);
    assert_eq!(chunks_for_page(&spans, 5, None), vec![3]);
    // The last chunk runs to the end of the document.
    assert_eq!(chunks_for_page(&spans, 9, Some((0, 1))), vec![4]);
}

#[test]
fn a_chunk_reaches_its_own_last_page_even_past_the_next_start() {
    // Chunk 1 runs from page 5 to 8 (page_breaks); chunk 2 (page 6) stands after it.
    let spans = [ChunkSpan { last_page: Some(8), ..span(1, 0, 10, Some(5)) }, span(2, 0, 10, Some(6)), span(3, 0, 10, Some(9))];
    assert_eq!(chunks_for_page(&spans, 7, None), vec![1, 2]);
    assert_eq!(chunks_for_page(&spans, 8, None), vec![1, 2]);
    assert_eq!(chunks_for_page(&spans, 9, None), vec![2, 3]);
    // A recorded last page before the next start changes nothing.
    let spans = [ChunkSpan { last_page: Some(5), ..span(1, 0, 10, Some(5)) }, span(2, 0, 10, Some(7))];
    assert_eq!(chunks_for_page(&spans, 6, None), vec![1]);
}

#[test]
fn chunk_without_page_inherits_its_predecessor() {
    let spans = [span(1, 0, 10, Some(2)), span(2, 0, 10, None), span(3, 0, 10, Some(4))];
    assert_eq!(chunks_for_page(&spans, 3, None), vec![2]);
}

#[test]
fn llm_chunks_overlap_the_page_range() {
    let spans = [span(1, 0, 10, None), span(2, 10, 20, None), span(3, 20, 30, None)];
    assert_eq!(chunks_for_page(&spans, 1, Some((10, 20))), vec![2]);
    assert_eq!(chunks_for_page(&spans, 1, Some((5, 21))), vec![1, 2, 3]);
    assert_eq!(chunks_for_page(&spans, 1, Some((30, 40))), Vec::<i64>::new());
    assert_eq!(chunks_for_page(&spans, 1, None), Vec::<i64>::new());
}

#[test]
fn text_less_page_maps_to_the_chunks_touching_its_position() {
    let spans = vec![span(1, 0, 10, None), span(2, 10, 20, None)];
    // On a chunk boundary: both neighbours are candidates.
    assert_eq!(chunks_for_page(&spans, 1, Some((10, 10))), vec![1, 2]);
    // Inside a chunk: that chunk.
    assert_eq!(chunks_for_page(&spans, 1, Some((15, 15))), vec![2]);
}

// ---- Jede Spalte von page_stats nachgerechnet -------------------------------
// Bisher prüften die Tests von page_stats vor allem `solid` und das Endurteil.
// Die folgenden Seiten sind so gebaut, dass sich jede Spalte von Hand
// nachrechnen lässt; die Rechnung steht jeweils daneben.

/// 40×20 px Raster für eine 80×40-pt-Seite: 0,5 px je pt, damit eine
/// verwechselte Multiplikation/Division in der Umrechnung auffällt.
fn half_scale<'a>(rgba: &'a [u8], chars: &'a [BoxPt], paths: &'a [PathInfo], images: &'a [BoxPt]) -> PageInput<'a> {
    PageInput { rgba, width: 40, height: 20, page_w: 80.0, page_h: 40.0, chars, paths, images, glyphs_as_boxes: false }
}

fn path(x: f32, y: f32, width: f32, height: f32, ops: Vec<PathOp>, dark_fill: bool) -> PathInfo {
    PathInfo { bbox: BoxPt { x, y, width, height }, ops, dark_fill }
}

fn set(buf: &mut [u8], w: usize, x: usize, y: usize, rgb: [u8; 3]) {
    fill(buf, w, x, x + 1, y, y + 1, rgb);
}

#[test]
fn page_stats_measures_every_column_on_a_scaled_page() {
    // Zeichenkasten x 20..28, y 10..18 pt, um 1,5 pt verbreitert, in Pixeln:
    // x0 = ⌊18,5·0,5⌋ = 9, x1 = ⌈29,5·0,5⌉ = 15,
    // y0 = ⌊(40 − 19,5)·0,5⌋ = 10, y1 = ⌈(40 − 8,5)·0,5⌉ = 16 → 6×6 = 36 Textpixel.
    let chars = [BoxPt { x: 20.0, y: 10.0, width: 8.0, height: 8.0 }];
    let mut buf = vec![255u8; 40 * 20 * 4];
    set(&mut buf, 40, 0, 0, [0, 0, 0]); // Tinte (min 0 < 230), keine Farbe
    set(&mut buf, 40, 1, 0, [230, 230, 230]); // min genau 230: keine Tinte
    set(&mut buf, 40, 2, 0, [229, 255, 255]); // Tinte, Spanne 26: keine Farbe
    set(&mut buf, 40, 3, 0, [100, 140, 255]); // Tinte und Farbe (Spanne 155)
    set(&mut buf, 40, 4, 0, [215, 255, 255]); // Tinte, Spanne genau 40: keine Farbe
    set(&mut buf, 40, 5, 0, [0, 0, 255]); // Tinte und Farbe (Spanne 255)
    set(&mut buf, 40, 10, 11, [0, 200, 0]); // im Text: Tinte und Farbe
    set(&mut buf, 40, 11, 11, [0, 0, 0]); // im Text: Tinte
    set(&mut buf, 40, 12, 10, [0, 0, 0]); // erste Textzeile (y0 = 10): Tinte im Text
    set(&mut buf, 40, 12, 15, [0, 0, 0]); // letzte Textzeile (y1 − 1 = 15): Tinte im Text
    set(&mut buf, 40, 9, 12, [0, 0, 0]); // erste Textspalte (x0 = 9): Tinte im Text
    let paths = [
        // Pixel x 12..14, y 12..14 → Mitte (13, 13) liegt im Text: übersprungen.
        path(24.0, 12.0, 4.0, 4.0, vec![PathOp::CurveTo; 3], true),
        // Pixel x 12..20, y 12..14 (16 px), Mitte (16, 13) außerhalb des Texts;
        // 3×2 px davon liegen im Text → 10 px Pfadfläche. 1 Diagonale, 2 Kurven.
        path(24.0, 12.0, 16.0, 4.0, vec![PathOp::MoveTo(24.0, 12.0), PathOp::LineTo(40.0, 16.0), PathOp::CurveTo, PathOp::CurveTo], true),
        // Senkrechte Linie: x0 = x1 = 34, y 2..18 → zählt, deckt eine Spalte = 16 px.
        path(68.0, 4.0, 0.0, 32.0, vec![PathOp::MoveTo(68.0, 4.0), PathOp::LineTo(68.0, 36.0)], false),
        // Waagrechte Linie: y0 = y1 = 15, x 2..8 → zählt, deckt eine Zeile = 6 px.
        path(4.0, 10.0, 12.0, 0.0, vec![PathOp::MoveTo(4.0, 10.0), PathOp::LineTo(16.0, 10.0)], false),
        // Punkt (x0 = x1 = 35, y0 = y1 = 5): ohne Ausdehnung, übersprungen.
        path(70.0, 30.0, 0.0, 0.0, vec![PathOp::CurveTo; 5], true),
    ];
    // Bild x 20..60, y 20..40 pt → Pixel x 10..30, y 0..10 = 200 px.
    let images = [BoxPt { x: 20.0, y: 20.0, width: 40.0, height: 20.0 }];
    let s = page_stats(&half_scale(&buf, &chars, &paths, &images));
    let px = |v: f64| v / 800.0;
    let expected = PageStats {
        ink: px(10.0),       // (0,0) (2,0) (3,0) (4,0) (5,0) + 5 im Text
        ink_free: px(5.0),   // ohne die 5 im Text
        color: px(3.0),      // (3,0) (5,0) und (10,11)
        color_free: px(2.0), // (3,0) und (5,0)
        solid: 0.0,          // nur Einzelpixel, kein 3×3-Block
        paths: 3,
        curves: 2,
        fills: 1,
        boxes: 0, // keiner ist in beiden Richtungen ≥ 6 pt
        diag: 1,
        path_area: px(10.0 + 16.0 + 6.0),
        img_area: px(200.0),
        chars: 1,
    };
    assert_eq!(s, expected);
}

#[test]
fn solid_counts_exactly_the_strong_three_by_three_blocks() {
    // Je ein 3×3-Block zählt mit seinem Mittelpunkt, wenn alle 9 Pixel „stark“ sind:
    // Helligkeit (299 r + 587 g + 114 b) / 1000 < 200 oder Sättigung max − min > 60.
    let mut buf = white(40, 20);
    let blocks: [(usize, usize, [u8; 3]); 6] = [
        (0, 0, [20, 20, 20]),     // in der Ecke, dunkel: zählt (Mitte (1,1))
        (5, 5, [199, 199, 199]),  // Helligkeit 199: zählt
        (10, 5, [200, 200, 200]), // Helligkeit genau 200: zählt nicht
        (15, 5, [255, 255, 195]), // Sättigung genau 60 (Helligkeit 248): zählt nicht
        (20, 5, [255, 255, 0]),   // Sättigung 255, Helligkeit 225: zählt
        (25, 5, [255, 255, 100]), // Sättigung 155, Helligkeit 237: zählt
    ];
    for (x, y, rgb) in blocks {
        fill(&mut buf, 40, x, x + 3, y, y + 3, rgb);
    }
    let s = page_stats(&input(&buf, 40, 20, &[], &[], &[]));
    assert_eq!(s.solid, 4.0 / 800.0);
}

#[test]
fn degenerate_renders_report_only_the_character_count() {
    let chars = [BoxPt { x: 1.0, y: 1.0, width: 2.0, height: 2.0 }; 3];
    let only_chars = PageStats { chars: 3, ..PageStats::default() };
    // Breite 0, Höhe 0, oder ein Puffer, dem ein Byte fehlt: keine Messung.
    assert_eq!(page_stats(&input(&[], 0, 5, &chars, &[], &[])), only_chars);
    assert_eq!(page_stats(&input(&[], 5, 0, &chars, &[], &[])), only_chars);
    let short = vec![0u8; 10 * 10 * 4 - 1];
    assert_eq!(page_stats(&input(&short, 10, 10, &chars, &[], &[])), only_chars);
    // Ein längerer Puffer wird gemessen (nur die ersten w·h·4 Bytes).
    let long = vec![0u8; 10 * 10 * 4 + 4];
    assert_eq!(page_stats(&input(&long, 10, 10, &[], &[], &[])).ink, 1.0);
}

#[test]
fn paths_at_the_page_edge_take_their_centre_inside_the_page() {
    let buf = white(40, 20);
    let paths = [
        // Senkrechte Linie am rechten Rand: x0 = x1 = 40, y 18..20 → Mitte
        // x = 40 wird auf 39 begrenzt; zählt (Fläche 0, keine Spalte im Bild).
        path(40.0, 0.0, 0.0, 2.0, vec![PathOp::CurveTo], false),
        // Unter dem unteren Rand: y0 = 22, y1 = 20 → Mitte y = 21 wird auf 19
        // begrenzt. Die Fläche rechnet mit Überlauf ((14−10)·(20−22) ≫ 90 %),
        // also gilt der Pfad wie ein Seitenhintergrund und zählt nicht.
        path(10.0, -2.0, 4.0, 0.0, vec![PathOp::CurveTo], false),
    ];
    let s = page_stats(&input(&buf, 40, 20, &[], &paths, &[]));
    assert_eq!((s.paths, s.curves, s.path_area), (1, 1, 0.0));
}

#[test]
fn background_limit_is_more_than_ninety_percent() {
    // 40×18 px = 720 px = genau 90 % von 800: noch eine Zeichnung.
    let buf = white(40, 20);
    let at_limit = [path(0.0, 0.0, 40.0, 18.0, vec![PathOp::CurveTo], false)];
    assert_eq!(page_stats(&input(&buf, 40, 20, &[], &at_limit, &[])).paths, 1);
}

#[test]
fn boxes_need_six_points_each_way_and_less_than_a_third_of_the_page_width() {
    // 60×60-pt-Seite, 1 px je pt: ein Drittel der Breite = 20 pt.
    let buf = white(60, 60);
    let paths = [
        path(2.0, 2.0, 6.0, 6.0, vec![], false),    // 6×6: Kasten
        path(2.0, 20.0, 5.0, 10.0, vec![], false),  // zu schmal
        path(2.0, 40.0, 10.0, 5.0, vec![], false),  // zu flach
        path(15.0, 2.0, 19.0, 10.0, vec![], true),  // 19 < 20: Kasten
        path(15.0, 20.0, 20.0, 10.0, vec![], true), // genau ein Drittel: kein Kasten
        path(15.0, 40.0, 40.0, 10.0, vec![], false), // breiter: kein Kasten
    ];
    let s = page_stats(&input(&buf, 60, 60, &[], &paths, &[]));
    assert_eq!((s.paths, s.boxes, s.fills), (6, 2, 2));
}

#[test]
fn diagonal_needs_more_than_0_3_pt_in_each_direction_from_the_last_point() {
    let ops = [
        // genau 0,3 pt in einer Richtung (und weit in der anderen): noch keine Diagonale
        PathOp::MoveTo(0.0, 0.0),
        PathOp::LineTo(0.3, 5.0),
        PathOp::MoveTo(0.0, 0.0),
        PathOp::LineTo(5.0, 0.3),
        // senkrecht ab (10, 10): dx = 0 (nicht x + letzter Punkt, nicht x / letzter Punkt)
        PathOp::MoveTo(10.0, 10.0),
        PathOp::LineTo(10.0, 20.0),
    ];
    assert_eq!(count_curves_and_diagonals(&ops), (0, 0));
}
