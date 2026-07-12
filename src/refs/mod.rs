//! Deterministic (no-LLM, no-network) parser for German/EU legal-norm
//! references (RETRIEVAL_QUALITY_PLAN.md AP2). Turns raw chunk text into a set
//! of normalized `ref_key`s so retrieval can follow a chunk's outgoing norm
//! references to their definition/mention chunks.
//!
//! **Precision over recall** is the governing rule: a false-positive ref that
//! resolves to the wrong chunk pollutes retrieval, so when a pattern does not
//! clearly identify a real norm we do NOT emit it. In particular anaphoric
//! references ("Absatz 3 dieses Artikels"), bare page numbers, and dates never
//! match.
//!
//! Normalized `ref_key` shapes:
//!   - `KWG:§25a`, `GWG:§6`         — a paragraph of a named law (Kürzel).
//!   - `DORA:Art.28`                — a DORA article.
//!   - `EU:2022/2554`              — a "Verordnung (EU) YYYY/NNNN".
//!   - `EU:2013/575:Art.395`       — an article bound to an EU regulation via
//!     the long form "Artikel 395 der Verordnung (EU) Nr. 575/2013" (no
//!     Kürzel in the text; the regulation number directly after the article
//!     is the act identity).
//!   - `MARISK:AT4.3.2`, `MARISK:BTO1.1` — a MaRisk module clause.
//!
//! The key deliberately keeps only the *norm identity* (law + paragraph/article/
//! module), not the finer Abs./Satz/Nr. sub-refs — two chunks that cite
//! "§ 25a Abs. 1 KWG" and "§ 25a Abs. 3 Satz 2 KWG" should resolve to the same
//! norm. The full span/sub-parts are retained on the `NormRef` for callers that
//! want them.

use std::sync::LazyLock;

use regex::Regex;

mod law_abbrevs;
use law_abbrevs::is_known_law_abbrev;

/// Public wrapper over the closed law-abbreviation set, so other modules (e.g.
/// `db::chunk_refs`' Kürzel-conflict check) can reuse the exact same recognition
/// without duplicating the list. Input may be any case, no surrounding space.
pub fn is_known_law_abbrev_public(abbrev: &str) -> bool {
    is_known_law_abbrev(abbrev)
}

/// Closed table of act-Kürzel ↔ EU-regulation-number aliases for
/// [`equivalent_ref_keys`]. Same precision-over-recall rule as
/// `law_abbrevs.rs`: only add a pair here when the Kürzel and the regulation
/// number are unambiguously the same act — a too-broad entry would make
/// `find_citing_chunks` merge unrelated norms.
const ACT_ALIASES: &[(&str, &str)] = &[("CRR", "EU:2013/575"), ("DORA", "EU:2022/2554")];

/// For an `Art.`-shaped `ref_key` (`CRR:Art.395` or `EU:2013/575:Art.395`),
/// return the key itself plus its alias form under [`ACT_ALIASES`] (both
/// directions). All other keys — including bare `§`-paragraph keys, since no
/// EU act in the corpus is ever cited by Kürzel-only paragraph — come back
/// unchanged as a single-element vec. Order: the input key first, then the
/// alias (if any); never duplicates.
pub fn equivalent_ref_keys(ref_key: &str) -> Vec<String> {
    let Some((prefix, art)) = ref_key.split_once(":Art.") else {
        return vec![ref_key.to_string()];
    };
    let alias_prefix = ACT_ALIASES.iter().find_map(|&(kuerzel, eu)| {
        if prefix == kuerzel {
            Some(eu.to_string())
        } else if prefix == eu {
            Some(kuerzel.to_string())
        } else {
            None
        }
    });
    match alias_prefix {
        Some(alias) => vec![ref_key.to_string(), format!("{alias}:Art.{art}")],
        None => vec![ref_key.to_string()],
    }
}

/// One parsed norm reference found in a piece of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormRef {
    /// Normalized identity key (e.g. `KWG:§25a`, `DORA:Art.28`, `MARISK:AT4.3.2`).
    pub ref_key: String,
    /// The kind of norm the key denotes.
    pub kind: RefKind,
    /// Byte offset of the match start within the parsed text.
    pub start: usize,
    /// Byte offset just past the match end.
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// `§ N[a] <Kürzel>` — a paragraph of a named German law.
    Paragraph,
    /// `Art. N <DORA|Kürzel>` — an article of a named act.
    Article,
    /// `Verordnung (EU) YYYY/NNNN` — an EU regulation by CELEX-style number.
    EuRegulation,
    /// `AT|BT|BTO|BTR N(.N)*` — a MaRisk module clause.
    MaRiskModule,
}

