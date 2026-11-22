//! The outline of a document or slide in a picture — a page photographed on a table, a projection screen
//! filmed from the room — without Apple Vision (Windows, Linux; on Apple `services::ingest::ocr::apple_quad`
//! asks Vision first). Shared by the video stills and the document scan; straightening: `rectify.rs`.
//!
//! [`find_quad`]:
//! 1. The picture's brightness at [`WORK_EDGE`] pixels on the long edge, blurred twice (3 × 3 box) so the
//!    text on the page melts into the page.
//! 2. Split into bright and dark by Otsu's threshold (the one that separates the two brightness groups
//!    best); pages and lit screens are brighter than what is around them.
//! 3. The largest connected bright area, if it covers at least [`MIN_AREA`] of the picture.
//! 4. Its convex hull (from the leftmost and rightmost pixel of each row), then the four hull points that
//!    span the largest quadrilateral.
//! 5. Accepted when the quadrilateral covers at least [`MIN_HULL_SHARE`] of the hull (four straight sides,
//!    not a disc), the area fills it to at least [`MIN_FILL`] (a person standing in front of a screen cuts
//!    out less) and every corner angle lies within [`ANGLE_RANGE`] degrees.
//! 6. Each side is fitted as a line through the hull points between its corners (without the parts near
//!    the corners, which the blur rounded), and the corners become the crossings of those lines.
//!
//! A page on an equally bright ground, or a dark slide in a dark room, gives no outline: the caller then
//! uses the whole picture. [`Quad::covers`] tells when the outline is (nearly) the whole picture, so there
//! is nothing to straighten.

use super::{Gray, Picture};

/// Long edge of the working picture.
pub const WORK_EDGE: u32 = 320;
/// Least share of the picture the outline's area covers.
pub const MIN_AREA: f32 = 0.08;
/// Least share of the quadrilateral the bright area fills.
pub const MIN_FILL: f32 = 0.8;
/// Least share of the hull the quadrilateral covers: a disc reaches 2/π ≈ 0.64; a lit screen whose bright
/// area runs into a light wall or ceiling at one edge stays above.
pub const MIN_HULL_SHARE: f32 = 0.8;
/// Allowed corner angles (degrees).
pub const ANGLE_RANGE: (f32, f32) = (45.0, 135.0);
/// Share of the picture from which an outline counts as the whole picture.
pub const COVERS_SHARE: f32 = 0.9;
/// Distance (share of width or height) within which an outline's corner counts as the picture's corner.
pub const CORNER_MARGIN: f32 = 0.03;
/// Most hull points searched for the quadrilateral (a longer hull is thinned evenly).
const MAX_HULL: usize = 160;

/// Four corners in pixels of the picture they were found in: top left, top right, bottom right, bottom
/// left (as seen in the picture).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub corners: [(f32, f32); 4],
}

impl Quad {
    /// Corners in any order, sorted into top left, top right, bottom right, bottom left: around their
    /// centre by angle, starting with the one nearest the top-left direction.
    pub fn from_points(points: [(f32, f32); 4]) -> Self {
        let cx = points.iter().map(|p| p.0).sum::<f32>() / 4.0;
        let cy = points.iter().map(|p| p.1).sum::<f32>() / 4.0;
        let mut pts = points;
        // Clockwise on screen (y down) = increasing atan2(y, x).
        pts.sort_by(|a, b| (a.1 - cy).atan2(a.0 - cx).total_cmp(&(b.1 - cy).atan2(b.0 - cx)));
        let tl = (0..4).min_by(|&i, &j| (pts[i].0 + pts[i].1).total_cmp(&(pts[j].0 + pts[j].1))).unwrap_or(0);
        Quad { corners: [pts[tl], pts[(tl + 1) % 4], pts[(tl + 2) % 4], pts[(tl + 3) % 4]] }
    }

    /// Corners given as shares of the picture (0–1, origin top left) in a picture of `w × h`.
    pub fn from_shares(shares: [(f32, f32); 4], w: u32, h: u32) -> Self {
        Self::from_points(shares.map(|(x, y)| (x * w as f32, y * h as f32)))
    }

