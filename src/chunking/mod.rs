//! Deterministic prompt-based chunking: sentence segmentation → sliding-window
//! pre-chunks → (LLM, injected by the caller) → signature assembly.
//!
//! This module is pure and offline. The remote-LLM call is **not** here: the
//! caller fetches per-window boundary signatures and hands the parsed
//! [`LlmChunkResponse`]s to [`assemble`]. That keeps the engine testable and
//! the single network egress isolated in the Tauri bridge.

pub mod segments;
pub mod sentences;
pub mod signatures;
pub mod window;
pub mod structural;

pub use segments::{split_segments, Segment};
pub use sentences::{split_sentences, Sentence};
pub use signatures::{assemble, LlmChunkResponse, StartItem};
pub use window::{build_windows, estimate_tokens, Window};

/// Pre-chunking stage: split into segments (line- or sentence-fine) and build
/// token-budgeted overlapping windows whose text is the indexed render.
pub fn prepare_windows(raw: &str, window_tokens: usize, overlap_ratio: f64) -> Vec<Window> {
    let segments = split_segments(raw);
    build_windows(&segments, window_tokens, overlap_ratio)
}

#[cfg(test)]
mod tests;
