//! Segmentation into the smallest sensible unit: the text is broken at every
//! newline AND every sentence end (whichever comes first). This is fixed (not
//! configurable) and handles prose, bullet points and table rows uniformly —
//! a table row is one segment, a bullet is one segment, a prose sentence is one
//! segment (even inside a long unwrapped paragraph).

use serde::{Deserialize, Serialize};

use super::sentences::split_sentences;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub index: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub text: String,
}

/// Split `text` into segments: newline-delimited lines, each further split into
/// sentences. Whitespace-only lines yield no segment. Byte offsets are absolute.
pub fn split_segments(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut line_offset = 0usize;
    for line in text.split_inclusive('\n') {
        for s in split_sentences(line) {
            segments.push(Segment {
                index: segments.len(),
                byte_start: line_offset + s.byte_start,
                byte_end: line_offset + s.byte_end,
                text: s.text,
            });
        }
        line_offset += line.len();
    }
    segments
}