    /// Area (shoelace formula).
    pub fn area(&self) -> f32 {
        let c = &self.corners;
        0.5 * (0..4).map(|i| c[i].0 * c[(i + 1) % 4].1 - c[(i + 1) % 4].0 * c[i].1).sum::<f32>().abs()
    }

    /// Whether the outline is (nearly) the whole `w × h` picture, so there is nothing to straighten: it
    /// covers at least [`COVERS_SHARE`] of it, or three of its corners lie within [`CORNER_MARGIN`] of the
    /// picture's corners (a screen recording whose speaker inset cuts one corner off the slide).
    pub fn covers(&self, w: u32, h: u32) -> bool {
        let (w, h) = (w as f32, h as f32);
        let picture = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
        let near = self
            .corners
            .iter()
            .zip(picture)
            .filter(|(c, p)| (c.0 - p.0).abs() <= CORNER_MARGIN * w && (c.1 - p.1).abs() <= CORNER_MARGIN * h)
            .count();
        self.area() >= COVERS_SHARE * w * h || near >= 3
    }

    /// Whether the outline can be a page or screen in a `w × h` picture: at least [`MIN_AREA`] of it,
    /// every corner angle within [`ANGLE_RANGE`] and every corner inside the picture (give or take
    /// [`CORNER_MARGIN`]; a page cut off by the picture's edge has no corner there to straighten by).
    /// Also the check for an outline Apple Vision found.
    pub fn plausible(&self, w: u32, h: u32) -> bool {
        let (wf, hf) = (w as f32, h as f32);
        let inside = |&(x, y): &(f32, f32)| {
            (-CORNER_MARGIN * wf..=(1.0 + CORNER_MARGIN) * wf).contains(&x)
                && (-CORNER_MARGIN * hf..=(1.0 + CORNER_MARGIN) * hf).contains(&y)
        };
        self.area() >= MIN_AREA * wf * hf
            && self.corners.iter().all(inside)
            && self.angles().iter().all(|a| (ANGLE_RANGE.0..=ANGLE_RANGE.1).contains(a))
    }

    /// Whether the point (pixels) lies inside the outline.
    pub fn contains(&self, (x, y): (f32, f32)) -> bool {
        let c = &self.corners;
        let side = |i: usize| {
            let (a, b) = (c[i], c[(i + 1) % 4]);
            (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0)
        };
        let s = [side(0), side(1), side(2), side(3)];
        s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0)
    }

    /// Interior angles in degrees, one per corner.
    fn angles(&self) -> [f32; 4] {
        let c = &self.corners;
        std::array::from_fn(|i| {
            let (p, a, b) = (c[i], c[(i + 3) % 4], c[(i + 1) % 4]);
            let (ux, uy, vx, vy) = (a.0 - p.0, a.1 - p.1, b.0 - p.0, b.1 - p.1);
            let cos = (ux * vx + uy * vy) / ((ux.hypot(uy) * vx.hypot(vy)).max(f32::EPSILON));
            cos.clamp(-1.0, 1.0).acos().to_degrees()
        })
    }
}

/// The outline of the brightest large quadrilateral area (see the module comment).
pub fn find_quad(p: &Picture) -> Option<Quad> {
    let long = p.width.max(p.height);
    let scale = (WORK_EDGE as f32 / long as f32).min(1.0);
    let (w, h) = (((p.width as f32 * scale).round() as u32).max(1), ((p.height as f32 * scale).round() as u32).max(1));
    let g = blur(&blur(&p.luma_area(w, h)));
    let threshold = otsu(&g.px)?;
    let (pixels, rows) = largest_bright_area(&g, threshold)?;
    if (pixels as f32) < MIN_AREA * (w * h) as f32 {
        return None;
    }
    let hull = hull(&rows);
    let (quad, idx) = largest_quad(&hull)?;
    let fill = pixels as f32 / quad.area().max(1.0);
    let hull_share = quad.area() / polygon_area(&hull).max(1.0);
    let quad = straight_sides(&hull, idx).unwrap_or(quad);
    if fill < MIN_FILL || hull_share < MIN_HULL_SHARE || !quad.plausible(w, h) {
        return None;
    }
    // Pixel centres of the working picture back to the picture's pixels.
    let (sx, sy) = (p.width as f32 / w as f32, p.height as f32 / h as f32);
    Some(Quad { corners: quad.corners.map(|(x, y)| ((x + 0.5) * sx, (y + 0.5) * sy)) })
}

