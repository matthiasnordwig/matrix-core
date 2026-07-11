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
/// separately by `EU_REG_RE`.
const KNOWN: &[&str] = &[
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

/// True if `abbrev` (any case, no surrounding whitespace) is a recognized law
/// abbreviation. An empty string is never known — that is how a bare `§ N` /
/// `Art. N` with no trailing act gets rejected.
pub(crate) fn is_known_law_abbrev(abbrev: &str) -> bool {
    if abbrev.is_empty() {
        return false;
    }
    let lower = abbrev.to_lowercase();
    KNOWN.contains(&lower.as_str())
}
