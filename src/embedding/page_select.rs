//! Image search over graphics pages — the pure, model-free parts (always
//! compiled; the PDF side is `page_render.rs` behind `pdf-render`).
//!
//! 1. **Graphics-page rule** ([`page_stats`] → [`PageStats::is_graphics_page`]):
//!    a page is worth embedding as an image when `solid ≥ 0.0005` OR
//!    `diag ≥ 10` OR `curves ≥ 20` OR `img_area ≥ 0.15`, measured on a 40-dpi
//!    raw RGBA render. `solid` = share of pixels outside the character boxes
//!    (widened by 1.5 pt) whose whole 3×3 neighbourhood is "strong" (luminance
//!    < 200 or saturation max−min > 60); `diag` = line segments with dx and dy
//!    > 0.3 pt, `curves` = Bézier segments — both only for paths whose box
//!    centre lies outside text and that cover < 90 % of the page; `img_area` =
//!    page share of raster-image boxes. Calibrated on the KfW and Uniper
//!    reports (all charts/org charts found except one line-only matrix, no
//!    table page; title/photo pages come along). The remaining columns
//!    (`ink`, `color`, `fills`, …) are only reported by `examples/page_stats.rs`.
//! 2. **Page offsets after text repair** ([`repair_with_page_starts`]): the
//!    ingest repairs `pdf_extract` text line by line; page starts recorded in
//!    the raw text are carried over line-wise (a start inside a line rounds to
//!    that line's start).
//! 3. **Page → chunks** ([`chunks_for_page`]): structural chunks carry their
//!    start page in `metadata.page` — a chunk then covers its start page up to
//!    and including the start page of the next chunk; otherwise chunks whose
//!    byte range overlaps the page's range in `extracted_text` belong to it.

/// A box in PDF user space (points, origin bottom-left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxPt {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The parts of a path operation the rule looks at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathOp {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    CurveTo,
    /// Rectangle, close path, … (ignored; the current point is not moved).
    Other,
}

/// One vector path of a page.
#[derive(Debug, Clone, PartialEq)]
pub struct PathInfo {
    pub bbox: BoxPt,
    pub ops: Vec<PathOp>,
    /// Filled with a non-white colour (min channel < 0.9).
    pub dark_fill: bool,
}

/// Everything the rule needs about one page.
pub struct PageInput<'a> {
    /// Raw RGBA8 render, `width * height * 4` bytes.
    pub rgba: &'a [u8],
    pub width: usize,
    pub height: usize,
    /// Page size in points (media box).
    pub page_w: f32,
    pub page_h: f32,
    pub chars: &'a [BoxPt],
    pub paths: &'a [PathInfo],
    pub images: &'a [BoxPt],
}

/// Per-page measurements; see the module header for the rule columns.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PageStats {
    pub ink: f64,
    pub ink_free: f64,
    pub color: f64,
    pub color_free: f64,
    pub solid: f64,
    pub paths: usize,
    pub curves: usize,
    pub fills: usize,
    pub boxes: usize,
    pub diag: usize,
    pub path_area: f64,
    pub img_area: f64,
    pub chars: usize,
}

pub const SOLID_MIN: f64 = 0.0005;
pub const DIAG_MIN: usize = 10;
pub const CURVES_MIN: usize = 20;
pub const IMG_AREA_MIN: f64 = 0.15;

impl PageStats {
    /// The graphics-page rule (module header, item 1).
    pub fn is_graphics_page(&self) -> bool {
        self.solid >= SOLID_MIN || self.diag >= DIAG_MIN || self.curves >= CURVES_MIN || self.img_area >= IMG_AREA_MIN
    }
}