/// 3 × 3 box blur (edges repeat).
fn blur(g: &Gray) -> Gray {
    let (w, h) = (g.width as i64, g.height as i64);
    let at = |x: i64, y: i64| g.px[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize];
    let mut px = Vec::with_capacity(g.px.len());
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    s += at(x + dx, y + dy);
                }
            }
            px.push(s / 9.0);
        }
    }
    Gray { width: g.width, height: g.height, px }
}

/// Otsu's threshold over 256 levels; `None` for a picture of one brightness.
fn otsu(px: &[f32]) -> Option<f32> {
    let mut hist = [0u32; 256];
    for v in px {
        hist[v.round().clamp(0.0, 255.0) as usize] += 1;
    }
    let total = px.len() as f64;
    let sum_all: f64 = hist.iter().enumerate().map(|(i, &n)| i as f64 * f64::from(n)).sum();
    let (mut w0, mut sum0, mut best, mut best_t) = (0.0f64, 0.0f64, 0.0f64, None);
    for (t, &n) in hist.iter().enumerate() {
        w0 += f64::from(n);
        sum0 += t as f64 * f64::from(n);
        let w1 = total - w0;
        if w0 == 0.0 || w1 == 0.0 {
            continue;
        }
        let between = w0 * w1 * (sum0 / w0 - (sum_all - sum0) / w1).powi(2);
        if between > best {
            best = between;
            best_t = Some(t as f32 + 0.5);
        }
    }
    best_t
}

/// One row of an area: its y and the x of its leftmost and rightmost pixel.
type RowSpan = (u32, u32, u32);

