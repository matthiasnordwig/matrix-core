//! Table-driven tests for the norm-reference parser. ≥ 20 positive cases with
//! real MaRisk/DORA/KWG/GwG phrasings and ≥ 8 negative cases (anaphoric refs,
//! page numbers, dates) that MUST NOT match — precision is the priority.

use super::{parse_refs, RefKind};

/// Collect just the normalized keys for a text (order = appearance).
fn keys(text: &str) -> Vec<String> {
    parse_refs(text).into_iter().map(|r| r.ref_key).collect()
}

/// True iff `text` yields exactly one ref with `key`.
fn one(text: &str, key: &str) -> bool {
    let k = keys(text);
    k == vec![key.to_string()]
}

#[test]
fn positive_cases() {
    // (text, expected single ref_key)
    let cases: &[(&str, &str)] = &[
        // --- KWG / GwG paragraphs (with Abs./Satz/Nr. sub-parts stripped) ---
        ("Die Anforderungen des § 25a KWG sind einzuhalten.", "KWG:§25a"),
        ("gemäß §25a KWG", "KWG:§25a"),
        ("§ 25a Abs. 1 KWG", "KWG:§25a"),
        ("§ 25a Abs. 1 Satz 3 Nr. 1 KWG", "KWG:§25a"),
        ("Nach § 6 GwG hat das Institut …", "GWG:§6"),
        ("§ 6 Absatz 2 GwG", "GWG:§6"),
        ("die Vorgaben aus § 10 GwG", "GWG:§10"),
        ("§ 64a VAG", "VAG:§64a"),
        ("Pflichten nach § 80 WpHG", "WPHG:§80"),
        ("Regelungen des § 91 Abs. 2 AktG", "AKTG:§91"),
        // --- DORA / EU articles ---
        ("nach Art. 28 DORA", "DORA:Art.28"),
        ("Artikel 28 DORA", "DORA:Art.28"),
        ("Art. 5 Abs. 1 DORA", "DORA:Art.5"),
        ("Art. 30 DORA regelt vertragliche Vereinbarungen.", "DORA:Art.30"),
        ("gemäß Art. 92 CRR", "CRR:Art.92"),
        // --- EU regulations by number ---
        ("Verordnung (EU) 2022/2554", "EU:2022/2554"),
        ("die Verordnung (EU) Nr. 575/2013 (CRR)", "EU:2013/575"),
        ("Verordnung (EU) 2016/679", "EU:2016/679"),
        // --- MaRisk modules ---
        ("Siehe AT 4.3.2 der MaRisk.", "MARISK:AT4.3.2"),
        ("Modul BTO 1.1", "MARISK:BTO1.1"),
        ("Anforderungen in BTR 2.1", "MARISK:BTR2.1"),
        ("BT 3 der MaRisk", "MARISK:BT3"),
        ("AT 4.3.2 Absatz 1", "MARISK:AT4.3.2"),
        ("gemäß AT 9 (Auslagerung)", "MARISK:AT9"),
    ];
    for (text, expected) in cases {
        assert!(
            one(text, expected),
            "expected {expected:?} for {text:?}, got {:?}",
            keys(text)
        );
    }
}

#[test]
fn negative_cases_must_not_match() {
    let cases: &[&str] = &[
        // Anaphoric — no number+law identity.
        "Absatz 3 dieses Artikels bleibt unberührt.",
        "wie in diesem Absatz beschrieben",
        "Art. 28 dieses Übereinkommens",
        // Bare page numbers.
        "siehe Seite 28",
        "vgl. S. 128 des Berichts",
        "auf den Seiten 12 bis 15",
        // Dates.
        "vom 28. Mai 2022",
        "im Jahr 2022",
        "Stand: 14.12.2022",
        // A § / Art. with no recognized law abbreviation.
        "§ 5 Satz 1 des Vertrags",
        "gemäß § 3 der Ordnung",
        "Art. 12 des Anhangs",
    ];
    for text in cases {
        assert!(
            keys(text).is_empty(),
            "expected NO refs for {text:?}, got {:?}",
            keys(text)
        );
    }
}

#[test]
fn multiple_refs_in_order_and_deduped() {
    let text = "§ 25a KWG verlangt Regelungen; ferner gelten AT 4.3.2 und § 25a Abs. 3 KWG sowie Art. 28 DORA.";
    let ks = keys(text);
    // § 25a KWG appears twice but is emitted once; order by first appearance.
    assert_eq!(ks, vec!["KWG:§25a", "MARISK:AT4.3.2", "DORA:Art.28"]);
}

#[test]
fn kind_is_classified() {
    let refs = parse_refs("§ 25a KWG, Art. 28 DORA, Verordnung (EU) 2022/2554, AT 4.3.2");
    let kinds: Vec<RefKind> = refs.iter().map(|r| r.kind).collect();
    assert!(kinds.contains(&RefKind::Paragraph));
    assert!(kinds.contains(&RefKind::Article));
    assert!(kinds.contains(&RefKind::EuRegulation));
    assert!(kinds.contains(&RefKind::MaRiskModule));
}

#[test]
fn spans_point_at_the_match() {
    let text = "Text vor § 25a KWG hier.";
    let refs = parse_refs(text);
    assert_eq!(refs.len(), 1);
    let r = &refs[0];
    // The span must cover at least the "§ 25a" .. "KWG" region.
    let matched = &text[r.start..r.end];
    assert!(matched.contains("25a"), "span {matched:?}");
    assert!(matched.contains("KWG"), "span {matched:?}");
}
