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
    PageInput { rgba, width: w, height: h, page_w: w as f32, page_h: h as f32, chars, paths, images }
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
    ChunkSpan { chunk_id: id, char_start: start, char_end: end, page }
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
