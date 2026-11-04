//! Tests for `pdf_lines.rs`: lines follow the reading direction of the text,
//! also on pages printed sideways.

use super::pdf_lines::{page_lines, Glyph};

/// A word laid out from (x, y) in direction `rotation` (degrees, counter-clockwise in PDF space, y up).
fn word(text: &str, x: f32, y: f32, rotation: f32) -> Vec<Glyph> {
    let (s, c) = rotation.to_radians().sin_cos();
    text.chars()
        .enumerate()
        .map(|(i, ch)| {
            let d = i as f32 * 6.0;
            let (ox, oy) = (x + d * c, y + d * s);
            Glyph {
                ch,
                rotation_degrees: rotation,
                bbox_x: ox,
                bbox_y: oy,
                // Like pdf_oxide: upright glyphs have a width, rotated ones none.
                bbox_width: if rotation == 0.0 { 5.5 } else { 0.0 },
                origin_x: ox,
                origin_y: oy,
                font_size: 12.0,
                bold: false,
            }
        })
        .collect()
}

fn texts(glyphs: &[Glyph]) -> Vec<String> {
    page_lines(glyphs).iter().map(|l| l.glyphs.iter().map(|g| g.ch).collect()).collect()
}

#[test]
fn upright_lines_run_top_to_bottom_left_to_right() {
    let mut g = word("second", 72.0, 680.0, 0.0);
    g.extend(word("first", 72.0, 700.0, 0.0));
    assert_eq!(texts(&g), ["first", "second"]);
}

#[test]
fn text_turned_by_90_degrees_stays_one_line_per_line() {
    // Landscape transcript as in the NTSB report: reading bottom to top, next line further right.
    let mut g = word("Report", 40.4, 600.0, 90.0);
    g.extend(word("CAPT", 83.6, 600.0, 90.0));
    assert_eq!(texts(&g), ["Report", "CAPT"]);
}

#[test]
fn text_turned_by_270_degrees_reads_top_to_bottom() {
    let mut g = word("down", 500.0, 700.0, 270.0);
    g.extend(word("next", 480.0, 700.0, 270.0));
    assert_eq!(texts(&g), ["down", "next"]);
}

#[test]
fn rotated_glyphs_in_shuffled_order_are_sorted_along_the_line() {
    let mut g = word("cockpit", 40.0, 300.0, 90.0);
    g.reverse();
    assert_eq!(texts(&g), ["cockpit"]);
}

#[test]
fn upright_page_number_on_a_sideways_page_is_its_own_line() {
    let mut g = word("Report", 40.4, 600.0, 90.0);
    g.extend(word("172", 300.0, 30.0, 0.0));
    assert_eq!(texts(&g), ["172", "Report"]);
}

#[test]
fn wide_gap_on_a_rotated_line_is_kept_measurable() {
    let mut g = word("TIME", 40.0, 100.0, 90.0);
    g.extend(word("SOURCE", 40.0, 200.0, 90.0));
    let lines = page_lines(&g);
    assert_eq!(lines.len(), 1);
    let l = &lines[0];
    let gap = l.glyphs[4].along - l.glyphs[3].end;
    assert!(gap > 12.0 * 3.0, "gap {gap}");
}