/// Measure one page. Arithmetic deliberately mirrors the calibration run
/// (including wrapping `usize` math on degenerate boxes), so the thresholds
/// keep their meaning.
pub fn page_stats(p: &PageInput<'_>) -> PageStats {
    let (w, h) = (p.width, p.height);
    if w == 0 || h == 0 || p.rgba.len() < w * h * 4 {
        return PageStats { chars: p.chars.len(), ..PageStats::default() };
    }
    let (pw, ph) = (p.page_w.abs().max(1.0), p.page_h.abs().max(1.0));
    let sx = w as f32 / pw;
    let sy = h as f32 / ph;
    // PDF user space (origin bottom-left) → pixel box, widened by `pad` pt.
    let to_px = |b: &BoxPt, pad: f32| {
        let x0 = ((b.x - pad) * sx).floor().max(0.0) as usize;
        let x1 = (((b.x + b.width + pad) * sx).ceil() as usize).min(w);
        let y0 = ((ph - (b.y + b.height + pad)) * sy).floor().max(0.0) as usize;
        let y1 = (((ph - (b.y - pad)) * sy).ceil().max(0.0) as usize).min(h);
        (x0, x1, y0, y1)
    };
    let mut text = vec![false; w * h];
    for c in p.chars {
        let (x0, x1, y0, y1) = to_px(c, 1.5);
        for yy in y0..y1 {
            for xx in x0..x1 {
                text[yy * w + xx] = true;
            }
        }
    }
    let px = &p.rgba[..w * h * 4];
    let (mut ink, mut ink_free, mut color, mut color_free) = (0usize, 0usize, 0usize, 0usize);
    for (i, q) in px.chunks_exact(4).enumerate() {
        let (r, g, b) = (q[0], q[1], q[2]);
        let (mn, mx) = (r.min(g).min(b), r.max(g).max(b));
        if mn < 230 {
            ink += 1;
            if !text[i] {
                ink_free += 1;
            }
        }
        if mx - mn > 40 {
            color += 1;
            if !text[i] {
                color_free += 1;
            }
        }
    }
    // Solid non-text areas: thin rules and light table shading erode away,
    // bars/wedges/filled boxes stay.
    let strong: Vec<bool> = px
        .chunks_exact(4)
        .enumerate()
        .map(|(i, q)| {
            let (r, g, b) = (q[0] as u32, q[1] as u32, q[2] as u32);
            let lum = (r * 299 + g * 587 + b * 114) / 1000;
            let sat = r.max(g).max(b) - r.min(g).min(b);
            !text[i] && (lum < 200 || sat > 60)
        })
        .collect();
    let mut solid = 0usize;
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            if (0..3).all(|dy| (0..3).all(|dx| strong[(y + dy - 1) * w + x + dx - 1])) {
                solid += 1;
            }
        }
    }
    let mut covered = vec![false; w * h];
    let (mut paths, mut curves, mut fills, mut boxes, mut diag) = (0usize, 0usize, 0usize, 0usize, 0usize);
    for path in p.paths {
        let b = &path.bbox;
        let (x0, x1, y0, y1) = to_px(b, 0.0);
        if x1 <= x0 && y1 <= y0 {
            continue;
        }
        // Inside text = its centre lies in a character box (underlines, bullets).
        let (cx, cy) = (((x0 + x1) / 2).min(w - 1), ((y0 + y1) / 2).min(h - 1));
        if text[cy * w + cx] {
            continue;
        }
        // Full-page background rectangles are not drawings.
        if x1.wrapping_sub(x0).wrapping_mul(y1.wrapping_sub(y0)) > w * h * 9 / 10 {
            continue;
        }
        paths += 1;
        let (c, d) = count_curves_and_diagonals(&path.ops);
        curves += c;
        diag += d;
        if b.width >= 6.0 && b.height >= 6.0 && b.width < pw / 3.0 {
            boxes += 1;
        }
        if path.dark_fill {
            fills += 1;
        }
        for yy in y0..y1.max(y0 + 1).min(h) {
            for xx in x0..x1.max(x0 + 1).min(w) {
                covered[yy * w + xx] = true;
            }
        }
    }
    let mut img_cov = vec![false; w * h];
    for b in p.images {
        let (x0, x1, y0, y1) = to_px(b, 0.0);
        for yy in y0..y1 {
            for xx in x0..x1 {
                img_cov[yy * w + xx] = true;
            }
        }
    }
    let n = (w * h) as f64;
    let frac = |v: usize| v as f64 / n;
    PageStats {
        ink: frac(ink),
        ink_free: frac(ink_free),
        color: frac(color),
        color_free: frac(color_free),
        solid: frac(solid),
        paths,
        curves,
        fills,
        boxes,
        diag,
        path_area: frac(covered.iter().zip(&text).filter(|(c, t)| **c && !**t).count()),
        img_area: frac(img_cov.iter().filter(|c| **c).count()),
        chars: p.chars.len(),
    }
}

