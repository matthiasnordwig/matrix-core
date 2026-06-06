//! Token-budgeted sliding windows over **segments**. Each window's text is the
//! indexed render `[i] <segment text>` (one per line) that the LLM sees; it
//! returns the segment indices where chunks start, so no string matching is
//! ever needed.

use serde::{Deserialize, Serialize};

use super::segments::Segment;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Window {
    pub idx: usize,
    pub start_segment: usize, // inclusive
    pub end_segment: usize,   // inclusive
    pub byte_start: usize,
    pub byte_end: usize,
    pub text: String, // indexed render shown to the LLM
}

/// Rough, offline token estimate (~4 chars/token). Deliberately approximate —
/// leave headroom in `window_tokens` for the prompt template + model output.
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4).max(1)
}

fn render(segments: &[Segment], start: usize, end: usize) -> String {
    (start..end)
        .map(|i| format!("[{}] {}", segments[i].index, segments[i].text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build overlapping windows whose estimated token count stays within
/// `window_tokens` (a single oversized segment still forms its own window).
pub fn build_windows(
    segments: &[Segment],
    window_tokens: usize,
    overlap_ratio: f64,
) -> Vec<Window> {
    if segments.is_empty() || window_tokens == 0 {
        return Vec::new();
    }
    let n = segments.len();
    let tok: Vec<usize> = segments.iter().map(|s| estimate_tokens(&s.text)).collect();
    let overlap = overlap_ratio.clamp(0.0, 0.95);
    let overlap_tokens = ((window_tokens as f64) * overlap) as usize;

    let mut windows = Vec::new();
    let mut start = 0usize;
    let mut idx = 0usize;
    loop {
        let mut end = start;
        let mut sum = 0usize;
        while end < n && (end == start || sum + tok[end] <= window_tokens) {
            sum += tok[end];
            end += 1;
        }
        windows.push(Window {
            idx,
            start_segment: start,
            end_segment: end - 1,
            byte_start: segments[start].byte_start,
            byte_end: segments[end - 1].byte_end,
            text: render(segments, start, end),
        });
        idx += 1;
        if end >= n {
            break;
        }
        let mut s = end;
        let mut back = 0usize;
        while s > start + 1 && back < overlap_tokens {
            s -= 1;
            back += tok[s];
        }
        let next_start = s.max(start + 1);
        start = if next_start >= end { end } else { next_start };
    }
    windows
}
