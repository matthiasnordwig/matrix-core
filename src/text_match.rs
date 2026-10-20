//! Text normalization for locating quoted passages in chunk texts: PDF
//! line-break dehyphenation and whitespace collapse (case preserved), plus the
//! "fragment … fragment" ellipsis convention LLMs use for bridging quote gaps.
//! One definition for every caller: the evidence matcher and the citation check
//! of the app (`ontology/extract/evidence_match.rs`, `citations.rs`, which add
//! span mapping and fuzzy search on top) and the carrying of graph evidence onto
//! new chunks ([`crate::db::ontology::evidence_carry`]).
//! `scripts/eval_extraction.py` mirrors this normalization — keep both in sync.
//! Pure, no DB.

/// Normalizes text for matching and returns, for every byte in the normalized
/// string, the corresponding byte offset in the original string (so matches
/// found in normalized space can be mapped back). Normalization steps:
/// (a) dehyphenation: a `-` followed by optional spaces/tabs, a newline, and
///     optional spaces/tabs is removed entirely (PDF line-wrap hyphens);
/// (b) every run of whitespace (including newlines) collapses to a single
///     space;
/// (c) case is preserved — capitalization is a signal, not noise.
pub fn normalize(text: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(text.len());
    // index_map[k] = original byte offset that output byte k "belongs to"
    // (i.e. the start byte of the original char/whitespace-run it came from).
    let mut index_map: Vec<usize> = Vec::with_capacity(text.len());

    let char_indices: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0usize;
    while i < char_indices.len() {
        let (byte_pos, ch) = char_indices[i];

        // (a) Dehyphenation: '-' + optional [ \t]* + '\n' + optional [ \t]*.
        if ch == '-' {
            let mut j = i + 1;
            // optional spaces/tabs before the newline
            while j < char_indices.len() && (char_indices[j].1 == ' ' || char_indices[j].1 == '\t') {
                j += 1;
            }
            if j < char_indices.len() && char_indices[j].1 == '\n' {
                j += 1;
                // optional spaces/tabs after the newline
                while j < char_indices.len() && (char_indices[j].1 == ' ' || char_indices[j].1 == '\t') {
                    j += 1;
                }
                // Drop the hyphen + all consumed whitespace/newline entirely —
                // the word continues seamlessly, no separator inserted.
                i = j;
                continue;
            }
        }

        // (b) Whitespace run (that isn't the dehyphenation case above)
        // collapses to a single space.
        if ch.is_whitespace() {
            out.push(' ');
            index_map.push(byte_pos);
            let mut j = i + 1;
            while j < char_indices.len() && char_indices[j].1.is_whitespace() {
                j += 1;
            }
            i = j;
            continue;
        }

        // (c) Regular character: copy through as-is, case preserved. Every
        // UTF-8 byte of the output char maps back to this char's start byte.
        out.push(ch);
        for _ in 0..ch.len_utf8() {
            index_map.push(byte_pos);
        }
        i += 1;
    }

    debug_assert_eq!(out.len(), index_map.len());
    (out, index_map)
}

/// Shared normalized-substring test: is `needle` contained in `haystack`
/// after both are run through `normalize` (whitespace-collapse + PDF
/// dehyphenation, case preserved)? Empty `needle` → false.
pub fn normalized_contains(haystack: &str, needle: &str) -> bool {
    let (norm_needle, _) = normalize(needle);
    if norm_needle.is_empty() {
        return false;
    }
    let (norm_haystack, _) = normalize(haystack);
    norm_haystack.contains(&norm_needle)
}

/// Splits evidence text on the ellipsis convention ("…" or "...", optionally
/// surrounded by spaces) into non-empty fragments, in order.
pub fn split_ellipsis_fragments(evidence: &str) -> Vec<&str> {
    let mut fragments = Vec::new();
    let mut rest = evidence;
    loop {
        let next = rest.find("…").map(|p| (p, "…".len())).or_else(|| {
            rest.find("...").map(|p| (p, 3))
        });
        match next {
            Some((pos, marker_len)) => {
                let (before, after) = rest.split_at(pos);
                let frag = before.trim();
                if !frag.is_empty() {
                    fragments.push(frag);
                }
                rest = after[marker_len..].trim_start();
            }
            None => {
                let frag = rest.trim();
                if !frag.is_empty() {
                    fragments.push(frag);
                }
                break;
            }
        }
    }
    fragments
}

/// Whether every ellipsis fragment of `evidence` occurs in `haystack`, in
/// order and after normalization — the yes/no form of the app's
/// `match_evidence`. Blank evidence → false.
pub fn contains_evidence(haystack: &str, evidence: &str) -> bool {
    contains_evidence_normalized(&normalize(haystack).0, evidence)
}

/// [`contains_evidence`] for a haystack that is already [`normalize`]d (saves
/// normalizing a chunk text again for every quote looked up in it).
pub fn contains_evidence_normalized(normalized_haystack: &str, evidence: &str) -> bool {
    let fragments = split_ellipsis_fragments(evidence);
    if fragments.is_empty() {
        return false;
    }
    let mut from = 0usize;
    for f in fragments {
        let (needle, _) = normalize(f);
        if needle.is_empty() {
            return false;
        }
        match normalized_haystack[from..].find(&needle) {
            Some(p) => from += p + needle.len(),
            None => return false,
        }
    }
    true
}

/// [`normalize`] plus lower-casing, for comparing node labels with chunk text
/// (labels are often written in another case than the passage).
pub fn fold(text: &str) -> String {
    normalize(text).0.to_lowercase()
}

#[cfg(test)]
mod tests;
