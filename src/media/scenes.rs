//! Scene changes in a video, from the stills the decoder hands out at a fixed interval (one per tick).
//!
//! - **Signature of a still** ([`Signature::of`]): a colour histogram (HSV, coarse bins) and the mean
//!   brightness of a fixed grid of cells over the picture (a small preview picture). The histogram sees a
//!   cut between two camera shots; the grid sees what a histogram misses — one white slide with black text
//!   after another has nearly the same colours, but other cells change.
//! - **Distance** ([`distance`], 0–1): the larger of the moved histogram mass and the share of grid cells
//!   that changed by more than [`CELL_CHANGE`] levels, after taking out the change all cells share (the
//!   camera adjusting its exposure is no cut).
//! - **Cut** (like PySceneDetect's AdaptiveDetector): a still is the first of a new shot when its distance
//!   to the one before is at least [`MIN_SCORE`] and at least [`RATIO`] times the mean distance of its
//!   [`WINDOW`] neighbours on each side. A panning camera changes every still a lot, so only a jump well
//!   above that motion counts; on a standing slide the neighbours are near zero and any real change counts.
//!   The decision waits for [`WINDOW`] stills after the candidate. A shot of a single unstable still
//!   between two cuts (a fade, a slide transition) joins the next shot.
//! - **Pieces of a moving shot:** a handheld camera filming a projection screen changes every still, so a
//!   new slide on the screen is no jump above the motion and gives no cut. A shot is therefore closed as
//!   a piece ([`Shot::continues`] on the next) at the first moving still (distance at least
//!   [`MIN_SCORE`]) once it lasts [`PIECE_MS`]; the caller reads each piece's still, and
//!   `slide_track.rs` joins pieces showing the same slide or the same scene. A slide change inside such a
//!   shot is placed to within [`PIECE_MS`]; a standing picture (a screen recording, a tripod on a slide)
//!   never moves and is not split.
//! - **Still of a shot** ([`ShotSplitter`]): the latest still that stands (distance to its neighbours in
//!   the shot below [`MIN_SCORE`]): on a slide built up point by point that is the full slide; without one
//!   (constant motion) the steadiest. Only that still and the ones waiting for the decision are held, so
//!   memory does not grow with the video.
//!
//! The constants are rules of the method, not measurements of a device; checked on generated clips and a
//! real talk (`handbuch/medien.md` „Standbilder“).

use std::collections::VecDeque;

use super::Picture;

/// Hue, saturation and value bins of the histogram.
const H_BINS: usize = 12;
const S_BINS: usize = 3;
const V_BINS: usize = 4;
const BINS: usize = H_BINS * S_BINS * V_BINS;
/// Cells of the brightness grid (16:9 like most videos; a portrait video simply has taller cells).
pub const GRID_W: u32 = 32;
pub const GRID_H: u32 = 18;
/// Brightness change of a cell (levels of 255) that counts as changed content: above compression noise
/// and flicker of a standing picture, below one line of text appearing in a cell.
pub const CELL_CHANGE: f32 = 6.0;
/// Least distance of a cut (share of changed cells or moved histogram mass): about ten cells of the grid.
pub const MIN_SCORE: f32 = 0.02;
/// How far a cut must stand above the mean distance of its neighbours.
pub const RATIO: f32 = 3.0;
/// Neighbours on each side the cut is compared with.
pub const WINDOW: usize = 2;
/// Length of the pieces a moving shot is read in (see the module comment).
pub const PIECE_MS: u64 = 10_000;
/// At most this many pixels feed the histogram (every n-th pixel of a large picture).
const HIST_SAMPLES: u64 = 65_536;

/// What a still looks like, small enough to keep for the next comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct Signature {
    hist: Vec<f32>,
    grid: Vec<f32>,
}

