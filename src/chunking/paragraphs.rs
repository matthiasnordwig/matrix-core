//! The paragraphs of a mail passage that get a vector of their own (services
//! `commands/paragraph_vectors.rs`): of a long mail the reranker then reads the paragraph closest to
//! the question instead of the header (`commands/rerank_excerpt.rs`). Each paragraph is taken alone,
//! nothing is merged: neighbouring paragraphs of one mail lie too close (median cosine 0.76) for a
//! threshold to tell themes apart (`handbuch/core.md`).
//!
//! Pure: [`paragraph_units`] cuts, the caller embeds. A paragraph is the text between blank lines.
//! Left out are paragraphs under `min_words` words (greetings, short lines) and those that are no text
//! of their own (`mail::is_boilerplate`: header blocks, signatures, footers). A passage with fewer than
//! two such paragraphs gets none: its start or its own vector already stands for that one paragraph.

use super::markdown::split_long_text;

/// Most text for one model call, about 2 000 tokens; a longer paragraph is read by sentences.
pub const MAX_UNIT_CHARS: usize = 8_000;

/// Byte ranges of the paragraphs of `text` that get a vector, in text order; empty below two.
pub fn paragraph_units(text: &str, min_words: usize) -> Vec<(usize, usize)> {
    let mut units = Vec::new();
    let mut start: Option<usize> = None;
    let mut last_end = 0;
    let mut pos = 0;
    let close = |s: usize, e: usize, units: &mut Vec<(usize, usize)>| {
        let para = &text[s..e];
        if para.split_whitespace().count() >= min_words && !super::mail::is_boilerplate(para) {
            units.extend(split_long_text(text, s, e, MAX_UNIT_CHARS));
        }
    };
    for line in text.split_inclusive('\n') {
        if line.trim().is_empty() {
            if let Some(s) = start.take() {
                close(s, last_end, &mut units);
            }
        } else {
            start.get_or_insert(pos);
            last_end = pos + line.trim_end().len();
        }
        pos += line.len();
    }
    if let Some(s) = start {
        close(s, last_end, &mut units);
    }
    if units.len() < 2 {
        units.clear();
    }
    units
}

