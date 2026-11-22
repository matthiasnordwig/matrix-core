//! Perspective straightening of a found outline (`quad.rs`, Apple Vision) into an upright rectangle: a
//! page photographed at an angle, a projection screen filmed from the side of the room. Shared by the video
//! stills and the document scan.
//!
//! - **Size** ([`output_size`]): width = the longer of the top and bottom edge, height = the longer of the
//!   left and right edge (the near edge shows the true length best), then scaled down to the requested long
//!   edge; never enlarged. The true aspect of a strongly tilted page would need the camera's focal length;
//!   the longer edges are the usual approximation and keep text legible.
//! - **Mapping** ([`homography`]): the projective map from the output rectangle onto the outline, from its
//!   four corner pairs (an 8 × 8 linear system, solved by Gaussian elimination with pivoting); each output
//!   pixel reads the picture there with bilinear interpolation ([`rectify`]).

use super::quad::Quad;
use super::{Picture, RgbBuf};

/// Output size for `q` (see the module comment), at most `long_edge` on the long side.
pub fn output_size(q: &Quad, long_edge: Option<u32>) -> (u32, u32) {
    let c = &q.corners;
    let len = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).hypot(a.1 - b.1);
    let w = len(c[0], c[1]).max(len(c[3], c[2])).max(1.0);
    let h = len(c[0], c[3]).max(len(c[1], c[2])).max(1.0);
    let scale = long_edge.map_or(1.0, |l| (l as f32 / w.max(h)).min(1.0));
    (((w * scale).round() as u32).max(1), ((h * scale).round() as u32).max(1))
}

/// The projective map taking each `from[i]` to `to[i]`, as a row-major 3 × 3 matrix with the last entry 1;
/// `None` when three points lie on one line.
pub fn homography(from: [(f32, f32); 4], to: [(f32, f32); 4]) -> Option<[f64; 9]> {
    // x' = (a x + b y + c) / (g x + h y + 1), y' = (d x + e y + f) / (g x + h y + 1)
    let mut m = [[0f64; 9]; 8];
    for i in 0..4 {
        let (x, y) = (f64::from(from[i].0), f64::from(from[i].1));
        let (u, v) = (f64::from(to[i].0), f64::from(to[i].1));
        m[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        m[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    for col in 0..8 {
        let pivot = (col..8).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
        if m[pivot][col].abs() < 1e-9 {
            return None;
        }
        m.swap(col, pivot);
        let pivot_row = m[col];
        for (r, row) in m.iter_mut().enumerate() {
            if r != col {
                let f = row[col] / pivot_row[col];
                row.iter_mut().zip(pivot_row).skip(col).for_each(|(v, p)| *v -= f * p);
            }
        }
    }
    let s: [f64; 8] = std::array::from_fn(|i| m[i][8] / m[i][i]);
    Some([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], 1.0])
}

/// Apply `h` to (`x`, `y`).
pub fn apply(h: &[f64; 9], x: f64, y: f64) -> (f64, f64) {
    let w = h[6] * x + h[7] * y + h[8];
    ((h[0] * x + h[1] * y + h[2]) / w, (h[3] * x + h[4] * y + h[5]) / w)
}

/// The outline `q` of `p` as an upright picture of at most `long_edge` on the long side; `None` when the
/// outline is degenerate.
pub fn rectify(p: &Picture, q: &Quad, long_edge: Option<u32>) -> Option<RgbBuf> {
    let (w, h) = output_size(q, long_edge);
    let rect = [(0.0, 0.0), (w as f32, 0.0), (w as f32, h as f32), (0.0, h as f32)];
    let map = homography(rect, q.corners)?;
    let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
    for oy in 0..h {
        for ox in 0..w {
            // Pixel centres on both sides.
            let (x, y) = apply(&map, f64::from(ox) + 0.5, f64::from(oy) + 0.5);
            rgb.extend_from_slice(&bilinear(p, x as f32 - 0.5, y as f32 - 0.5));
        }
    }
    Some(RgbBuf { width: w, height: h, rgb })
}

/// The picture at a fractional pixel position (edges repeat).
fn bilinear(p: &Picture, x: f32, y: f32) -> [u8; 3] {
    let (mx, my) = ((p.width - 1) as f32, (p.height - 1) as f32);
    let (x, y) = (x.clamp(0.0, mx), y.clamp(0.0, my));
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(p.width - 1), (y0 + 1).min(p.height - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let (a, b, c, d) = (p.px(x0, y0), p.px(x1, y0), p.px(x0, y1), p.px(x1, y1));
    std::array::from_fn(|i| {
        let top = f32::from(a[i]) * (1.0 - fx) + f32::from(b[i]) * fx;
        let bottom = f32::from(c[i]) * (1.0 - fx) + f32::from(d[i]) * fx;
        (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8
    })
}

#[cfg(test)]
#[path = "rectify_tests.rs"]
mod tests;
