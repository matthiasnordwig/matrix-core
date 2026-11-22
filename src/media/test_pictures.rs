//! Made-up pictures for the tests of the still analysis: plain backgrounds, dark bars standing in for
//! lines of text, noise standing in for a filmed scene, and a page drawn in perspective.

use super::RgbBuf;

/// A picture of `w × h` in one colour.
pub fn plain(w: u32, h: u32, c: [u8; 3]) -> RgbBuf {
    RgbBuf { width: w, height: h, rgb: c.iter().copied().cycle().take(w as usize * h as usize * 3).collect() }
}

/// Fill the rectangle `x0..x1 × y0..y1` (clipped) with `c`.
pub fn fill(b: &mut RgbBuf, (x0, y0, x1, y1): (u32, u32, u32, u32), c: [u8; 3]) {
    for y in y0.min(b.height)..y1.min(b.height) {
        for x in x0.min(b.width)..x1.min(b.width) {
            let i = (y as usize * b.width as usize + x as usize) * 3;
            b.rgb[i..i + 3].copy_from_slice(&c);
        }
    }
}

/// A white slide with dark bars as lines of text: `lines` gives each line's length as a share of the
/// width, `seed` shifts where words break.
pub fn slide(w: u32, h: u32, lines: &[f32], seed: u32) -> RgbBuf {
    let mut b = plain(w, h, [250, 250, 250]);
    let line_h = (h / 18).max(2);
    for (i, len) in lines.iter().enumerate() {
        let y = h / 8 + i as u32 * line_h * 2;
        let end = (w as f32 * 0.08 + w as f32 * 0.84 * len) as u32;
        let mut x = w / 12;
        let mut k = seed.wrapping_add(i as u32 * 7);
        while x < end {
            k = k.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let word = w / 30 + (k >> 16) % (w / 12).max(1);
            fill(&mut b, (x, y, (x + word).min(end), y + line_h), [30, 30, 30]);
            x += word + w / 60 + 1;
        }
    }
    b
}

/// Coloured blocks of a tenth of the width (a filmed scene); `shift` pixels move it sideways (a panning
/// camera).
pub fn scene(w: u32, h: u32, seed: u32, shift: u32) -> RgbBuf {
    let mut b = plain(w, h, [0, 0, 0]);
    for y in 0..h {
        for x in 0..w {
            let block = (w / 10).max(1);
            let sx = (x + shift) / block;
            let mut k = (sx.wrapping_mul(73_856_093) ^ (y / block).wrapping_mul(19_349_663) ^ seed.wrapping_mul(83_492_791)) as u64;
            k = k.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let i = (y as usize * w as usize + x as usize) * 3;
            b.rgb[i..i + 3].copy_from_slice(&[(k >> 40) as u8, (k >> 48) as u8, (k >> 56) as u8]);
        }
    }
    b
}

/// Whether (`x`, `y`) lies inside the convex quadrilateral `q` (corners in order).
pub fn inside(q: &[(f32, f32); 4], x: f32, y: f32) -> bool {
    let s: Vec<f32> =
        (0..4).map(|i| { let (a, b) = (q[i], q[(i + 1) % 4]); (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0) }).collect();
    s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0)
}