// --- Patterns -----------------------------------------------------------------

/// `§ 25a`, `§25a`, `§ 6` (optionally a doubled `§§ … bis …` range). We capture
/// the leading number+optional letter, the *end* of a `bis`/`-`/`–` range (so
/// the range can be expanded into individual refs), and separately whatever
/// Kürzel follows so the caller can decide whether it is a real law. Abs./Satz/
/// Nr. sub-parts are consumed (so they don't get mis-parsed as the Kürzel) but
/// dropped from the key.
static PARAGRAPH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        §\s*                                            # section sign
        (?P<num>\d+)(?P<letter>[a-z])?                   # 25 (a)
        (?:\s*(?:bis|-|–)\s*(?P<end_num>\d+)(?P<end_letter>[a-z])?)?  # … bis 13c
        (?P<sub>
            (?:\s*,?\s*(?:Abs(?:atz|\.)?|Satz|Nr\.?|Nummer|S\.)\s*\d+[a-z]?)*
        )
        \s*
        (?P<law>[A-ZÄÖÜ][A-Za-zÄÖÜäöü]{1,14})?          # trailing Kürzel candidate (KWG …)
        ",
    )
    .unwrap()
});

/// `Art. 28`, `Artikel 28`, `Art 28 Abs. 1`, followed by a Kürzel/DORA — or a
/// range `Artikel 387 bis 410 …`. Captures the range end so it can be expanded.
/// Bare `Art. 28` with no following act is NOT emitted (too ambiguous). The
/// range alternative is essential so a following `bis` token (which is not a
/// Kürzel) does not cause the whole ref to be dropped. `Artikels`/`Artikeln`
/// cover the genitive/dative forms ("des Artikels 92", "in den Artikeln 387
/// bis 410").
static ARTICLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        \bArt(?:ikels|ikeln|ikel|\.|\b)\s*
        (?P<num>\d+)(?P<letter>[a-z])?
        (?:\s*(?:bis|-|–)\s*(?P<end_num>\d+)(?P<end_letter>[a-z])?)?  # … bis 410
        (?:\s*(?:Abs(?:atz|\.)?|Satz|Nr\.?|Nummer)\s*\d+[a-z]?)*
        \s*
        (?P<law>[A-ZÄÖÜ][A-Za-zÄÖÜäöü.\-]{1,24})?
        ",
    )
    .unwrap()
});

/// The EU-regulation *long form* directly after an article reference:
/// `Artikel 387 bis 410 der Verordnung (EU) Nr. 575/2013`. Anchored at the
/// start of the tail right after the ARTICLE_RE match, allowing only the
/// determiner (`der`/`des`/`die`/`den`) plus an optional `Delegierten` before
/// the regulation — a deliberately NARROW window (no sentence boundary, no
/// free text in between), so a regulation mentioned merely later in the same
/// sentence can never be bound to the article.
static EU_AFTER_ARTICLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)^
        (?:der|des|die|den)\s+
        (?:Delegierten\s+)?
        Verordnung\s*\(\s*EU\s*\)\s*
        (?:Nr\.?\s*)?
        (?P<a>\d{2,4})\s*/\s*(?P<b>\d{2,4})
        ",
    )
    .unwrap()
});

/// Normalize an EU-regulation number pair to `YYYY/NNNN` (year first) — the
/// same rule the EU_REG_RE loop applies: the 4-digit part is the year; when
/// ambiguous, keep source order.
fn normalize_eu_number(a: &str, b: &str) -> String {
    if a.len() == 4 && b.len() != 4 {
        format!("{a}/{b}")
    } else if b.len() == 4 && a.len() != 4 {
        format!("{b}/{a}")
    } else {
        format!("{a}/{b}")
    }
}

/// If `tail` (the text right after an article match) begins with the EU long
/// form ("der Verordnung (EU) Nr. 575/2013"), return the normalized regulation
/// number (`2013/575`).
fn eu_regulation_after(tail: &str) -> Option<String> {
    let caps = EU_AFTER_ARTICLE_RE.captures(tail)?;
    Some(normalize_eu_number(
        caps.name("a").unwrap().as_str(),
        caps.name("b").unwrap().as_str(),
    ))
}

