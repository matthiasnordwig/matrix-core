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

// Grenzfälle der Silbentrennung: Leerraum zwischen Bindestrich und Umbruch,
// Bindestrich ohne Umbruch und Bindestrich/Leerraum am Textende (dort darf
// keine Schleife über das Ende hinaus lesen).

#[test]
fn normalize_dehyphenates_across_spaces_and_tabs_before_the_newline() {
    // „-“, dann Leerzeichen bzw. Tab, dann „\n“: alles fällt weg, das Wort
    // läuft nahtlos weiter (Schritt (a) überspringt [ \t]* vor dem Umbruch).
    assert_eq!(normalize("Finanz- \nbericht").0, "Finanzbericht");
    assert_eq!(normalize("Finanz-\t\n bericht").0, "Finanzbericht");
    let (n, map) = normalize("Finanz- \t \nbericht");
    assert_eq!(n, "Finanzbericht");
    // „b“ steht im Original an Byte 11 (6 Buchstaben, „-“, 3× Leerraum, „\n“).
    assert_eq!(map[6], 11);
}

#[test]
fn normalize_keeps_a_hyphen_without_line_break() {
    // Kein Umbruch nach dem Bindestrich → er bleibt Teil des Worts.
    assert_eq!(normalize("E-Mail").0, "E-Mail");
    // Leerraum ohne Umbruch: Bindestrich bleibt, Leerraum wird ein Leerzeichen.
    assert_eq!(normalize("Nord- und Süd").0, "Nord- und Süd");
}

#[test]
fn normalize_handles_hyphen_and_whitespace_at_text_end() {
    // Bindestrich als letztes Zeichen: nichts dahinter, er bleibt stehen.
    assert_eq!(normalize("abc-").0, "abc-");
    // Bindestrich + Leerraum am Ende, ohne Umbruch: „-“ bleibt, Leerraum → „ “.
    assert_eq!(normalize("abc- \t").0, "abc- ");
    // Bindestrich + Umbruch am Ende: wird wie mitten im Text entfernt.
    assert_eq!(normalize("Finanz-\n").0, "Finanz");
    assert_eq!(normalize("Finanz-\n \t").0, "Finanz");
    // Leerraum am Ende: ein Leerzeichen, das auf den Anfang des Laufs zeigt.
    let (n, map) = normalize("abc  \n");
    assert_eq!(n, "abc ");
    assert_eq!(map, vec![0, 1, 2, 3]);
}
