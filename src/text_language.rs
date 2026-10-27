//! Language of a passage, a document or a search query by function words (`der`, `the`, `pour`, …),
//! as ISO 639-1 code (`de`, `en`, `fr`, `es`, `it`, `nl`, `pt`). Pure, no DB, no dependency.
//!
//! Users: the hybrid fusion (`embedding/retrieval.rs` — the keyword list gets less weight when the
//! question's language differs from the language of the passages the vector search found, see
//! `embedding/fusion.rs`), and the app via `matrix_services::text_language` (language of each
//! search hit and document in the MCP results, function words of the search snippet).

/// Function words that tell the common document languages apart. No word stands in two lists
/// (test `passage_word_lists_are_disjoint`), so a word counts for one language only — words
/// several of them share ("de", "la", "que", "ou", …) are left out.
const PASSAGE_WORDS: &[(&str, &[&str])] = &[
    ("de", &["der", "die", "das", "und", "nicht", "mit", "für", "auf", "ist", "sind", "wird", "werden", "eine", "einer", "dem", "den", "von", "zu", "sich", "oder"]),
    ("en", &["the", "and", "of", "to", "is", "are", "for", "with", "that", "this", "which", "shall", "be", "by", "from", "or"]),
    ("fr", &["le", "les", "des", "du", "et", "est", "pour", "dans", "qui", "sur", "sont", "aux", "ne", "pas", "ou", "au"]),
    ("es", &["el", "los", "las", "del", "y", "es", "por", "lo", "su", "sus", "o", "como", "este", "esta"]),
    ("it", &["il", "gli", "della", "delle", "dei", "che", "è", "per", "non", "sono", "nel", "alla", "degli", "nella"]),
    ("nl", &["het", "een", "van", "en", "niet", "zijn", "wordt", "worden", "voor", "met", "op", "dat", "aan", "bij"]),
    ("pt", &["os", "dos", "não", "uma", "com", "em", "ao", "pelo", "pela", "são", "às", "seu"]),
];

/// `word` (lower case) is a function word of one of the document languages above (`der`, `the`,
/// `pour`, …): no content to search for.
pub fn is_function_word(word: &str) -> bool {
    PASSAGE_WORDS.iter().any(|(_, words)| words.contains(&word))
}

/// The language whose function words occur most in the first 4,000 characters of `text`, at least
/// `min_hits` times and 1.5× the runner-up; else `None`.
fn by_function_words(text: &str, min_hits: usize) -> Option<&'static str> {
    let mut score = [0usize; PASSAGE_WORDS.len()];
    let sample: String = text.chars().take(4000).collect();
    for raw in sample.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()) {
        let w = raw.to_lowercase();
        if let Some(i) = PASSAGE_WORDS.iter().position(|(_, words)| words.contains(&w.as_str())) {
            score[i] += 1;
        }
    }
    let mut ranked: Vec<(usize, usize)> = score.iter().copied().enumerate().collect();
    ranked.sort_by_key(|r| std::cmp::Reverse(r.1));
    let (best, n) = ranked[0];
    (n >= min_hits && n * 2 >= ranked[1].1 * 3).then_some(PASSAGE_WORDS[best].0)
}

/// Language of a passage or document text, or `None` when too short or mixed: at least 5 function
/// words of one language and 1.5× the runner-up. Looks at no more than the first 4,000 characters.
pub fn passage_language(text: &str) -> Option<&'static str> {
    by_function_words(text, 5)
}

/// Language of a search query (a question of one sentence): as [`passage_language`], but one
/// function word suffices when no other language has one — „Wie lange muss ein Hersteller nach
/// der EU-Cyberresilienzverordnung …" has only „der". Measured: two words left a third of the
/// German questions of `scripts/bench_search.py` unknown.
pub fn query_language(text: &str) -> Option<&'static str> {
    by_function_words(text, 1)
}

/// Language of a whole document: [`passage_language`] of three samples of a few KB (start,
/// middle, three quarters — skips a title page in another language), never the whole text; the
/// majority, else the first known.
pub fn document_language(text: &str) -> Option<&'static str> {
    const SAMPLE_BYTES: usize = 16_000;
    let boundary = |mut i: usize| {
        while !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let samples: Vec<Option<&str>> = [0, text.len() / 2, text.len() * 3 / 4]
        .iter()
        .map(|&at| {
            let start = boundary(at);
            passage_language(&text[start..boundary((start + SAMPLE_BYTES).min(text.len()))])
        })
        .collect();
    let known: Vec<&str> = samples.iter().flatten().copied().collect();
    known.iter().find(|l| known.iter().filter(|m| m == l).count() >= 2).or(known.first()).copied()
}

#[cfg(test)]
mod tests;