/// `Verordnung (EU) 2022/2554`, `Verordnung (EU) Nr. 575/2013`. The `(EU)` and
/// the `YYYY/NNNN` (or `Nr. NNN/YYYY`) shape make this unambiguous.
static EU_REG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?xi)
        Verordnung\s*\(\s*EU\s*\)\s*
        (?:Nr\.?\s*)?
        (?P<a>\d{2,4})\s*/\s*(?P<b>\d{2,4})
        ",
    )
    .unwrap()
});

/// MaRisk modules `AT 4.3.2`, `BTO 1.1`, `BTR 2.1`, `BT 3`. The module letters
/// are a closed set (AT/BT/BTO/BTR) and must be followed by a dotted number, so
/// this does not collide with ordinary uppercase words.
static MARISK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?P<mod>AT|BTO|BTR|BT)\s*(?P<clause>\d+(?:\.\d+)*)\b").unwrap()
});

/// Hard upper bound on the number of individual refs a single range may expand
/// into. EU-article ranges can span hundreds of articles ("Artikel 1 bis 521");
/// materializing all of them would flood retrieval with a regulation-wide ref
/// cloud, violating the module's precision-over-recall rule. Above the cap we
/// emit only the range's start and end ref.
const MAX_RANGE_EXPANSION: usize = 30;

/// Expand a norm range `start..=end` into the individual `(num, letter)` parts
/// it covers. Two iteration modes, chosen by the shape of start/end:
///   - **Letter suffixes on the same stem** (`13` → `13c`): same number, iterate
///     the letter suffix from the start letter (or bare, treated as "before a")
///     up to the end letter → `13, 13a, 13b, 13c`.
///   - **Consecutive numbers** (`13` → `15`): iterate the integer, dropping any
///     letter suffixes → `13, 14, 15`.
/// If the range is degenerate (end before start, or an unhandled mixed shape)
/// only the start is returned. Above `MAX_RANGE_EXPANSION` individual parts,
/// only start and end are returned (no full expansion).
fn expand_range(
    start_num: &str,
    start_letter: &str,
    end_num: &str,
    end_letter: &str,
) -> Vec<(String, String)> {
    let (Ok(sn), Ok(en)) = (start_num.parse::<u32>(), end_num.parse::<u32>()) else {
        return vec![(start_num.to_string(), start_letter.to_string())];
    };
    let start = (sn, start_letter.chars().next());
    let end = (en, end_letter.chars().next());

    // Same number, differing only by letter suffix → iterate the letter.
    if sn == en {
        // Bare stem is "before a": start at the given start letter (or None),
        // walk up to the end letter inclusive.
        let end_c = match end.1 {
            Some(c) => c,
            // "§§ 13 bis 13" — same identical ref, just the start.
            None => return vec![(start_num.to_string(), start_letter.to_string())],
        };
        if !end_c.is_ascii_lowercase() {
            return vec![(start_num.to_string(), start_letter.to_string())];
        }
        let mut out: Vec<(String, String)> = Vec::new();
        // First the bare/start form.
        out.push((start_num.to_string(), start_letter.to_string()));
        // Then a, b, … up to end_c, skipping any at/below the start letter.
        let from = start.1.map(|c| (c as u8) + 1).unwrap_or(b'a');
        for c in from..=(end_c as u8) {
            out.push((start_num.to_string(), (c as char).to_string()));
            if out.len() >= MAX_RANGE_EXPANSION {
                break;
            }
        }
        return out;
    }

    // Consecutive numbers (drop letter suffixes on the endpoints).
    if en < sn {
        return vec![(start_num.to_string(), start_letter.to_string())];
    }
    let count = (en - sn) as usize + 1;
    if count > MAX_RANGE_EXPANSION {
        // Over-expansion guard: only the two endpoints.
        return vec![
            (sn.to_string(), String::new()),
            (en.to_string(), String::new()),
        ];
    }
    (sn..=en).map(|n| (n.to_string(), String::new())).collect()
}

