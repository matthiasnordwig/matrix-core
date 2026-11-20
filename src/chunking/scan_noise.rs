//! Recognition noise in place of punctuation in scanned text (pure rules, no OCR engine here).
//!
//! Apple Vision reads marks it has no model for — guillemets, the double hyphen of older print,
//! a hyphen at the line end — as runs of Vietnamese letters with a dot below or a hook
//! ("bisạẠạ", "beiại merkt"): letters of the block U+1EA0–U+1EF9 ([`is_noise_letter`]), often
//! with an `i` before or after them. In a text that is not written in Vietnamese these letters
//! are otherwise absent, so a run of them is noise ([`repair_noise`], gated by [`is_foreign`]: the
//! block is below [`MAX_SHARE`] of the letters of the whole run of pages).
//!
//! A noise run is the noise letters plus every `i`/`I` right after one (not the `I` of "Ich").
//! It is replaced:
//! - at the end of a line whose paragraph continues with a lower-case word: by a hyphen, so the
//!   joining (`scan_text::join_kind`) glues the word — unless the text itself shows that the two
//!   parts are separate words and the glued form is not one (then a space: "mitkommen?« fragte");
//! - anywhere else: by a space (a guillemet or hyphen between two words), dropped at a line edge.
//!
//! An `i` right before the run is mostly noise as well ("Türiạ fragte", "geiẠạ schlafen"), but
//! words also end in `i` ("frei"). It is dropped unless the text's own words say otherwise
//! ([`Vocabulary`]: the form with `i` occurs elsewhere and the one without does not).
//! Tests: `scan_noise_tests.rs`.

use std::collections::HashMap;

use super::scan_text::PageLine;

/// Above this share of the letters the noise block is a script in use (Vietnamese), not noise.
pub const MAX_SHARE: f64 = 0.01;

/// A letter of the Vietnamese block U+1EA0–U+1EF9 (vowels with dot below, hook, …).
pub fn is_noise_letter(c: char) -> bool {
    matches!(c as u32, 0x1EA0..=0x1EF9)
}

fn is_i(c: char) -> bool {
    matches!(c, 'i' | 'I' | 'ı')
}

/// Whether the noise letters are foreign to `texts`: some occur, and they stay below
/// [`MAX_SHARE`] of all letters.
pub fn is_foreign<'a>(texts: impl IntoIterator<Item = &'a str>) -> bool {
    let (mut noise, mut all) = (0usize, 0usize);
    for c in texts.into_iter().flat_map(str::chars).filter(|c| c.is_alphabetic()) {
        all += 1;
        noise += usize::from(is_noise_letter(c));
    }
    noise > 0 && (noise as f64) < MAX_SHARE * all as f64
}

/// Byte ranges of the noise runs in `t`: noise letters and the `i`s right after them (an `I`
/// followed by a lower-case letter starts the next word: "ạIch").
fn runs(t: &str) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut in_run = false;
    let mut chars = t.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let end = at + c.len_utf8();
        let word_start = c == 'I' && chars.peek().is_some_and(|&(_, n)| n.is_lowercase());
        if is_noise_letter(c) || (in_run && is_i(c) && !word_start) {
            match out.last_mut() {
                Some(last) if in_run => last.1 = end,
                _ => out.push((at, end)),
            }
            in_run = true;
        } else {
            in_run = false;
        }
    }
    out
}

/// Lower-case word counts of a text with its noise runs blanked.
pub struct Vocabulary(HashMap<String, usize>);

impl Vocabulary {
    pub fn of<'a>(texts: impl IntoIterator<Item = &'a str>) -> Self {
        let mut counts = HashMap::new();
        for t in texts {
            let mut clean = t.to_string();
            for (s, e) in runs(t).into_iter().rev() {
                clean.replace_range(s..e, " ");
            }
            for w in clean.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()) {
                *counts.entry(w.to_lowercase()).or_insert(0) += 1;
            }
        }
        Vocabulary(counts)
    }

    fn count(&self, w: &str) -> usize {
        self.0.get(&w.to_lowercase()).copied().unwrap_or(0)
    }
}

