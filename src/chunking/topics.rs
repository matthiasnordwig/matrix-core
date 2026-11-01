//! The topics of a long text, for search: the author's paragraphs are read one by one, and
//! neighbouring ones that speak of the same thing form one topic. A topic is the most sensible unit
//! here: a paragraph alone is often too little, the whole mail too much when it has several themes.
//! The passage stays whole for reading and citing; each topic gets its own search vector.
//!
//! Pure: `paragraph_units` cuts, the caller embeds, `topic_groups` groups. A topic ends at a paragraph
//! at the latest when it reaches [`MAX_UNIT_CHARS`], about 2 000 tokens: one vector must still stand
//! for it, and the model's time and memory climb steeply above that (`embedding/onnx.rs`).

use std::ops::Range;

use super::markdown::split_long_text;

/// A paragraph with fewer words is a greeting or a signature: it says nothing alone and goes with
/// its neighbour.
const MIN_UNIT_WORDS: usize = 12;
/// Most text one vector stands for, about 2 000 tokens; a longer paragraph is read by sentences.
pub const MAX_UNIT_CHARS: usize = 8_000;

/// The paragraphs of `text` as byte ranges, short ones joined to their neighbour.
pub fn paragraph_units(text: &str) -> Vec<(usize, usize)> {
    let mut units: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    let mut last_end = 0;
    let mut pos = 0;
    for line in text.split_inclusive('\n') {
        let end = pos + line.len();
        if line.trim().is_empty() {
            if let Some(s) = start.take() {
                units.push((s, last_end));
            }
        } else {
            start.get_or_insert(pos);
            last_end = pos + line.trim_end().len();
        }
        pos = end;
    }
    if let Some(s) = start {
        units.push((s, last_end));
    }
    let mut merged: Vec<(usize, usize)> = Vec::new();
    let mut carry: Option<usize> = None;
    for (s, e) in units {
        let first = carry.take().unwrap_or(s);
        if text[first..e].split_whitespace().count() < MIN_UNIT_WORDS {
            carry = Some(first);
        } else {
            merged.push((first, e));
        }
    }
    if let (Some(s), Some(last)) = (carry, merged.last_mut()) {
        last.1 = text[s..].trim_end().len() + s;
    } else if let Some(s) = carry {
        merged.push((s, s + text[s..].trim_end().len()));
    }
    merged.into_iter().flat_map(|(s, e)| split_long_text(text, s, e, MAX_UNIT_CHARS)).collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    let n = norm(a) * norm(b);
    if n == 0.0 { 0.0 } else { dot / n }
}

/// Normalized mean of vectors of equal length.
pub fn mean_vector(vectors: &[Vec<f32>]) -> Vec<f32> {
    let mut mean = vec![0.0f32; vectors[0].len()];
    for v in vectors {
        mean.iter_mut().zip(v).for_each(|(m, x)| *m += x);
    }
    let norm = mean.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        mean.iter_mut().for_each(|x| *x /= norm);
    }
    mean
}

/// Runs of neighbouring units on one topic: a unit joins the current run while it is at least
/// `min_similarity` close to the run's mean and the run stays within [`MAX_UNIT_CHARS`].
/// `sizes` are the units' lengths in characters.
pub fn topic_groups(vectors: &[Vec<f32>], sizes: &[usize], min_similarity: f32) -> Vec<Range<usize>> {
    let mut groups = Vec::new();
    let mut start = 0;
    let mut size = sizes.first().copied().unwrap_or(0);
    for i in 1..vectors.len() {
        let same_topic = cosine(&mean_vector(&vectors[start..i]), &vectors[i]) >= min_similarity;
        if !same_topic || size + sizes[i] > MAX_UNIT_CHARS {
            groups.push(start..i);
            start = i;
            size = 0;
        }
        size += sizes[i];
    }
    if !vectors.is_empty() {
        groups.push(start..vectors.len());
    }
    groups
}
