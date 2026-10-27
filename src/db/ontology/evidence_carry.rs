//! Carrying a document's graph evidence over onto its new chunks — the pure
//! half (no DB). When a document's chunks are made anew (re-chunking, a new
//! image description, the same file deleted and added again), the evidence rows
//! of the graph (`ontology_edge_chunks`, the chunk link of
//! `ontology_edge_reviews`) and the "chunk extracted" marks
//! (`ontology_extracted_chunks`) hang at chunk ids that are about to vanish
//! (`ON DELETE CASCADE`). [`plan_carry`] decides where each of them goes among
//! the new chunks; `evidence_carry_db.rs` reads, writes and wraps it all in the
//! savepoint that replaces the chunks.
//!
//! **Where the old place lies.** Same document text before and after (the
//! usual case: chunk offsets are byte offsets into `documents.extracted_text`)
//! → the old chunk's byte range, and the candidates are the new chunks that
//! overlap it. Changed text (a new image description) → the new chunks whose
//! normalized, lower-cased text contains the old chunk's text or is contained
//! in it.
//!
//! **Which candidate gets a piece of evidence** (first rule that applies; the
//! rule is reported as [`MatchTier`]):
//! 1. the stored quote (`evidence`) is found in a candidate (normalization of
//!    `crate::text_match`) → the candidate with the largest overlap among those;
//!    no candidate at all → any new chunk of the document that contains it;
//! 2. exactly one candidate → that one;
//! 3. the candidate that contains the most of the edge's two node labels
//!    (folded), ties by overlap;
//! 4. the candidate with the largest overlap.
//!
//!    Without any candidate (the new cut lies across the old one), two last
//!    rules before giving up:
//! 5. the quote runs over two new chunks → the chunk where it starts;
//! 6. changed text and no quote found → among the new chunks that name at
//!    least one of the edge's node labels, the one sharing the most words
//!    (folded, 3+ letters) with the old chunk's text. Words alone would hang
//!    a relation onto a chunk that does not name its ends (an image text that
//!    kept only the other half of the old chunk). Same text: the old range is
//!    authoritative, nothing is guessed.
//!
//!    What still finds no place is not placed: counted, never silent. The
//!    schema has no evidence row without a chunk (`chunk_id NOT NULL`).
//!
//! **Extracted rule.** A new chunk counts as extracted only when every byte of
//! it was already read by the extractor: same text → the old chunks overlapping
//! it are all extracted and together cover its whole range; changed text → its
//! folded text lies inside one extracted old chunk. Everything else stays open,
//! so a continued extraction does exactly the gaps (e.g. a new chunk that
//! reaches into a formerly omitted passage is extracted again as a whole).
use std::collections::HashSet;

use crate::text_match::{contains_evidence_normalized, fold, normalize, split_ellipsis_fragments};

/// One chunk of the document as it was before the replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OldChunk {
    /// Old chunk id (for parked evidence: the parked row's id).
    pub id: i64,
    pub char_start: i64,
    pub char_end: i64,
    pub text: String,
    pub extracted: bool,
}

/// What a piece of evidence is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    /// A row of `ontology_edge_chunks`.
    Edge,
    /// The chunk link of the `ontology_edge_reviews` row with this id.
    Review(i64),
}

/// One piece of evidence hanging at an old chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OldEvidence {
    pub kind: EvidenceKind,
    pub edge_id: i64,
    /// [`OldChunk::id`] it hung at.
    pub chunk_id: i64,
    /// The stored quote, if any.
    pub evidence: Option<String>,
    /// Labels of the edge's source and target node.
    pub labels: [String; 2],
}

/// A document's chunks and graph evidence before the replacement.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocumentGraph {
    pub context_id: i64,
    pub document_id: i64,
    /// [`text_fingerprint`] of the document text the old chunks were cut from.
    pub text_fingerprint: String,
    pub chunks: Vec<OldChunk>,
    pub evidence: Vec<OldEvidence>,
}

impl DocumentGraph {
    /// Nothing to carry: no evidence and no extracted chunk.
    pub fn is_empty(&self) -> bool {
        self.evidence.is_empty() && !self.chunks.iter().any(|c| c.extracted)
    }
}

/// A chunk of the document after the replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewChunkSpan {
    pub id: i64,
    pub char_start: i64,
    pub char_end: i64,
    pub text: String,
    pub is_omitted: bool,
}

/// Which rule placed a piece of evidence (see the module comment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchTier {
    EvidenceText,
    SinglePlace,
    Labels,
    Overlap,
}

/// Where everything goes: `placed` = (index into `DocumentGraph::evidence`,
/// new chunk id, rule), `unplaced` = indexes without a place, `extracted` = new
/// chunk ids to mark as extracted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CarryPlan {
    pub placed: Vec<(usize, i64, MatchTier)>,
    pub unplaced: Vec<usize>,
    pub extracted: Vec<i64>,
}

