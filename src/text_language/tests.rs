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

#[test]
fn mail_with_a_long_distribution_list_is_english() {
    let to: Vec<String> = (0..30).map(|i| format!("person{i}.name@enron.com")).collect();
    let cc: Vec<String> = (0..10).map(|i| format!("Name{i}/HOU/ECT@ECT")).collect();
    let mail = format!(
        "From: jeff.skilling@enron.com\nTo: {}\n\t{}\nCc: {}\nSubject: Press review\n\n**To:** a@enron.com, b@enron.com\n\nThe press review is attached and the energy services unit is on the front page of the paper, which shall be read by all of us. Visit www.enron.com/news or https://enron.com/x for the rest.",
        to.join(", "),
        to.join(", "),
        cc.join(", ")
    );
    assert_eq!(passage_language(&mail), Some("en"));
    // The same list without the header lines (a forwarded list inside the text) is no Portuguese either.
    let inline = format!("Please send this to {} and keep it short for the whole team and for the rest of the unit.", to.join(" "));
    assert_ne!(passage_language(&inline), Some("pt"));
    assert_eq!(passage_language(&to.join(" ")), None);
}

#[test]
fn portuguese_stays_portuguese() {
    let pt = "Os fabricantes não devem colocar no mercado produtos com vulnerabilidades conhecidas, e os dados em causa são tratados pelo responsável ao longo de uma vida útil, às vezes até ao seu fim, pela empresa.";
    assert_eq!(passage_language(pt), Some("pt"));
    // With an address and a header in front.
    assert_eq!(passage_language(&format!("From: ana.silva@empresa.com.br\nTo: rui@empresa.com\n\n{pt}")), Some("pt"));
}

#[test]
fn german_text_with_urls_stays_german() {
    let de = "Der Verantwortliche und der Auftragsverarbeiter benennen auf jeden Fall einen Datenschutzbeauftragten, wenn die Verarbeitung von einer Behörde durchgeführt wird, siehe https://www.example.org/dsgvo/art-37 oder www.datenschutz.de und /usr/local/share/doc/dsgvo.txt.";
    assert_eq!(passage_language(de), Some("de"));
    assert_eq!(passage_language(&format!("{de} {}", "www.a.com b.net c.org ".repeat(30))), Some("de"));
}

#[test]
fn prose_keeps_ordinary_words_with_dots_and_slashes() {
    let kept = prose::prose("and/or e.g. i.e. end.Die Dr. Müller 24/7 Mr. Smith", 4000);
    for w in ["and/or", "e.g.", "i.e.", "end.Die", "Müller", "24/7"] {
        assert!(kept.contains(w), "{w} missing in {kept:?}");
    }
    let dropped = prose::prose("see x.com, (www.y.de) <a@b.org> /usr/bin/ls ~/doc C:\\Temp\\a.txt http://z", 4000);
    assert_eq!(dropped.trim(), "see");
}
