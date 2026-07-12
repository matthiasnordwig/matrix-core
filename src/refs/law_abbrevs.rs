//! Closed set of recognized German/EU financial-regulation law abbreviations
//! (Kürzel) and act names. A `§ N` or `Art. N` is only emitted as a norm
//! reference when it is followed by one of these — this is the primary
//! false-positive filter (RETRIEVAL_QUALITY_PLAN.md AP2: precision > recall).
//!
//! Matching is case-insensitive on the abbreviation. Keep this list focused on
//! the MaRisk/DORA/banking-supervision corpus; adding an over-broad word here
//! (e.g. a common noun) would let anaphoric cross-references leak through, so
//! new entries should be genuine act abbreviations only.

/// Recognized law/act abbreviations, lowercased. `DORA` is included so
/// `Art. 28 DORA` resolves; the EU-regulation *number* form is handled
/// separately by `EU_REG_RE`. `pub(super)` so `RefLexicon::builtin()` (in
/// `refs/mod.rs`) can build its abbreviation set from the same source of
/// truth instead of duplicating the list.
pub(super) const KNOWN: &[&str] = &[
    // Banking / supervision
    "kwg",   // Kreditwesengesetz
    "cra",   // (CRR/CRA context)
    "crr",   // Capital Requirements Regulation
    "crd",   // Capital Requirements Directive
    "wphg",  // Wertpapierhandelsgesetz
    "kagb",  // Kapitalanlagegesetzbuch
    "vag",   // Versicherungsaufsichtsgesetz
    "zag",   // Zahlungsdiensteaufsichtsgesetz
    "bsig",  // BSI-Gesetz
    "gwg",   // Geldwäschegesetz
    "sag",   // Sanierungs- und Abwicklungsgesetz
    // General codes occasionally cross-referenced
    "hgb",   // Handelsgesetzbuch
    "bgb",   // Bürgerliches Gesetzbuch
    "aktg",  // Aktiengesetz
    "gmbhg", // GmbH-Gesetz
    "stgb",  // Strafgesetzbuch
    "ao",    // Abgabenordnung
    "dsgvo", // Datenschutz-Grundverordnung
    "bdsg",  // Bundesdatenschutzgesetz
    // EU acts by short name
    "dora",  // Digital Operational Resilience Act
    "mifid", // Markets in Financial Instruments Directive
    "emir",  // European Market Infrastructure Regulation
];

/// Long forms for a subset of [`KNOWN`] Kürzel, used to seed
/// `RefLexicon::builtin()`'s long-form matching (TOOL_TIER_PLAN.md Teil B —
/// "§ N des <Langform>[es|s]" with no Kürzel in the text, e.g. "§ 6 des
/// Geldwäschegesetzes" → `GWG:§6`). Deliberately not exhaustive: only acts
/// whose long form is unambiguous and commonly written out in the corpus are
/// listed here — same precision-over-recall rule as `KNOWN` itself.
pub(super) const BUILTIN_LONG_FORMS: &[(&str, &str)] = &[
    ("kwg", "Kreditwesengesetz"),
    ("vag", "Versicherungsaufsichtsgesetz"),
    ("zag", "Zahlungsdiensteaufsichtsgesetz"),
    ("kagb", "Kapitalanlagegesetzbuch"),
    ("gwg", "Geldwäschegesetz"),
    ("hgb", "Handelsgesetzbuch"),
    ("bgb", "Bürgerliches Gesetzbuch"),
    ("aktg", "Aktiengesetz"),
    ("sag", "Sanierungs- und Abwicklungsgesetz"),
    ("wphg", "Wertpapierhandelsgesetz"),
];