/// Parse all norm references in `text`, de-duplicated by `(ref_key)` keeping the
/// first occurrence's span. Order follows first appearance in the text.
pub fn parse_refs(text: &str) -> Vec<NormRef> {
    let mut out: Vec<NormRef> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let push = |r: NormRef, seen: &mut std::collections::HashSet<String>, out: &mut Vec<NormRef>| {
        if seen.insert(r.ref_key.clone()) {
            out.push(r);
        }
    };

    // MaRisk modules.
    for caps in MARISK_RE.captures_iter(text) {
        let m = caps.get(0).unwrap();
        let module = caps.name("mod").unwrap().as_str();
        let clause = caps.name("clause").unwrap().as_str();
        let ref_key = format!("MARISK:{module}{clause}");
        push(
            NormRef { ref_key, kind: RefKind::MaRiskModule, start: m.start(), end: m.end() },
            &mut seen,
            &mut out,
        );
    }

    // EU regulations.
    for caps in EU_REG_RE.captures_iter(text) {
        let m = caps.get(0).unwrap();
        let a = caps.name("a").unwrap().as_str();
        let b = caps.name("b").unwrap().as_str();
        // Normalize to YYYY/NNNN — the four-digit part is the year. Both orders
        // ("2022/2554" and legacy "Nr. 575/2013") are accepted; we place the
        // 4-digit year first when unambiguous, else keep source order.
        let ref_key = format!("EU:{}", normalize_eu_number(a, b));
        push(
            NormRef { ref_key, kind: RefKind::EuRegulation, start: m.start(), end: m.end() },
            &mut seen,
            &mut out,
        );
    }

    // Paragraphs (§ N[a] <Kürzel>).
    for caps in PARAGRAPH_RE.captures_iter(text) {
        let law = caps.name("law").map(|m| m.as_str()).unwrap_or("");
        // A § without a recognized law abbreviation is too ambiguous to resolve
        // (could be an internal cross-ref or a different act) — drop it.
        if !is_known_law_abbrev(law) {
            continue;
        }
        let num = caps.name("num").unwrap().as_str();
        let letter = caps.name("letter").map(|m| m.as_str()).unwrap_or("");
        let m = caps.get(0).unwrap();
        // Expand a `bis`/`-` range into individual refs; a plain ref yields just
        // one part. Every part shares the match span (they came from one range).
        let parts = match caps.name("end_num") {
            Some(en) => expand_range(
                num,
                letter,
                en.as_str(),
                caps.name("end_letter").map(|m| m.as_str()).unwrap_or(""),
            ),
            None => vec![(num.to_string(), letter.to_string())],
        };
        for (n, l) in parts {
            let ref_key = format!("{}:§{n}{l}", law.to_uppercase());
            push(
                NormRef { ref_key, kind: RefKind::Paragraph, start: m.start(), end: m.end() },
                &mut seen,
                &mut out,
            );
        }
    }

    // Articles (Art. N <DORA|Kürzel>) — or the EU long form ("Artikel 387 bis
    // 410 der Verordnung (EU) Nr. 575/2013"), which has no Kürzel: the article
    // is bound to the regulation number instead → key `EU:2013/575:Art.395`.
    for caps in ARTICLE_RE.captures_iter(text) {
        let raw_law = caps.name("law").map(|m| m.as_str()).unwrap_or("");
        // Strip trailing punctuation the greedy law class may have grabbed.
        let law = raw_law.trim_end_matches(['.', '-', ',']);
        let m = caps.get(0).unwrap();
        // Determine the act prefix for the key: a known Kürzel ("CRR"), or the
        // EU regulation named directly after the match ("EU:2013/575"). Bare
        // "Art. 28" with neither is anaphoric/ambiguous — drop it. This also
        // rejects "Art. 28 dieses …" ("dieses" is not a known law abbrev).
        let prefix = if is_known_law_abbrev(law) {
            law.to_uppercase()
        } else if law.is_empty() {
            match eu_regulation_after(&text[m.end()..]) {
                Some(eu) => format!("EU:{eu}"),
                None => continue,
            }
        } else {
            continue;
        };
        let num = caps.name("num").unwrap().as_str();
        let letter = caps.name("letter").map(|m| m.as_str()).unwrap_or("");
        let parts = match caps.name("end_num") {
            Some(en) => expand_range(
                num,
                letter,
                en.as_str(),
                caps.name("end_letter").map(|m| m.as_str()).unwrap_or(""),
            ),
            None => vec![(num.to_string(), letter.to_string())],
        };
        for (n, l) in parts {
            let ref_key = format!("{prefix}:Art.{n}{l}");
            push(
                NormRef { ref_key, kind: RefKind::Article, start: m.start(), end: m.end() },
                &mut seen,
                &mut out,
            );
        }
    }

    // Stable order: by first appearance in the source text.
    out.sort_by_key(|r| r.start);
    out
}

#[cfg(test)]
mod tests;
