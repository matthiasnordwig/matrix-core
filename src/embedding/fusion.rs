//! Parameters of the hybrid fusion (vector list + FTS5/BM25 keyword list, see
//! `retrieval.rs::retrieve_hybrid_with`) and the keyword-query cleanup.
//!
//! Why this exists: the keyword list is built from an OR over EVERY word of
//! the question. Function words ("in", "der", "the", "of") match almost every
//! chunk, so a question whose content words do not occur in the corpus (an
//! English question on German text) still yields a full keyword list — of
//! noise. RRF then rewards chunks that sit in BOTH lists far above a vector
//! hit that is missing from the keyword list (rank 5 in both: 2/65 = 0.031;
//! rank 1 in one: 1/61 = 0.016), so correct vector hits drop from rank 1 to
//! 4–6 (measured, `handbuch/core.md` „Embedding-Kandidat …").
//!
//! [`keyword_query`] drops German/English function words before the FTS
//! query is built; [`FusionParams::fts_weight`] scales the keyword list's RRF
//! contribution, [`FusionParams::k`] the RRF dampening. Defaults are the
//! measured production choice (function words dropped, k = 10, weight 1 — the
//! grid over all golden sets and both embedding models: dropping function words
//! is the large lever, k = 10 is never worse than 60 in any cell, a lower
//! keyword weight does not help; numbers in `handbuch/core.md`); the `MATRIX_HYBRID_FUSION` env var (`stop=0|1`,
//! `w=<f32>`, `k=<usize>`, comma-separated) only
//! exists to re-measure alternatives headlessly (`services/examples/embed_compare.rs`).

use std::sync::OnceLock;

/// How the hybrid path builds and weighs its keyword list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FusionParams {
    /// Drop function words from the keyword query.
    pub drop_stopwords: bool,
    /// RRF weight of the keyword list (vector list = 1.0).
    pub fts_weight: f32,
    /// RRF dampening constant. With the standard 60, rank 1 vs. 5 differs by
    /// only ~6 %, so presence in both lists dominates and down-weighting the
    /// keyword list barely helps; a smaller k makes the position count.
    pub k: usize,
}

impl Default for FusionParams {
    fn default() -> Self {
        FusionParams { drop_stopwords: true, fts_weight: 1.0, k: 10 }
    }
}

impl FusionParams {
    /// `stop=0|1`, `w=<f32>`, `k=<usize>`, comma-separated; unknown parts are ignored and
    /// missing ones keep their default.
    pub fn parse(spec: &str) -> Self {
        let mut p = FusionParams::default();
        for part in spec.split(',').map(str::trim) {
            match part.split_once('=') {
                Some(("stop", v)) => p.drop_stopwords = v != "0",
                Some(("k", v)) => {
                    if let Ok(k) = v.parse::<usize>() {
                        p.k = k.clamp(1, 1000);
                    }
                }
                Some(("w", v)) => {
                    if let Ok(w) = v.parse::<f32>() {
                        p.fts_weight = w.clamp(0.0, 2.0);
                    }
                }
                _ => {}
            }
        }
        p
    }

    /// Defaults, or the `MATRIX_HYBRID_FUSION` override (read once per process).
    pub fn current() -> Self {
        static P: OnceLock<FusionParams> = OnceLock::new();
        *P.get_or_init(|| {
            std::env::var("MATRIX_HYBRID_FUSION").map(|s| Self::parse(&s)).unwrap_or_default()
        })
    }
}

/// German and English function words (lowercase). Deliberately only words that
/// never carry a legal identifier — "art", "nr", "abs", numbers and anything
/// with a digit stay in the query.
const STOPWORDS: &[&str] = &[
    // German
    "aber", "alle", "allem", "allen", "aller", "alles", "als", "also", "am", "an", "ander", "andere",
    "anderen", "anderer", "anderes", "auch", "auf", "aus", "bei", "beim", "bin", "bis", "bist", "da",
    "damit", "dann", "das", "dass", "dem", "den", "denn", "der", "des", "die", "dies", "diese",
    "diesem", "diesen", "dieser", "dieses", "doch", "dort", "du", "durch", "ein", "eine", "einem",
    "einen", "einer", "eines", "er", "es", "etwa", "für", "gegen", "hat", "hatte", "hier", "ich", "ihr",
    "ihre", "ihrem", "ihren", "ihrer", "im", "in", "ins", "ist", "ja", "jede", "jedem", "jeden",
    "jeder", "jedes", "kann", "kein", "keine", "können", "man", "mit", "muss", "müssen", "nach",
    "nicht", "noch", "nur", "ob", "oder", "ohne", "sein", "seine", "seinem", "seinen", "seiner",
    "sich", "sie", "sind", "so", "soll", "sollen", "über", "um", "und", "uns", "unter", "vom", "von",
    "vor", "war", "was", "welche", "welchem", "welchen", "welcher", "welches", "wenn", "wer", "werden",
    "wie", "wird", "wo", "zu", "zum", "zur",
    // English
    "a", "about", "after", "all", "also", "an", "and", "any", "are", "as", "at", "be", "been", "before",
    "being", "between", "both", "but", "by", "can", "could", "did", "do", "does", "each", "for",
    "from", "had", "has", "have", "he", "her", "his", "how", "if", "in", "into", "is", "it", "its",
    "may", "more", "must", "no", "not", "of", "on", "or", "other", "our", "shall", "should", "so",
    "some", "such", "than", "that", "the", "their", "them", "then", "there", "these", "they", "this",
    "those", "to", "under", "up", "upon", "was", "we", "were", "what", "when", "where", "which",
    "while", "who", "whom", "why", "will", "with", "within", "would", "you", "your",
];

fn is_stopword(token: &str) -> bool {
    let core: String = token.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    !core.is_empty() && !core.chars().any(|c| c.is_ascii_digit()) && STOPWORDS.contains(&core.as_str())
}

/// The question as handed to the keyword search: with `drop_stopwords`, every
/// whitespace token that is only a function word is removed. If nothing would
/// remain, the original query is kept (an all-function-word query is better
/// matched as typed than not at all).
pub fn keyword_query(raw: &str, drop_stopwords: bool) -> String {
    if !drop_stopwords {
        return raw.to_string();
    }
    let kept: Vec<&str> = raw.split_whitespace().filter(|t| !is_stopword(t)).collect();
    if kept.is_empty() {
        raw.to_string()
    } else {
        kept.join(" ")
    }
}
