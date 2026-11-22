//! Slide/screen or scene: whether a video still is something to read (a slide, a screen, a page held into
//! the camera) or a picture of the world (people, a room, a stage).
//!
//! The rule ([`classify`]) weighs what the text recognition found against how the picture looks:
//! - **Nothing to read** (fewer than [`MIN_CHARS`] letters and digits) → scene.
//! - **A found outline** (a projection screen or page filmed at an angle, `quad.rs`/Apple Vision) holding
//!   text over at least [`MIN_TEXT_SHARE`] of the straightened picture → slide.
//! - **Text on plain colour**: text boxes over at least [`MIN_TEXT_SHARE`] of the picture and at least
//!   [`MIN_FLAT_SHARE`] of the pixels in the few largest colour bins ([`Colours`]) — slides and screens
//!   have plain backgrounds, a photographed scene spreads over many colours → slide.
//! - **Dense text** (boxes over [`DENSE_TEXT_SHARE`] of the picture: code, a document page, a busy screen)
//!   → slide whatever the background.
//! - Otherwise → scene (a sign or a shirt print in a room stays a scene; its text is not a slide).
//!
//! A slide or screen recording with a speaker inset is a slide: the inset is small against the plain
//! background. The shares are rules of the method; checked on generated clips and real talks
//! (`handbuch/medien.md` „Standbilder“).

use super::Picture;

/// Least letters and digits to call a still readable.
pub const MIN_CHARS: usize = 12;
/// Least share of the picture covered by text boxes for a slide on plain colour or inside an outline: a
/// title slide reaches it, a logo or a sign on a lectern in a stage shot stays below.
pub const MIN_TEXT_SHARE: f32 = 0.04;
/// Text box share from which a still is a slide whatever its background.
pub const DENSE_TEXT_SHARE: f32 = 0.15;
/// Least share of pixels in the [`FLAT_BINS`] largest colour bins for a plain background.
pub const MIN_FLAT_SHARE: f32 = 0.5;
/// How many of the largest colour bins make up the background.
const FLAT_BINS: usize = 3;
/// Levels per channel of a colour bin (8 × 8 × 8 bins).
const BIN_LEVELS: usize = 8;

/// Slide/screen or scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Slide,
    Scene,
}

/// How plain the colours of a picture are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colours {
    /// Share of pixels in the [`FLAT_BINS`] largest colour bins.
    pub flat_share: f32,
}

impl Colours {
    pub fn of(p: &Picture) -> Self {
        let mut bins = vec![0u32; BIN_LEVELS * BIN_LEVELS * BIN_LEVELS];
        let shift = 8 - BIN_LEVELS.trailing_zeros();
        let total = u64::from(p.width) * u64::from(p.height);
        let step = total.div_ceil(65_536).max(1) as usize;
        let mut n = 0u32;
        for px in p.rgb.chunks_exact(3).step_by(step) {
            let q = |v: u8| usize::from(v >> shift);
            bins[(q(px[0]) * BIN_LEVELS + q(px[1])) * BIN_LEVELS + q(px[2])] += 1;
            n += 1;
        }
        bins.sort_unstable_by(|a, b| b.cmp(a));
        let top: u32 = bins.iter().take(FLAT_BINS).sum();
        Colours { flat_share: top as f32 / n.max(1) as f32 }
    }
}

/// What is known about a still when it is classified.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StillFacts {
    /// Letters and digits the text recognition read.
    pub text_chars: usize,
    /// Share of the read picture covered by text boxes (0–1).
    pub text_share: f32,
    /// The text was read inside a found outline (the straightened picture), not the whole still.
    pub in_outline: bool,
    pub colours: Colours,
}

/// The rule of the module comment.
pub fn classify(f: &StillFacts) -> FrameKind {
    if f.text_chars < MIN_CHARS {
        return FrameKind::Scene;
    }
    let readable = f.text_share >= MIN_TEXT_SHARE && (f.in_outline || f.colours.flat_share >= MIN_FLAT_SHARE);
    let slide = readable || f.text_share >= DENSE_TEXT_SHARE;
    if slide { FrameKind::Slide } else { FrameKind::Scene }
}

/// Letters and digits in `text` (what [`StillFacts::text_chars`] counts).
pub fn readable_chars(text: &str) -> usize {
    text.chars().filter(|c| c.is_alphanumeric()).count()
}

#[cfg(test)]
#[path = "frame_kind_tests.rs"]
mod tests;