/// The word ending right before byte `at` of `t` (letters only).
fn word_before(t: &str, at: usize) -> &str {
    let start = t[..at].char_indices().rev().take_while(|(_, c)| c.is_alphabetic()).last().map_or(at, |(i, _)| i);
    &t[start..at]
}

/// Whether the `i` closing `word` (right before a noise run) is a letter of the word, not noise.
/// `word` itself counts once in `voc` (this very place), so it needs a second occurrence.
fn keeps_i(word: &str, voc: &Vocabulary) -> bool {
    let Some(stem) = word.strip_suffix(is_i) else { return false };
    voc.count(word) >= 2 && voc.count(stem) == 0
}

/// How a noise run at the end of a line whose paragraph goes on with `next` (lower-case word)
/// ends the line: `(keep the i before it, hyphen)`.
fn line_end(word: &str, next: &str, voc: &Vocabulary) -> (bool, bool) {
    let stem = word.strip_suffix(is_i);
    let glued = |w: &str| voc.count(&format!("{w}{next}")) >= 1;
    if glued(word) && !stem.is_some_and(glued) {
        return (true, true);
    }
    if stem.is_some_and(glued) {
        return (false, true);
    }
    let keep = keeps_i(word, voc);
    let left = match stem {
        Some(stem) if !keep => stem,
        _ => word,
    };
    // `left` is in `voc` once from this place unless the i was dropped.
    let left_is_word = voc.count(left) >= if left == word { 2 } else { 1 };
    // `next` likewise counts once from the next line.
    (keep, !(left_is_word && voc.count(next) >= 2))
}

/// Replace the noise runs of one line. `next` = the first word of the line that continues this
/// paragraph, if any. Returns the new text and the number of runs replaced.
fn repair_line(t: &str, next: Option<&str>, voc: &Vocabulary) -> (String, usize) {
    let found = runs(t);
    let mut out = t.to_string();
    for &(s, e) in found.iter().rev() {
        let word = word_before(t, s);
        let at_end = t[e..].trim().is_empty();
        let next_lower = next.filter(|n| n.chars().next().is_some_and(char::is_lowercase));
        let (keep, hyphen) = match next_lower {
            Some(n) if at_end && !word.is_empty() => line_end(word, n, voc),
            _ => (keeps_i(word, voc), false),
        };
        let i_len = word.chars().last().filter(|&c| is_i(c) && !keep && word.chars().count() > 1).map_or(0, char::len_utf8);
        out.replace_range(s - i_len..e, if hyphen { "-" } else { " " });
    }
    (out.split_whitespace().collect::<Vec<_>>().join(" "), found.len())
}

/// Replace the noise runs in the lines of a run of pages (see the module comment); nothing
/// changes when the noise letters are not foreign to the text. Returns the runs replaced.
pub fn repair_noise(pages: &mut [(i64, Vec<PageLine>)]) -> usize {
    let texts = || pages.iter().flat_map(|(_, l)| l.iter().map(|x| x.text.as_str()));
    if !is_foreign(texts()) {
        return 0;
    }
    let voc = Vocabulary::of(texts());
    let flat: Vec<(usize, usize)> = pages.iter().enumerate().flat_map(|(p, (_, l))| (0..l.len()).map(move |i| (p, i))).collect();
    let mut replaced = 0;
    for (k, &(p, i)) in flat.iter().enumerate() {
        if !pages[p].1[i].text.chars().any(is_noise_letter) {
            continue;
        }
        let next = flat.get(k + 1).map(|&(q, j)| &pages[q].1[j]).filter(|l| !l.para_start());
        let next_word = next.map(|l| word_before(&l.text, l.text.find(|c: char| !c.is_alphabetic()).unwrap_or(l.text.len())).to_string());
        let (text, n) = repair_line(&pages[p].1[i].text, next_word.as_deref().filter(|w| !w.is_empty()), &voc);
        pages[p].1[i].text = text;
        replaced += n;
    }
    replaced
}