/// `(Bézier segments, diagonal line segments)` of one path. A line is diagonal
/// when both dx and dy exceed 0.3 pt; only `MoveTo`/`LineTo` move the point.
pub fn count_curves_and_diagonals(ops: &[PathOp]) -> (usize, usize) {
    let (mut curves, mut diag) = (0usize, 0usize);
    let mut last = (0.0f32, 0.0f32);
    for op in ops {
        match *op {
            PathOp::MoveTo(x, y) => last = (x, y),
            PathOp::LineTo(x, y) => {
                if (x - last.0).abs() > 0.3 && (y - last.1).abs() > 0.3 {
                    diag += 1;
                }
                last = (x, y);
            }
            PathOp::CurveTo => curves += 1,
            PathOp::Other => {}
        }
    }
    (curves, diag)
}

/// Line-wise text repair exactly like the ingest (`split('\n')` →
/// `repair_mojibake` → `join("\n")`), with the raw page starts (byte offsets
/// into `raw`, one per page) carried over into the repaired text. A start at a
/// line's start or end maps exactly; one inside a line rounds to that line's
/// start. Starts beyond the text map to its end; the result is non-decreasing.
pub fn repair_with_page_starts(raw: &str, raw_starts: &[usize]) -> (String, Vec<usize>) {
    // (raw start, raw end, repaired start, repaired end) per line.
    let mut lines: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut out = String::with_capacity(raw.len());
    let mut raw_pos = 0usize;
    for (i, line) in raw.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let rep_start = out.len();
        out.push_str(&crate::chunking::text_repair::repair_mojibake(line));
        lines.push((raw_pos, raw_pos + line.len(), rep_start, out.len()));
        raw_pos += line.len() + 1;
    }
    let mut starts = Vec::with_capacity(raw_starts.len());
    let mut prev = 0usize;
    for &s in raw_starts {
        let mapped = if s >= raw.len() {
            out.len()
        } else {
            // The line containing `s` (its raw range including the newline).
            let idx = lines.partition_point(|l| l.1 < s);
            match lines.get(idx) {
                Some(&(_, re, _, pe)) if s == re => pe,
                Some(&(_, _, ps, _)) => ps,
                None => out.len(),
            }
        };
        let mapped = mapped.max(prev);
        starts.push(mapped);
        prev = mapped;
    }
    (out, starts)
}

/// Byte range `[start, end)` of every page from its start offsets.
pub fn page_ranges(starts: &[usize], text_len: usize) -> Vec<(usize, usize)> {
    starts
        .iter()
        .enumerate()
        .map(|(i, &s)| (s.min(text_len), starts.get(i + 1).copied().unwrap_or(text_len).clamp(s.min(text_len), text_len)))
        .collect()
}

/// A chunk's position, for mapping pages onto chunks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkSpan {
    pub chunk_id: i64,
    pub char_start: i64,
    pub char_end: i64,
    /// `metadata.page` (structural chunker: 1-based start page), if any.
    pub page: Option<i64>,
}

/// Chunk ids belonging to `page` (1-based). `spans` = ALL chunks of the
/// document in `chunk_index` order (omitted ones too — they still occupy
/// pages; the caller filters them afterwards). If any chunk carries a page,
/// the page numbers decide (a chunk without one inherits its predecessor's);
/// otherwise the byte ranges are overlapped with `range` (`None` = unknown
/// range = no chunk).
pub fn chunks_for_page(spans: &[ChunkSpan], page: i64, range: Option<(i64, i64)>) -> Vec<i64> {
    if spans.iter().any(|s| s.page.is_some()) {
        let mut starts = Vec::with_capacity(spans.len());
        let mut last = 1i64;
        for s in spans {
            if let Some(p) = s.page {
                last = p;
            }
            starts.push(last);
        }
        return spans
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                let from = starts[*i];
                let to = starts.get(i + 1).copied().unwrap_or(i64::MAX);
                from <= page && page <= to.max(from)
            })
            .map(|(_, s)| s.chunk_id)
            .collect();
    }
    let Some((ps, pe)) = range else {
        return Vec::new();
    };
    // A page without extractable text has an empty range; it then belongs to
    // the chunk(s) touching that position (also when it sits on a boundary).
    spans
        .iter()
        .filter(|s| if ps == pe { s.char_start <= ps && ps <= s.char_end } else { s.char_start < pe && ps < s.char_end })
        .map(|s| s.chunk_id)
        .collect()
}
