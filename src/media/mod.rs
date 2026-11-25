//! Pure logic of time-based documents (audio, video), without network or OS: the stored text of a
//! media document, clustering of voice vectors, and the analysis of video stills (scene changes, slide
//! or scene, the same slide over time, finding and straightening a document quadrilateral), and the scene labels of
//! a picture and those a question names.
//! Decoding, models and the job live in the services crate (`media/`, `speech/`). Flow and decisions:
//! `handbuch/medien.md`.

pub mod diarize_cluster;
pub mod frame_kind;
pub mod quad;
pub mod rectify;
pub mod scene_labels;
pub mod scene_words;
pub mod scenes;
pub mod slide_track;
pub mod text;

/// An RGB picture borrowed from the caller: 8 bits per channel, rows top to bottom without padding (what
/// `image::RgbImage::as_raw` holds). The still analysis reads pictures only through this.
#[derive(Debug, Clone, Copy)]
pub struct Picture<'a> {
    pub width: u32,
    pub height: u32,
    pub rgb: &'a [u8],
}

impl<'a> Picture<'a> {
    /// `None` when `rgb` is not exactly `width × height × 3` bytes or the picture is empty.
    pub fn new(width: u32, height: u32, rgb: &'a [u8]) -> Option<Self> {
        (width > 0 && height > 0 && rgb.len() == width as usize * height as usize * 3).then_some(Picture { width, height, rgb })
    }

    /// The pixel at (`x`, `y`).
    pub fn px(&self, x: u32, y: u32) -> [u8; 3] {
        let i = (y as usize * self.width as usize + x as usize) * 3;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }

    /// Luma (BT.601 weights) of the picture scaled down to `out_w × out_h` by averaging the pixels under
    /// each output pixel; never larger than the picture.
    pub fn luma_area(&self, out_w: u32, out_h: u32) -> Gray {
        let (ow, oh) = (out_w.clamp(1, self.width), out_h.clamp(1, self.height));
        let span = |o: u32, out: u32, src: u32| {
            let a = (u64::from(o) * u64::from(src) / u64::from(out)) as u32;
            let b = ((u64::from(o + 1) * u64::from(src) / u64::from(out)) as u32).max(a + 1).min(src);
            (a, b)
        };
        let mut px = Vec::with_capacity(ow as usize * oh as usize);
        for oy in 0..oh {
            let (y0, y1) = span(oy, oh, self.height);
            for ox in 0..ow {
                let (x0, x1) = span(ox, ow, self.width);
                let mut sum = 0u32;
                for y in y0..y1 {
                    let row = &self.rgb[(y as usize * self.width as usize + x0 as usize) * 3..(y as usize * self.width as usize + x1 as usize) * 3];
                    for c in row.chunks_exact(3) {
                        sum += 299 * u32::from(c[0]) + 587 * u32::from(c[1]) + 114 * u32::from(c[2]);
                    }
                }
                px.push(sum as f32 / (1000.0 * ((y1 - y0) * (x1 - x0)) as f32));
            }
        }
        Gray { width: ow, height: oh, px }
    }
}

/// A grey picture (0–255 as `f32`), rows top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct Gray {
    pub width: u32,
    pub height: u32,
    pub px: Vec<f32>,
}

impl Gray {
    pub fn at(&self, x: u32, y: u32) -> f32 {
        self.px[y as usize * self.width as usize + x as usize]
    }
}

/// An RGB picture owned (a straightened slide or page), same layout as [`Picture`].
#[derive(Debug, Clone, PartialEq)]
pub struct RgbBuf {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

impl RgbBuf {
    pub fn picture(&self) -> Picture<'_> {
        Picture { width: self.width, height: self.height, rgb: &self.rgb }
    }
}

#[cfg(test)]
mod test_pictures;
#[cfg(test)]
mod tests_props;
