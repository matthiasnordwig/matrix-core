//! The same slide over time becomes one unit with a time range.
//!
//! The shots of a video (`scenes.rs`) arrive in time order, each classified (`frame_kind.rs`). Consecutive
//! slide shots showing the same slide join ([`Tracker`]): a slide that stands while the camera flickers,
//! or that is built up point by point. Whether two stills show the same slide ([`same_slide`]):
//! - **With text on both** (at least [`MIN_WORDS`] words each): the words of the shorter text are, to at
//!   least [`SAME_WORDS`], also in the longer one. A slide built up point by point keeps all earlier words;
//!   the next slide of a talk shares at most its header and footer.
//! - **Otherwise** (a picture slide, little text): the brightness hashes ([`SlideKey`], 16 × 16 cells above
//!   or below their mean) differ in at most [`SAME_HASH_BITS`] of 256 cells.
//!
//! The unit keeps the still with the most words (the fully built slide; on a tie the later one) and the
//! next comparison uses the latest still, so a slide that grows step by step stays one unit — unless that
//! still was read with fewer than [`MIN_WORDS`] words while an earlier one had more (a speaker in front of
//! the screen): then the earlier words stay the comparison, so a chain of poorly read stills cannot join
//! different slides by their similar pictures. A scene shot
//! ends the slide. Scenes join only as pieces of one moving shot (`scenes.rs`, [`ShotStill::continues`]);
//! after a cut a scene is a unit of its own. The thresholds are rules of the method,
//! checked on generated clips and real talks (`handbuch/medien.md` „Standbilder“).

use std::collections::HashSet;

use super::frame_kind::FrameKind;
use super::Picture;

/// Least words on both stills to compare by text.
pub const MIN_WORDS: usize = 3;
/// Share of the shorter text's words that must be in the longer one.
pub const SAME_WORDS: f32 = 0.8;
/// Most differing hash cells (of 256) between two stills of the same slide.
pub const SAME_HASH_BITS: u32 = 24;
/// Side of the hash grid.
const HASH_SIDE: u32 = 16;

/// What a slide still is compared by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlideKey {
    hash: [u64; 4],
    words: HashSet<String>,
}

impl SlideKey {
    /// From the (straightened) slide picture and its recognised text.
    pub fn new(p: &Picture, text: &str) -> Self {
        let g = p.luma_area(HASH_SIDE, HASH_SIDE);
        let mean = g.px.iter().sum::<f32>() / g.px.len().max(1) as f32;
        let mut hash = [0u64; 4];
        for (i, v) in g.px.iter().enumerate().take(256) {
            if *v > mean {
                hash[i / 64] |= 1 << (i % 64);
            }
        }
        SlideKey { hash, words: words(text) }
    }

    fn hash_distance(&self, other: &SlideKey) -> u32 {
        self.hash.iter().zip(&other.hash).map(|(a, b)| (a ^ b).count_ones()).sum()
    }
}

/// Lower-case words of two or more letters or digits (what slides are compared by).
pub fn words(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 2).map(str::to_lowercase).collect()
}

/// The text part of the rule: `None` when either side has fewer than [`MIN_WORDS`] words (then only the
/// picture can tell); also the rule for build-up pages of a slide PDF (`chunking/slide_steps.rs`).
pub fn same_words(a: &HashSet<String>, b: &HashSet<String>) -> Option<bool> {
    if a.len() < MIN_WORDS || b.len() < MIN_WORDS {
        return None;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let shared = short.intersection(long).count();
    Some(shared as f32 >= SAME_WORDS * short.len() as f32)
}

/// The rule of the module comment.
pub fn same_slide(a: &SlideKey, b: &SlideKey) -> bool {
    same_words(&a.words, &b.words).unwrap_or_else(|| a.hash_distance(b) <= SAME_HASH_BITS)
}

/// One classified shot of the video.
#[derive(Debug, Clone)]
pub struct ShotStill<T> {
    pub t_start_ms: u64,
    pub t_end_ms: u64,
    pub kind: FrameKind,
    /// For slides; scenes need none.
    pub key: Option<SlideKey>,
    /// Recognised text (slides; may be empty).
    pub text: String,
    pub payload: T,
    /// A further piece of the shot before (no cut between).
    pub continues: bool,
}

/// A slide standing over a time range, or one scene shot.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit<T> {
    pub t_start_ms: u64,
    pub t_end_ms: u64,
    pub kind: FrameKind,
    /// Text of the kept still.
    pub text: String,
    /// The kept still (see the module comment).
    pub payload: T,
    /// Shots joined into this unit.
    pub shots: usize,
}

struct Open<T> {
    unit: Unit<T>,
    kept_words: usize,
    /// Latest still of an open slide; `None` for an open scene.
    latest: Option<SlideKey>,
}

/// Joins consecutive shots of the same slide, and the pieces of one moving scene (see the module comment).
pub struct Tracker<T> {
    open: Option<Open<T>>,
}

impl<T> Default for Tracker<T> {
    fn default() -> Self {
        Tracker { open: None }
    }
}

impl<T> Tracker<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add the next shot; returns the units it closed (in time order).
    pub fn push(&mut self, s: ShotStill<T>) -> Vec<Unit<T>> {
        let key = if s.kind == FrameKind::Slide { s.key } else { None };
        let joins = self.open.as_ref().is_some_and(|o| match (&o.latest, &key) {
            (Some(a), Some(b)) => same_slide(a, b),
            (None, None) => s.continues,
            _ => false,
        });
        let n_words = key.as_ref().map_or(0, |k| k.words.len());
        if joins && let Some(open) = &mut self.open {
            let u = &mut open.unit;
            u.t_end_ms = u.t_end_ms.max(s.t_end_ms);
            u.shots += 1;
            // Slides keep the fullest still; a moving scene keeps its first piece's.
            if key.is_some() && n_words >= open.kept_words {
                u.text = s.text;
                u.payload = s.payload;
                open.kept_words = n_words;
            }
            // A piece read with too few words (a speaker in front of the screen) does not replace the words
            // the next slide is compared with.
            let keep_old = open.latest.as_ref().is_some_and(|k| k.words.len() >= MIN_WORDS) && n_words < MIN_WORDS;
            if key.is_some() && !keep_old {
                open.latest = key;
            }
            return Vec::new();
        }
        let kind = if key.is_some() { FrameKind::Slide } else { FrameKind::Scene };
        let unit = Unit { t_start_ms: s.t_start_ms, t_end_ms: s.t_end_ms, kind, text: s.text, payload: s.payload, shots: 1 };
        let closed = self.open.replace(Open { unit, kept_words: n_words, latest: key });
        closed.map(|o| o.unit).into_iter().collect()
    }

    /// The last open unit.
    pub fn finish(mut self) -> Option<Unit<T>> {
        self.open.take().map(|o| o.unit)
    }
}

#[cfg(test)]
#[path = "slide_track_tests.rs"]
mod tests;
