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
/// the leading number+optional letter and, separately, whatever Kürzel follows
/// so the caller can decide whether it is a real law. Abs./Satz/Nr. sub-parts
/// are consumed (so they don't get mis-parsed as the Kürzel) but dropped from
/// the key.
static PARAGRAPH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        §\s*                                   # section sign
        (?P<num>\d+)(?P<letter>[a-z])?          # 25 (a)
        (?:\s*(?:bis|-|–)\s*\d+[a-z]?)?          # optional … bis 30
        (?P<sub>
            (?:\s*,?\s*(?:Abs(?:atz|\.)?|Satz|Nr\.?|Nummer|S\.)\s*\d+[a-z]?)*
        )
        \s*
        (?P<law>[A-ZÄÖÜ][A-Za-zÄÖÜäöü]{1,14})?  # trailing Kürzel candidate (KWG …)
        ",
    )
    .unwrap()
});

/// `Art. 28`, `Artikel 28`, `Art 28 Abs. 1`, followed by a Kürzel/DORA. Bare
/// `Art. 28` with no following act is NOT emitted (too ambiguous).
static ARTICLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        \bArt(?:ikel|\.|\b)\s*
        (?P<num>\d+)(?P<letter>[a-z])?
        (?:\s*(?:Abs(?:atz|\.)?|Satz|Nr\.?|Nummer)\s*\d+[a-z]?)*
        \s*
        (?P<law>[A-ZÄÖÜ][A-Za-zÄÖÜäöü.\-]{1,24})?
        ",
    )
    .unwrap()
});

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
        let ref_key = if a.len() == 4 && b.len() != 4 {
            format!("EU:{a}/{b}")
        } else if b.len() == 4 && a.len() != 4 {
            format!("EU:{b}/{a}")
        } else {
            format!("EU:{a}/{b}")
        };
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
        let ref_key = format!("{}:§{num}{letter}", law.to_uppercase());
        push(
            NormRef { ref_key, kind: RefKind::Paragraph, start: m.start(), end: m.end() },
            &mut seen,
            &mut out,
        );
    }

    // Articles (Art. N <DORA|Kürzel>).
    for caps in ARTICLE_RE.captures_iter(text) {
        let raw_law = caps.name("law").map(|m| m.as_str()).unwrap_or("");
        // Strip trailing punctuation the greedy law class may have grabbed.
        let law = raw_law.trim_end_matches(['.', '-', ',']);
        // Bare "Art. 28" with no following act name, or a following word that is
        // not a known act, is anaphoric/ambiguous — drop it. This also rejects
        // "Art. 28 dieses …" ("dieses" is not a known law abbrev).
        if !is_known_law_abbrev(law) {
            continue;
        }
        let num = caps.name("num").unwrap().as_str();
        let letter = caps.name("letter").map(|m| m.as_str()).unwrap_or("");
        let m = caps.get(0).unwrap();
        let ref_key = format!("{}:Art.{num}{letter}", law.to_uppercase());
        push(
            NormRef { ref_key, kind: RefKind::Article, start: m.start(), end: m.end() },
            &mut seen,
            &mut out,
        );
    }

    // Stable order: by first appearance in the source text.
    out.sort_by_key(|r| r.start);
    out
}

#[cfg(test)]
mod tests;
