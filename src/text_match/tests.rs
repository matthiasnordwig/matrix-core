use super::*;

#[test]
fn normalize_dehyphenates_and_collapses_whitespace() {
    let (n, map) = normalize("Finanz-\n  bericht  2025\n\nKfW");
    assert_eq!(n, "Finanzbericht 2025 KfW");
    assert_eq!(n.len(), map.len());
}

#[test]
fn contains_evidence_respects_fragment_order() {
    let hay = "Die KfW fördert als Bank aus Verantwortung … und\nwächst weiter.";
    assert!(contains_evidence(hay, "fördert als Bank … wächst weiter"));
    assert!(!contains_evidence(hay, "wächst weiter … fördert als Bank"));
    assert!(!contains_evidence(hay, "   "));
    assert!(contains_evidence_normalized(&normalize(hay).0, "aus Verantwortung"));
}

#[test]
fn fold_lowercases_after_normalizing() {
    assert_eq!(fold("KfW  Förder-\nbank"), "kfw förderbank");
    assert!(normalized_contains("a  b", "a b"));
    assert!(!normalized_contains("a b", ""));
}
