//! Offline token estimate of a text in any language ([`estimate_tokens`]) — the one estimate
//! of Matrix: the LLM chunking windows here (`chunking::window`) and, in the app, the context
//! budget and the rate-limiter reservations (`commands::estimate_tokens_padded`) count with it.
//! No tokenizer is loaded: the text is split into runs (a word of one script, a digit, a
//! punctuation mark, whitespace) and each run is priced by its script.
//!
//! Calibration (2026-10-21, `handbuch/chat.md` „Kontextfenster"): 645 text blocks — German
//! and English chunks of the bundled samples, plus the UI catalogs of 18 languages — counted
//! with Apple's on-device tokenizer (`SystemLanguageModel.tokenCount`) and Qwen3's (llama.cpp);
//! the target was the larger of the two counts. Per language group the 5th percentile of
//! estimate / real is ≥ 1.0 (only tables of PDF debris: 0.98), the median 1.04 (English) to
//! 1.36 (Japanese). The former flat chars/4 × 1.2 was right for English only: German chunks
//! went down to 0.72, Arabic 0.70, Japanese/Korean/Chinese ~0.45, Hindi 0.32, number tables 0.34.
//!
//! What drives the counts: English words are mostly one token each, words of other Latin
//! languages split sooner (and letters with diacritics cost extra); every digit is a token, as
//! is the space before a digit; Cyrillic and Arabic words split after a few letters; Indic
//! scripts, CJK and Hangul cost about a token per character.

/// English-ness: share of these words among a text's Latin words (≥ [`ENGLISH_SHARE_PCT`] %).
/// Measured: English blocks 6.6–15 %, the other Latin languages ≤ 0.5 % (words like "is", "of",
/// "to" are left out: Dutch and Polish use them too).
const ENGLISH_MARKERS: &[&str] = &["the", "and", "that", "with", "this", "from", "which"];
const ENGLISH_SHARE_PCT: usize = 4;

/// Latin word: 1 token for the first `free` letters, then one per `rate` letters.
const ENGLISH_WORD: (f64, f64) = (7.5, 3.5);
const LATIN_WORD: (f64, f64) = (3.5, 2.5);
/// Extra per non-ASCII Latin letter (ä, ł, ş, ệ …).
const DIACRITIC: f64 = 0.4;
/// Cyrillic/Greek and Arabic/Hebrew words: first `LATIN_WORD.0` letters free, then one per rate.
const CYRILLIC_RATE: f64 = 2.25;
const ARABIC_RATE: f64 = 1.0;
/// Per character.
const ABUGIDA: f64 = 1.2;
const CJK: f64 = 0.9;
const HANGUL: f64 = 0.9;
const SYMBOL: f64 = 1.5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Space,
    Digit,
    Punct,
    Symbol,
    Latin,
    Cyrillic,
    Arabic,
    Abugida,
    Cjk,
    Hangul,
}

impl Kind {
    fn of(c: char) -> Kind {
        let o = c as u32;
        match o {
            _ if c.is_whitespace() => Kind::Space,
            0x30..=0x39 => Kind::Digit,
            0..=0x7F if c.is_ascii_alphabetic() => Kind::Latin,
            0..=0x7F => Kind::Punct,
            0xC0..=0x24F | 0x1E00..=0x1EFF if c.is_alphabetic() => Kind::Latin,
            0x370..=0x52F => Kind::Cyrillic,
            0x590..=0x8FF => Kind::Arabic,
            0x900..=0x109F => Kind::Abugida,
            0x3000..=0x30FF | 0x3400..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF => Kind::Cjk,
            0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => Kind::Hangul,
            _ => Kind::Symbol,
        }
    }

    /// Runs of these kinds are one piece; punctuation and symbols count per character.
    fn joins(self) -> bool {
        !matches!(self, Kind::Punct | Kind::Symbol)
    }
}

/// Estimated tokens of `text` (≥ 1). Pure; see the module comment for the calibration.
pub fn estimate_tokens(text: &str) -> usize {
    let runs = runs(text);
    let english = is_english(text, &runs);
    let mut total = 0.0;
    for (i, &(kind, start, end)) in runs.iter().enumerate() {
        let run = &text[start..end];
        let len = run.chars().count() as f64;
        let word = |free: f64, rate: f64| 1.0 + (len - free).max(0.0) / rate;
        total += match kind {
            Kind::Space => {
                let before_digit = runs.get(i + 1).is_some_and(|r| r.0 == Kind::Digit);
                f64::from(u8::from(run != " ")) + f64::from(u8::from(before_digit))
            }
            Kind::Digit => len,
            Kind::Punct => 1.0,
            Kind::Symbol => SYMBOL,
            Kind::Latin => {
                let (free, rate) = if english { ENGLISH_WORD } else { LATIN_WORD };
                word(free, rate) + DIACRITIC * run.chars().filter(|c| !c.is_ascii()).count() as f64
            }
            Kind::Cyrillic => word(LATIN_WORD.0, CYRILLIC_RATE),
            Kind::Arabic => word(LATIN_WORD.0, ARABIC_RATE),
            Kind::Abugida => ABUGIDA * len,
            Kind::Cjk => CJK * len,
            Kind::Hangul => HANGUL * len,
        };
    }
    (total.ceil() as usize).max(1)
}

/// `(kind, byte start, byte end)` of each piece of `text`.
fn runs(text: &str) -> Vec<(Kind, usize, usize)> {
    let mut out: Vec<(Kind, usize, usize)> = Vec::new();
    for (i, c) in text.char_indices() {
        let kind = Kind::of(c);
        let end = i + c.len_utf8();
        match out.last_mut() {
            Some(last) if last.0 == kind && kind.joins() => last.2 = end,
            _ => out.push((kind, i, end)),
        }
    }
    out
}

fn is_english(text: &str, runs: &[(Kind, usize, usize)]) -> bool {
    let words = runs.iter().filter(|r| r.0 == Kind::Latin).map(|r| &text[r.1..r.2]);
    let (mut n, mut marked) = (0usize, 0usize);
    for w in words {
        n += 1;
        marked += usize::from(ENGLISH_MARKERS.iter().any(|m| m.eq_ignore_ascii_case(w)));
    }
    n > 0 && marked * 100 >= ENGLISH_SHARE_PCT * n
}

#[cfg(test)]
mod tests;