/// Pixel count and the rows of the largest 4-connected area brighter than `threshold`.
fn largest_bright_area(g: &Gray, threshold: f32) -> Option<(usize, Vec<RowSpan>)> {
    let (w, h) = (g.width as usize, g.height as usize);
    let mut label = vec![0u32; w * h];
    let (mut best, mut best_size, mut next) = (0u32, 0usize, 1u32);
    let mut stack = Vec::new();
    for start in 0..w * h {
        if label[start] != 0 || g.px[start] <= threshold {
            continue;
        }
        let mut size = 0;
        label[start] = next;
        stack.push(start);
        while let Some(i) = stack.pop() {
            size += 1;
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if label[j] == 0 && g.px[j] > threshold {
                    label[j] = next;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        if size > best_size {
            (best, best_size) = (next, size);
        }
        next += 1;
    }
    if best_size == 0 {
        return None;
    }
    let rows = (0..h)
        .filter_map(|y| {
            let row = &label[y * w..(y + 1) * w];
            let first = row.iter().position(|&l| l == best)?;
            let last = row.iter().rposition(|&l| l == best)?;
            Some((y as u32, first as u32, last as u32))
        })
        .collect();
    Some((best_size, rows))
}

/// Convex hull (Andrew's monotone chain, counter-clockwise on screen) of the row ends.
fn hull(rows: &[RowSpan]) -> Vec<(f32, f32)> {
    let mut pts: Vec<(f32, f32)> =
        rows.iter().flat_map(|&(y, a, b)| [(a as f32, y as f32), (b as f32, y as f32)]).collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: (f32, f32), a: (f32, f32), b: (f32, f32)| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut lower: Vec<(f32, f32)> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<(f32, f32)> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Area of a polygon (shoelace formula).
fn polygon_area(pts: &[(f32, f32)]) -> f32 {
    let n = pts.len();
    0.5 * (0..n).map(|i| pts[i].0 * pts[(i + 1) % n].1 - pts[(i + 1) % n].0 * pts[i].1).sum::<f32>().abs()
}

/// The quadrilateral of largest area with corners on the hull (and the corners' hull indices, in hull
/// order): for every diagonal the farthest hull point on each side.
fn largest_quad(hull: &[(f32, f32)]) -> Option<(Quad, [usize; 4])> {
    let step = hull.len().div_ceil(MAX_HULL).max(1);
    let pts: Vec<(usize, (f32, f32))> = hull.iter().copied().enumerate().step_by(step).collect();
    let n = pts.len();
    if n < 4 {
        return None;
    }
    // Twice the signed triangle area; the hull runs one way round, so one side is positive.
    let tri = |a: (usize, (f32, f32)), b: (usize, (f32, f32)), c: (usize, (f32, f32))| {
        let (a, b, c) = (a.1, b.1, c.1);
        ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs()
    };
    let mut best: Option<(f32, [usize; 4])> = None;
    for i in 0..n {
        for k in i + 2..n {
            let side = |range: &mut dyn Iterator<Item = usize>| {
                range.map(|j| (tri(pts[i], pts[k], pts[j]), j)).max_by(|a, b| a.0.total_cmp(&b.0))
            };
            let Some((a1, j)) = side(&mut (i + 1..k)) else { continue };
            let Some((a2, l)) = side(&mut (k + 1..n).chain(0..i)) else { continue };
            let area = a1 + a2;
            if best.is_none_or(|(b, _)| area > b) {
                best = Some((area, [i, j, k, l]));
            }
        }
    }
    let (_, idx) = best?;
    let idx = idx.map(|i| pts[i].0);
    Some((Quad::from_points(idx.map(|i| hull[i])), idx))
}

/// Corners as crossings of the four sides, each fitted through the hull points between two corners
/// (`idx`: hull indices in hull order) without the fifth next to either corner; `None` when a side has
/// too few points or two sides are parallel.
fn straight_sides(hull: &[(f32, f32)], idx: [usize; 4]) -> Option<Quad> {
    let n = hull.len();
    let lines: Vec<Line> = (0..4)
        .map(|s| {
            let (a, b) = (idx[s], idx[(s + 1) % 4]);
            let len = (b + n - a) % n;
            let skip = len / 5;
            let pts: Vec<(f32, f32)> = (skip..=len - skip).map(|k| hull[(a + k) % n]).collect();
            // Too few points on the side: the line through its two corners.
            if pts.len() < 3 { Line::through(&[hull[a], hull[b]]) } else { Line::through(&pts) }
        })
        .collect::<Option<_>>()?;
    let corners: Vec<(f32, f32)> = (0..4).map(|s| lines[(s + 3) % 4].cross(&lines[s])).collect::<Option<_>>()?;
    Some(Quad::from_points([corners[0], corners[1], corners[2], corners[3]]))
}

/// A line `nx·x + ny·y = d` with unit normal.
struct Line {
    nx: f32,
    ny: f32,
    d: f32,
}

impl Line {
    /// Total least squares through `pts` (at least two distinct points).
    fn through(pts: &[(f32, f32)]) -> Option<Line> {
        let n = pts.len() as f32;
        let (mx, my) = (pts.iter().map(|p| p.0).sum::<f32>() / n, pts.iter().map(|p| p.1).sum::<f32>() / n);
        let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
        for p in pts {
            let (dx, dy) = (p.0 - mx, p.1 - my);
            sxx += dx * dx;
            sxy += dx * dy;
            syy += dy * dy;
        }
        if sxx + syy <= f32::EPSILON {
            return None;
        }
        // Direction of largest spread; the normal is perpendicular to it.
        let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
        let (nx, ny) = (-angle.sin(), angle.cos());
        Some(Line { nx, ny, d: nx * mx + ny * my })
    }

    fn cross(&self, o: &Line) -> Option<(f32, f32)> {
        let det = self.nx * o.ny - self.ny * o.nx;
        (det.abs() > 1e-4).then(|| ((self.d * o.ny - self.ny * o.d) / det, (self.nx * o.d - self.d * o.nx) / det))
    }
}

#[cfg(test)]
#[path = "quad_tests.rs"]
mod tests;