impl Signature {
    pub fn of(p: &Picture) -> Self {
        let mut hist = vec![0f32; BINS];
        let total = u64::from(p.width) * u64::from(p.height);
        let step = total.div_ceil(HIST_SAMPLES).max(1);
        let mut n = 0f32;
        let mut i = 0u64;
        while i < total {
            let (x, y) = ((i % u64::from(p.width)) as u32, (i / u64::from(p.width)) as u32);
            hist[bin(p.px(x, y))] += 1.0;
            n += 1.0;
            i += step;
        }
        hist.iter_mut().for_each(|h| *h /= n.max(1.0));
        let grid = p.luma_area(GRID_W, GRID_H).px;
        Signature { hist, grid }
    }
}

/// Histogram bin of one pixel (HSV; grey pixels all fall into hue 0).
fn bin([r, g, b]: [u8; 3]) -> usize {
    let (r, g, b) = (f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let s = if max > 0.0 { delta / max } else { 0.0 };
    let h = if delta <= f32::EPSILON {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } / 6.0;
    let q = |v: f32, n: usize| ((v * n as f32) as usize).min(n - 1);
    (q(h, H_BINS) * S_BINS + q(s, S_BINS)) * V_BINS + q(max, V_BINS)
}

/// How different two stills are, 0 (same) to 1 (see the module comment).
pub fn distance(a: &Signature, b: &Signature) -> f32 {
    let moved = 0.5 * a.hist.iter().zip(&b.hist).map(|(x, y)| (x - y).abs()).sum::<f32>();
    let mut diffs: Vec<f32> = a.grid.iter().zip(&b.grid).map(|(x, y)| y - x).collect();
    let shared = median(&mut diffs.clone());
    let n = diffs.len().max(1) as f32;
    let changed = diffs.iter_mut().filter(|d| (**d - shared).abs() > CELL_CHANGE).count() as f32 / n;
    moved.max(changed)
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// One shot: from its first still to the first still of the next shot (or the end), with the still that
/// stands for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Shot<T> {
    pub t_start_ms: u64,
    pub t_end_ms: u64,
    /// Time of the chosen still.
    pub still_ms: u64,
    pub still: T,
    /// Whether the chosen still stands (see the module comment); `false` = the steadiest of a moving shot.
    pub steady: bool,
    /// A further piece of the shot before (see the module comment), not a cut.
    pub continues: bool,
}

/// A still waiting for its cut decision.
struct Pending<T> {
    t_ms: u64,
    /// Distance to the still before; `None` for the very first.
    score: Option<f32>,
    payload: T,
}

/// A still of the open shot that may become its chosen one.
struct Candidate<T> {
    t_ms: u64,
    payload: T,
    /// Smallest distance to a neighbour in the same shot (`INFINITY` while it has none).
    steadiness: f32,
}

impl<T> Candidate<T> {
    fn steady(&self) -> bool {
        self.steadiness < MIN_SCORE
    }

    /// A standing still beats a moving one, a later standing one an earlier, otherwise the steadier.
    fn beats(&self, old: &Candidate<T>) -> bool {
        if self.steady() != old.steady() { self.steady() } else { self.steady() || self.steadiness < old.steadiness }
    }
}

struct OpenShot<T> {
    start_ms: u64,
    /// Started by a cut (not the first shot of the video).
    after_cut: bool,
    /// A further piece of a moving shot.
    continues: bool,
    /// The last still added; its steadiness is final once the next still is in or the shot closes.
    last: Candidate<T>,
    chosen: Option<Candidate<T>>,
    stills: usize,
}

impl<T> OpenShot<T> {
    fn new(start_ms: u64, after_cut: bool, t_ms: u64, payload: T) -> Self {
        let last = Candidate { t_ms, payload, steadiness: f32::INFINITY };
        OpenShot { start_ms, after_cut, continues: false, last, chosen: None, stills: 1 }
    }

    fn add(&mut self, t_ms: u64, score: f32, payload: T) {
        let mut done = std::mem::replace(&mut self.last, Candidate { t_ms, payload, steadiness: score });
        done.steadiness = done.steadiness.min(score);
        self.offer(done);
        self.stills += 1;
    }

    fn offer(&mut self, c: Candidate<T>) {
        if self.chosen.as_ref().is_none_or(|old| c.beats(old)) {
            self.chosen = Some(c);
        }
    }

    fn close(self, end_ms: u64) -> Shot<T> {
        let c = match self.chosen {
            Some(old) if !self.last.beats(&old) => old,
            _ => self.last,
        };
        let continues = self.continues;
        Shot { t_start_ms: self.start_ms, t_end_ms: end_ms.max(c.t_ms), still_ms: c.t_ms, steady: c.steady(), still: c.payload, continues }
    }
}

/// Splits a stream of stills into shots (see the module comment). `T` is whatever the caller keeps per
/// still (the picture); only the chosen and the last still of the open shot and [`WINDOW`] waiting ones
/// are held.
pub struct ShotSplitter<T> {
    prev: Option<Signature>,
    pending: VecDeque<Pending<T>>,
    /// Distances of the last decided stills (the neighbours before a candidate).
    before: VecDeque<f32>,
    open: Option<OpenShot<T>>,
}

impl<T> Default for ShotSplitter<T> {
    fn default() -> Self {
        ShotSplitter { prev: None, pending: VecDeque::new(), before: VecDeque::new(), open: None }
    }
}

impl<T> ShotSplitter<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add the still at `t_ms` (time order); returns the shots this closed.
    pub fn push(&mut self, t_ms: u64, sig: Signature, payload: T) -> Vec<Shot<T>> {
        let score = self.prev.as_ref().map(|p| distance(p, &sig));
        self.prev = Some(sig);
        self.pending.push_back(Pending { t_ms, score, payload });
        let mut out = Vec::new();
        while self.pending.len() > WINDOW {
            self.decide_front(&mut out);
        }
        out
    }

    /// End of the video at `end_ms`: decides the waiting stills and closes the last shot.
    pub fn finish(mut self, end_ms: u64) -> Vec<Shot<T>> {
        let mut out = Vec::new();
        while !self.pending.is_empty() {
            self.decide_front(&mut out);
        }
        if let Some(open) = self.open.take() {
            out.push(open.close(end_ms));
        }
        out
    }

    /// Decide whether the oldest waiting still starts a new shot, and add it.
    fn decide_front(&mut self, out: &mut Vec<Shot<T>>) {
        let Some(p) = self.pending.pop_front() else { return };
        let cut = p.score.is_some_and(|s| self.is_cut(s));
        if let Some(s) = p.score {
            self.before.push_back(s);
            if self.before.len() > WINDOW {
                self.before.pop_front();
            }
        }
        self.open = Some(match self.open.take() {
            None => OpenShot::new(p.t_ms, false, p.t_ms, p.payload),
            Some(open) if !cut && p.score.is_some_and(|s| s >= MIN_SCORE) && p.t_ms >= open.start_ms + PIECE_MS => {
                out.push(open.close(p.t_ms));
                let mut piece = OpenShot::new(p.t_ms, false, p.t_ms, p.payload);
                piece.continues = true;
                piece
            }
            Some(mut open) if !cut => {
                open.add(p.t_ms, p.score.unwrap_or(0.0), p.payload);
                open
            }
            // One still between two cuts is a transition (fade, slide animation): it joins this shot.
            Some(open) if open.after_cut && open.stills == 1 => OpenShot::new(open.start_ms, true, p.t_ms, p.payload),
            Some(open) => {
                out.push(open.close(p.t_ms));
                OpenShot::new(p.t_ms, true, p.t_ms, p.payload)
            }
        });
    }

    /// The rule of the module comment for a candidate with distance `s`; the neighbours after it are the
    /// waiting stills.
    fn is_cut(&self, s: f32) -> bool {
        if s < MIN_SCORE {
            return false;
        }
        let around: Vec<f32> =
            self.before.iter().copied().chain(self.pending.iter().take(WINDOW).filter_map(|p| p.score)).collect();
        if around.is_empty() {
            return true;
        }
        let mean = around.iter().sum::<f32>() / around.len() as f32;
        s >= RATIO * mean
    }
}

#[cfg(test)]
#[path = "scenes_tests.rs"]
mod tests;
