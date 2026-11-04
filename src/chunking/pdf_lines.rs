//! Groups the characters of one PDF page into text lines, following each
//! character's own reading direction (`rotation_degrees` from `pdf_oxide`).
//! Landscape tables and transcripts printed sideways (e.g. a cockpit voice
//! recorder transcript turned by 90°) share an x coordinate per line, not a y:
//! grouped by y alone, every character became a line of its own and the chunk
//! text a column-wise letter salad. Each character is therefore turned back by
//! its rotation, rounded to a quarter turn, before grouping: `along` runs in
//! reading direction, `across` grows from one line to the next. Unrotated text
//! keeps exactly the old frame (`bbox.x`, `bbox.y`, `bbox.width`), so ordinary
//! pages are cut as before.

use pdf_oxide::layout::TextChar;
use std::collections::BTreeMap;

/// One character as the line builder needs it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Glyph {
    pub ch: char,
    pub rotation_degrees: f32,
    pub bbox_x: f32,
    pub bbox_y: f32,
    pub bbox_width: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub font_size: f32,
    pub bold: bool,
}

impl Glyph {
    pub(crate) fn from_text_char(c: &TextChar) -> Self {
        Glyph {
            ch: c.char,
            rotation_degrees: c.rotation_degrees,
            bbox_x: c.bbox.x,
            bbox_y: c.bbox.y,
            bbox_width: c.bbox.width,
            origin_x: c.origin_x,
            origin_y: c.origin_y,
            font_size: c.font_size,
            bold: c.font_name.to_lowercase().contains("bold"),
        }
    }
}

/// A character placed in the frame of its line.
#[derive(Debug, Clone)]
pub(crate) struct PlacedGlyph {
    pub ch: char,
    /// Start and end in reading direction (points).
    pub along: f32,
    pub end: f32,
    pub font_size: f32,
    pub bold: bool,
}

/// One text line: its characters in reading order.
#[derive(Debug, Clone)]
pub(crate) struct PageLine {
    /// Quarter turns of the text (0 = upright, 1 = 90° …).
    pub quarter: u8,
    /// Position across the lines in tenths of a point; grows line by line.
    pub across: i32,
    pub glyphs: Vec<PlacedGlyph>,
}

/// Lines of a page in reading order: upright text first, then each rotation
/// in turn; within one rotation from the first line to the last.
pub(crate) fn page_lines(glyphs: &[Glyph]) -> Vec<PageLine> {
    let mut by_line: BTreeMap<(u8, i32), Vec<PlacedGlyph>> = BTreeMap::new();
    for g in glyphs {
        let quarter = ((g.rotation_degrees / 90.0).round() as i32).rem_euclid(4) as u8;
        let (along, end, across) = if quarter == 0 {
            (g.bbox_x, g.bbox_x + g.bbox_width, -g.bbox_y)
        } else {
            let (s, c) = (f32::from(quarter) * std::f32::consts::FRAC_PI_2).sin_cos();
            let along = g.origin_x * c + g.origin_y * s;
            // Rotated glyphs carry no advance width; half the font size is a usual character width.
            (along, along + g.font_size * 0.5, g.origin_x * s - g.origin_y * c)
        };
        by_line
            .entry((quarter, (across * 10.0).round() as i32))
            .or_default()
            .push(PlacedGlyph { ch: g.ch, along, end, font_size: g.font_size, bold: g.bold });
    }
    by_line
        .into_iter()
        .map(|((quarter, across), mut glyphs)| {
            glyphs.sort_by(|a, b| a.along.total_cmp(&b.along));
            PageLine { quarter, across, glyphs }
        })
        .collect()
}

/// Metadata key of a chunk cut from lines built by this module, and the version of
/// that line building. A PDF chunk without it was cut before sideways text was
/// followed in reading direction (`db/page_refresh.rs` looks for letter salad only there).
pub const LINES_KEY: &str = "line_rev";
pub const LINES_REV: u32 = 1;

/// Whether `text` is the letter salad the old line building made of sideways text:
/// at least [`SALAD_MIN_LINES`] lines, nearly all of them one or two characters, and
/// mostly letters. Chart axes (digits, short ticks among longer lines) stay below this.
pub fn looks_like_letter_salad(text: &str) -> bool {
    let (mut lines, mut short, mut letters) = (0usize, 0usize, 0usize);
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        lines += 1;
        let n = line.chars().count();
        if n <= 2 {
            short += 1;
            if line.chars().all(char::is_alphabetic) {
                letters += 1;
            }
        }
    }
    lines >= SALAD_MIN_LINES && short * 10 >= lines * 9 && letters * 10 >= short * 8
}

const SALAD_MIN_LINES: usize = 40;