/// Counts of one carry-over, for the app log and the restore tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EvidenceCarryReport {
    pub old_chunks: usize,
    pub new_chunks: usize,
    /// Pieces of evidence before (edge evidence + review links).
    pub evidence: usize,
    pub by_text: usize,
    pub by_place: usize,
    pub by_labels: usize,
    pub by_overlap: usize,
    /// No new chunk for it: dropped.
    pub unplaced: usize,
    /// Placed onto a new chunk the same edge already had evidence at.
    pub merged: usize,
    /// Its edge (or review) does not exist (any more) in the target database
    /// (restore from a backup only).
    pub edge_missing: usize,
    /// Its edge id exists but belongs to another context or joins other nodes
    /// (labels differ) — a foreign edge, not touched.
    pub edge_mismatch: usize,
    pub extracted_before: usize,
    pub extracted_after: usize,
}

impl EvidenceCarryReport {
    /// Pieces of evidence that found a new chunk.
    pub fn placed(&self) -> usize {
        self.by_text + self.by_place + self.by_labels + self.by_overlap
    }

    /// Nothing was there to carry.
    pub fn is_empty(&self) -> bool {
        self.evidence == 0 && self.extracted_before == 0
    }

    /// One line for the app log.
    pub fn summary(&self) -> String {
        format!(
            "evidence {placed}/{total} carried over (quote {t}, single place {p}, labels {l}, overlap {o}), \
             {u} without a place, {m} merged, {e} edge gone, {f} foreign edge; extracted chunks {xb} → {xa} of {n}",
            placed = self.placed(),
            total = self.evidence,
            t = self.by_text,
            p = self.by_place,
            l = self.by_labels,
            o = self.by_overlap,
            u = self.unplaced,
            m = self.merged,
            e = self.edge_missing,
            f = self.edge_mismatch,
            xb = self.extracted_before,
            xa = self.extracted_after,
            n = self.new_chunks,
        )
    }
}

/// Stable fingerprint of a document text (FNV-1a 64, hex) — tells whether the
/// old chunk offsets still point into the same text, and finds the parked
/// evidence of a deleted document when the same file comes back.
pub fn text_fingerprint(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}-{}", text.len())
}

fn overlap(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.1.min(b.1) - a.0.max(b.0)).max(0)
}

/// Shortest folded text a text-located candidate may have (shorter ones — a
/// lone heading — would match almost anywhere).
const MIN_FOLDED: usize = 20;

