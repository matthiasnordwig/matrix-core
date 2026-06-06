//! Index-based assembly. The LLM returns the segment indices where chunks
//! start (plus optional metadata and segments to leave out); we partition the
//! segment list accordingly. No string matching, no overlap, gapless by
//! construction. Ideas distilled from a legal-RAG prompt: definitions own
//! chunk, `abschnitt`/`titel` metadata with parent inheritance, leave-out of
//! TOC/headers/footers, tables row-wise (each row is its own segment).

use std::collections::{BTreeMap, HashSet};

use serde::Deserialize;

use super::segments::split_segments;
use crate::db::models::NewChunk;

/// A chunk start: either a bare index `12` or `{ "i": 12, "abschnitt": …, "titel": … }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum StartItem {
    Index(usize),
    Detailed {
        #[serde(alias = "index", alias = "start", alias = "segment")]
        i: usize,
        #[serde(default)]
        abschnitt: Option<String>,
        #[serde(default)]
        titel: Option<String>,
    },
}

impl StartItem {
    fn parts(&self) -> (usize, Option<String>, Option<String>) {
        match self {
            StartItem::Index(i) => (*i, None, None),
            StartItem::Detailed { i, abschnitt, titel } => (*i, abschnitt.clone(), titel.clone()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LlmChunkResponse {
    #[serde(default)]
    pub starts: Vec<StartItem>,
    #[serde(default)]
    pub leave_out: Vec<usize>,
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.trim().is_empty())
}

/// Apply the collected start indices onto the segments to produce chunks.
pub fn assemble(
    raw: &str,
    context_id: i64,
    document_id: i64,
    responses: &[LlmChunkResponse],
) -> Vec<NewChunk> {
    let segments = split_segments(raw);
    let n = segments.len();
    if n == 0 {
        return Vec::new();
    }

    // Merge starts (first metadata wins) and leave-out segment indices.
    let mut starts: BTreeMap<usize, (Option<String>, Option<String>)> = BTreeMap::new();
    let mut leave_out: HashSet<usize> = HashSet::new();
    for r in responses {
        for s in &r.starts {
            let (i, ab, ti) = s.parts();
            if i < n {
                starts.entry(i).or_insert((ab, ti));
            }
        }
        for &lo in &r.leave_out {
            if lo < n {
                leave_out.insert(lo);
            }
        }
    }
    // Gapless coverage: the document always starts a chunk at segment 0.
    starts.entry(0).or_insert((None, None));

    let idxs: Vec<usize> = starts.keys().copied().collect();
    let mut chunks = Vec::new();
    let mut last_ab: Option<String> = None;
    let mut last_ti: Option<String> = None;

    for (ci, &si) in idxs.iter().enumerate() {
        let end = if ci + 1 < idxs.len() { idxs[ci + 1] } else { n };

        // Concatenate the kept segments (drop leave-out: TOC/headers/footers).
        let mut pieces: Vec<&str> = Vec::new();
        for seg_i in si..end {
            if leave_out.contains(&seg_i) {
                continue;
            }
            pieces.push(&raw[segments[seg_i].byte_start..segments[seg_i].byte_end]);
        }
        if pieces.is_empty() {
            continue; // whole chunk was left out
        }
        let text = pieces.join("\n");

        // Metadata with inheritance of the last valid heading.
        let (ab0, ti0) = starts[&si].clone();
        let ab = non_empty(ab0).or_else(|| last_ab.clone());
        let ti = non_empty(ti0).or_else(|| last_ti.clone());
        last_ab = ab.clone();
        last_ti = ti.clone();
        let metadata = serde_json::json!({ "abschnitt": ab, "titel": ti }).to_string();

        chunks.push(NewChunk {
            context_id,
            document_id,
            chunk_index: chunks.len() as i64,
            char_start: segments[si].byte_start as i64,
            char_end: segments[end - 1].byte_end as i64,
            text,
            signature: None,
            is_omitted: false,
            metadata,
        });
    }
    chunks
}
