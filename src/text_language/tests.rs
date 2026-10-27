use super::*;

#[test]
fn passages_get_their_language_code() {
    let de = "Der Verantwortliche und der Auftragsverarbeiter benennen auf jeden Fall einen Datenschutzbeauftragten, wenn die Verarbeitung von einer Behörde durchgeführt wird.";
    let en = "The entity shall submit an early warning to the CSIRT within 24 hours of becoming aware of the significant incident, which shall indicate whether it is suspected.";
    let fr = "Une clause contractuelle est abusive si elle est d'une nature telle que son utilisation s'écarte manifestement des bonnes pratiques commerciales et qui ne sont pas conformes aux principes.";
    let es = "El período de soporte será de al menos cinco años y los fabricantes deberán garantizar que las vulnerabilidades del producto se gestionan por el tiempo que se espera que esté en uso.";
    assert_eq!(passage_language(de), Some("de"));
    assert_eq!(passage_language(en), Some("en"));
    assert_eq!(passage_language(fr), Some("fr"));
    assert_eq!(passage_language(es), Some("es"));
    assert_eq!(passage_language("Art. 33 DSGVO 72 h"), None);
    assert_eq!(document_language(&format!("{en} {de} {de} {de} {de}")), Some("de"));
    assert_eq!(document_language(""), None);
    // Samples start inside multi-byte characters without panicking.
    assert_eq!(document_language(&format!("ä{}", de.repeat(40))), Some("de"));
}

#[test]
fn queries_need_only_one_function_word() {
    assert_eq!(query_language("How long must the provider keep the documentation?"), Some("en"));
    assert_eq!(query_language("Verlangt NIST für den Zugang zu privilegierten Konten eine Mehr-Faktor-Authentifizierung?"), Some("de"));
    assert_eq!(query_language("¿Cuánto tiempo debe el fabricante ofrecer las actualizaciones de seguridad?"), Some("es"));
    assert_eq!(query_language("Wie lange muss ein Hersteller nach der EU-Cyberresilienzverordnung Schwachstellen behandeln?"), Some("de"));
    // A passage needs five; one sentence of a question would stay unknown.
    assert_eq!(passage_language("How long must the provider keep the documentation?"), None);
    // No function word, or a tie between two languages: unknown.
    assert_eq!(query_language("Art. 26 DORA TLPT"), None);
    assert_eq!(query_language("the der"), None);
    assert_eq!(query_language("the der and und"), None);
}

#[test]
fn passage_word_lists_are_disjoint() {
    for (i, (a, wa)) in PASSAGE_WORDS.iter().enumerate() {
        for (b, wb) in &PASSAGE_WORDS[i + 1..] {
            let shared: Vec<_> = wa.iter().filter(|w| wb.contains(w)).collect();
            assert!(shared.is_empty(), "{a} and {b} share {shared:?}");
        }
    }
}