/// Decide where the evidence and the extracted marks of `old` go among `new`
/// (rules in the module comment). `same_text`: the document text is unchanged,
/// so old byte ranges are valid in the new chunks' coordinates.
pub fn plan_carry(old: &DocumentGraph, same_text: bool, new: &[NewChunkSpan]) -> CarryPlan {
    let live: Vec<&NewChunkSpan> = new.iter().filter(|n| !n.is_omitted).collect();
    let folded: Vec<String> = live.iter().map(|n| fold(&n.text)).collect();
    let normalized: Vec<String> = live.iter().map(|n| normalize(&n.text).0).collect();
    let old_folded: Vec<String> = if same_text { Vec::new() } else { old.chunks.iter().map(|c| fold(&c.text)).collect() };

    // Candidates of each old chunk: (index into `live`, overlap measure).
    let candidates = |oi: usize| -> Vec<(usize, i64)> {
        let o = &old.chunks[oi];
        if same_text {
            let range = (o.char_start, o.char_end.max(o.char_start + 1));
            live.iter()
                .enumerate()
                .map(|(k, n)| (k, overlap(range, (n.char_start, n.char_end))))
                .filter(|&(_, ov)| ov > 0)
                .collect()
        } else {
            let of = &old_folded[oi];
            folded
                .iter()
                .enumerate()
                .filter(|(_, nf)| {
                    let shorter = nf.len().min(of.len());
                    shorter >= MIN_FOLDED && (nf.contains(of.as_str()) || of.contains(nf.as_str()))
                })
                .map(|(k, nf)| (k, nf.len().min(of.len()) as i64))
                .collect()
        }
    };
    let old_index = |id: i64| old.chunks.iter().position(|c| c.id == id);
    let best = |c: &[(usize, i64)], score: &dyn Fn(usize) -> usize| -> usize {
        // Highest score, then largest overlap, then earliest chunk.
        c.iter().copied().max_by(|a, b| score(a.0).cmp(&score(b.0)).then(a.1.cmp(&b.1)).then(b.0.cmp(&a.0))).map(|x| x.0).unwrap_or(0)
    };

    let mut plan = CarryPlan::default();
    let mut cache: Vec<Option<Vec<(usize, i64)>>> = vec![None; old.chunks.len()];
    // Rules 5 and 6, built only when needed.
    let mut joined: Option<(String, Vec<usize>)> = None;
    let mut words: Option<Vec<HashSet<String>>> = None;
    for (ei, ev) in old.evidence.iter().enumerate() {
        let cands = match old_index(ev.chunk_id) {
            Some(oi) => cache[oi].get_or_insert_with(|| candidates(oi)).clone(),
            None => Vec::new(),
        };
        let quote = ev.evidence.as_deref().filter(|q| !q.trim().is_empty());
        let has_quote = |k: usize| quote.is_some_and(|q| contains_evidence_normalized(&normalized[k], q));
        let pick: Option<(usize, MatchTier)> = {
            let quoted: Vec<(usize, i64)> = cands.iter().copied().filter(|&(k, _)| has_quote(k)).collect();
            if !quoted.is_empty() {
                Some((best(&quoted, &|_| 0), MatchTier::EvidenceText))
            } else if cands.is_empty() {
                quote.and_then(|_| (0..live.len()).find(|&k| has_quote(k))).map(|k| (k, MatchTier::EvidenceText))
            } else if cands.len() == 1 {
                Some((cands[0].0, MatchTier::SinglePlace))
            } else {
                let labels: Vec<String> = ev.labels.iter().map(|l| fold(l)).filter(|l| l.chars().count() >= 2).collect();
                let score = |k: usize| labels.iter().filter(|l| folded[k].contains(l.as_str())).count();
                let k = best(&cands, &score);
                Some((k, if score(k) > 0 { MatchTier::Labels } else { MatchTier::Overlap }))
            }
        };
        let pick = pick.or_else(|| {
            let joined = joined.get_or_insert_with(|| join_normalized(&normalized));
            if let Some(k) = quote.and_then(|q| quote_start(joined, q)) {
                return Some((k, MatchTier::EvidenceText));
            }
            if same_text {
                return None;
            }
            let words = words.get_or_insert_with(|| live.iter().map(|n| word_set(&n.text)).collect());
            let labels: Vec<String> = ev.labels.iter().map(|l| fold(l)).filter(|l| l.chars().count() >= 2).collect();
            let named = |k: usize| labels.iter().any(|l| folded[k].contains(l.as_str()));
            old_index(ev.chunk_id).and_then(|oi| most_shared_words(&old.chunks[oi].text, words, &named)).map(|k| (k, MatchTier::Labels))
        });
        match pick {
            Some((k, tier)) => plan.placed.push((ei, live[k].id, tier)),
            None => plan.unplaced.push(ei),
        }
    }

    for (k, n) in live.iter().enumerate() {
        let done = if same_text {
            let (s, e) = (n.char_start, n.char_end);
            let mut touching: Vec<&OldChunk> =
                old.chunks.iter().filter(|o| overlap((o.char_start, o.char_end), (s, e)) > 0).collect();
            touching.sort_by_key(|o| o.char_start);
            let mut covered = s;
            let mut all = !touching.is_empty();
            for o in &touching {
                all &= o.extracted && o.char_start <= covered;
                covered = covered.max(o.char_end);
            }
            all && covered >= e && e > s
        } else {
            folded[k].len() >= MIN_FOLDED
                && old.chunks.iter().zip(&old_folded).any(|(o, of)| o.extracted && of.contains(folded[k].as_str()))
        };
        if done {
            plan.extracted.push(n.id);
        }
    }
    plan
}

/// The normalized texts of the new chunks joined by one space, with the start
/// offset of each in the joined text.
fn join_normalized(normalized: &[String]) -> (String, Vec<usize>) {
    let mut text = String::new();
    let mut starts = Vec::with_capacity(normalized.len());
    for n in normalized {
        if !text.is_empty() {
            text.push(' ');
        }
        starts.push(text.len());
        text.push_str(n.trim());
    }
    (text, starts)
}

/// Rule 5: the chunk (index) where `quote` starts, when it is found only across chunks.
fn quote_start((text, starts): &(String, Vec<usize>), quote: &str) -> Option<usize> {
    if !contains_evidence_normalized(text, quote) {
        return None;
    }
    let first = normalize(split_ellipsis_fragments(quote).first()?).0;
    let at = text.find(&first)?;
    starts.iter().rposition(|&s| s <= at)
}

/// Folded words of 3+ letters.
fn word_set(text: &str) -> HashSet<String> {
    fold(text).split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3).map(str::to_string).collect()
}

/// Rule 6: among the chunks (index) `named` accepts, the one sharing the most
/// words with `old_text`; ties → the earliest.
fn most_shared_words(old_text: &str, words: &[HashSet<String>], named: &dyn Fn(usize) -> bool) -> Option<usize> {
    let old = word_set(old_text);
    let mut best: Option<(usize, usize)> = None;
    for (k, w) in words.iter().enumerate().filter(|(k, _)| named(*k)) {
        let shared = w.intersection(&old).count();
        if shared > 0 && best.is_none_or(|(_, b)| shared > b) {
            best = Some((k, shared));
        }
    }
    best.map(|(k, _)| k)
}
