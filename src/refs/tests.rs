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

// --- Range expansion (§§ X bis Y, Artikel X bis Y) ---------------------------

/// Collect keys as a sorted set for order-independent comparison.
fn keyset(text: &str) -> Vec<String> {
    let mut k = keys(text);
    k.sort();
    k.dedup();
    k
}

#[test]
fn range_expansion_positive() {
    // (text, expected exact set of keys)
    let cases: &[(&str, &[&str])] = &[
        // Same stem, letter suffixes → §13, §13a, §13b, §13c.
        ("§§ 13 bis 13c KWG", &["KWG:§13", "KWG:§13a", "KWG:§13b", "KWG:§13c"]),
        // Hyphen / en-dash as range separators behave identically.
        ("§§ 13-13c KWG", &["KWG:§13", "KWG:§13a", "KWG:§13b", "KWG:§13c"]),
        ("§§ 13–13c KWG", &["KWG:§13", "KWG:§13a", "KWG:§13b", "KWG:§13c"]),
        // Pure numeric paragraph range → §17 §18 … §22.
        (
            "§§ 17 bis 22 KWG",
            &["KWG:§17", "KWG:§18", "KWG:§19", "KWG:§20", "KWG:§21", "KWG:§22"],
        ),
        // Article range with an explicit Kürzel → full expansion (3 articles).
        ("Artikel 90 bis 92 CRR", &["CRR:Art.90", "CRR:Art.91", "CRR:Art.92"]),
        // Start already carries a letter: §10c bis 10i → c..i on stem 10.
        (
            "§§ 10c bis 10i KWG",
            &[
                "KWG:§10c", "KWG:§10d", "KWG:§10e", "KWG:§10f", "KWG:§10g", "KWG:§10h",
                "KWG:§10i",
            ],
        ),
    ];
    for (text, expected) in cases {
        let got = keyset(text);
        let mut exp: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
        exp.sort();
        assert_eq!(&got, &exp, "for {text:?}");
    }
}

#[test]
fn range_golden_387_to_410_contains_395() {
    // The KfW→CRR golden case: Art. 387–410 (24 articles, ≤ cap) must fully
    // expand and include Art. 395. Needs a recognized Kürzel to emit at all.
    let ks = keyset("Artikel 387 bis 410 CRR entsprechend");
    assert_eq!(ks.len(), 410 - 387 + 1, "24 articles expected, got {ks:?}");
    assert!(ks.contains(&"CRR:Art.395".to_string()), "must contain Art.395: {ks:?}");
    assert!(ks.contains(&"CRR:Art.387".to_string()));
    assert!(ks.contains(&"CRR:Art.410".to_string()));
}

#[test]
fn range_over_cap_yields_only_endpoints() {
    // "Artikel 1 bis 521 CRR" — 521 articles, way over the 30 cap → only the two
    // endpoints, never a regulation-wide ref cloud.
    let ks = keyset("Artikel 1 bis 521 CRR");
    assert_eq!(ks, vec!["CRR:Art.1".to_string(), "CRR:Art.521".to_string()]);

    // A paragraph range over the cap likewise collapses to its endpoints.
    let pk = keyset("§§ 1 bis 100 KWG");
    assert_eq!(pk, vec!["KWG:§1".to_string(), "KWG:§100".to_string()]);
}

#[test]
fn range_without_kuerzel_still_emits_nothing() {
    // Precision guard: a range with no recognized law abbreviation after it must
    // NOT leak refs (this is the "bis is not a Kürzel" drop case — the fix keeps
    // the drop when there is truly no act, only stops it from breaking parsing).
    assert!(keys("§§ 13 bis 13c des Vertrags").is_empty());
    assert!(keys("Artikel 387 bis 410 der Anlage").is_empty());
    // Anaphoric article range must not match either.
    assert!(keys("Artikel 5 bis 9 dieses Übereinkommens").is_empty());
}

#[test]
fn range_exact_count_no_over_expansion() {
    // §§ 13 bis 13c → exactly 4 refs (not 5, not the whole KWG).
    assert_eq!(keyset("§§ 13 bis 13c KWG").len(), 4);
}

// --- EU long form: "Artikel N [bis M] der Verordnung (EU) Nr. X/Y" ----------

#[test]
fn eu_long_form_range_golden_387_410() {
    // The exact ISSUES golden string: 24 articles, fully expanded, bound to the
    // regulation number → must contain Art.395. The plain EU-regulation ref is
    // emitted alongside (the chunk references the regulation as a whole too).
    let ks = keyset("Artikel 387 bis 410 der Verordnung (EU) Nr. 575/2013");
    assert!(ks.contains(&"EU:2013/575:Art.395".to_string()), "must contain Art.395: {ks:?}");
    assert!(ks.contains(&"EU:2013/575:Art.387".to_string()));
    assert!(ks.contains(&"EU:2013/575:Art.410".to_string()));
    let art_count = ks.iter().filter(|k| k.contains(":Art.")).count();
    assert_eq!(art_count, 24, "24 bound articles expected: {ks:?}");
    assert!(ks.contains(&"EU:2013/575".to_string()), "plain regulation ref stays");
}

#[test]
fn eu_long_form_single_article() {
    // Single article, legacy number order → EU:2013/575:Art.92 (+ the plain reg).
    let ks = keyset("Artikel 92 der Verordnung (EU) Nr. 575/2013");
    assert!(ks.contains(&"EU:2013/575:Art.92".to_string()), "{ks:?}");
    // Modern order and genitive form.
    let ks2 = keyset("gemäß des Artikels 28 der Verordnung (EU) 2022/2554");
    assert!(ks2.contains(&"EU:2022/2554:Art.28".to_string()), "{ks2:?}");
    // Dative plural + Delegierte Verordnung.
    let ks3 = keyset("in den Artikeln 3 bis 5 der Delegierten Verordnung (EU) 2024/1774");
    assert!(ks3.contains(&"EU:2024/1774:Art.4".to_string()), "{ks3:?}");
}

#[test]
fn eu_long_form_range_over_cap_only_endpoints() {
    // "Artikel 92 bis 386 …" — 295 articles, over the cap → only the endpoints
    // (which is exactly what the KfWV Q2 golden case needs: Art.92 IS a bound
    // endpoint), never a regulation-wide cloud.
    let ks = keyset("die Artikel 92 bis 386 der Verordnung (EU) Nr. 575/2013");
    let arts: Vec<&String> = ks.iter().filter(|k| k.contains(":Art.")).collect();
    assert_eq!(
        arts,
        vec!["EU:2013/575:Art.386", "EU:2013/575:Art.92"],
        "over-cap EU range must yield only its endpoints: {ks:?}"
    );
}

#[test]
fn eu_long_form_negative_cases() {
    // No regulation form directly after the article → nothing (still dropped).
    assert!(keys("Artikel 12 des Gesetzes über die Deutsche Bundesbank").is_empty());
    // Regulation too far away / free text or sentence boundary in between →
    // the narrow window must NOT bind the article to it. (The regulation itself
    // still emits its plain EU ref — but no article-bound key may appear.)
    let ks = keys(
        "Artikel 12 findet Anwendung. Unberührt bleibt die Verordnung (EU) Nr. 575/2013.",
    );
    assert!(
        ks.iter().all(|r| !r.contains(":Art.")),
        "no article binding across a sentence boundary: {ks:?}"
    );
    let ks2 = keys("Artikel 12 im Einklang mit den Vorgaben der Verordnung (EU) Nr. 575/2013");
    assert!(
        ks2.iter().all(|r| !r.contains(":Art.")),
        "no article binding across free text: {ks2:?}"
    );
    // An unknown uppercase token after the article never falls through to EU
    // binding ("Artikel 30 Verordnung …" without determiner stays ambiguous).
    let ks3 = keys("Artikel 30 Verordnung (EU) Nr. 575/2013");
    assert!(ks3.iter().all(|r| !r.contains(":Art.")), "{ks3:?}");
}

// --- EU long form with an Absatz/Satz/Buchstabe insert (ISSUES, 2026-07-12) --
// "Artikel 92 Absatz 1 Buchstabe c der Verordnung (EU) Nr. 575/2013" — a
// closed sub-part grammar between the article number and the EU long form
// must not break the recognition window (real KWG-Korpus phrasing).

#[test]
fn eu_long_form_with_subpart_insert_positive() {
    // The exact real-corpus KWG-chunk sentence from the ISSUES entry.
    let ks = keyset(
        "3. Artikel 92 Absatz 1 Buchstabe c der Verordnung (EU) Nr. 575/2013 und die \
         zusätzliche Eigenmittelanforderung",
    );
    assert!(ks.contains(&"EU:2013/575:Art.92".to_string()), "{ks:?}");

    // Plain Absatz insert.
    let ks2 = keyset("Artikel 395 Absatz 1 der Verordnung (EU) Nr. 575/2013");
    assert!(ks2.contains(&"EU:2013/575:Art.395".to_string()), "{ks2:?}");

    // Enumeration inside the insert ("Absatz 1 und 2").
    let ks3 = keyset("Artikel 5 Absatz 1 und 2 der Verordnung (EU) 2022/2554");
    assert!(ks3.contains(&"EU:2022/2554:Art.5".to_string()), "{ks3:?}");

    // Two chained inserts ("Absatz 1 Nummer 25").
    let ks4 = keyset("Artikel 4 Absatz 1 Nummer 25 der Verordnung (EU) Nr. 575/2013");
    assert!(ks4.contains(&"EU:2013/575:Art.4".to_string()), "{ks4:?}");
}

#[test]
fn eu_long_form_with_subpart_insert_negative() {
    // Free text between the article and the regulation (not a recognized
    // sub-part keyword) must still break the window — only the bare
    // EU-regulation ref (from the separate EU_REG_RE pass) may appear.
    let ks = keys("Artikel 5 gilt entsprechend, wie in der Verordnung (EU) 2016/679 beschrieben");
    assert!(
        ks.iter().all(|r| !r.contains(":Art.")),
        "free text before the regulation must not bind: {ks:?}"
    );
    assert!(ks.contains(&"EU:2016/679".to_string()), "plain regulation ref still emitted");

    // A non-sub-part determiner-like phrase ("des Gesetzes über die …") must
    // not bind either, even though a Verordnung (EU) follows later.
    let ks2 = keys("Artikel 12 des Gesetzes über die Verordnung (EU) Nr. 575/2013");
    assert!(ks2.iter().all(|r| !r.contains(":Art.")), "{ks2:?}");

    // Existing anaphoric/date negatives stay green (regression guard for the
    // sub-part grammar, which must remain closed).
    assert!(keys("Art. 28 dieses Übereinkommens").is_empty());
    assert!(keys("vom 28. Mai 2022").is_empty());
}

// --- equivalent_ref_keys (TOOL_CALLS_V2_PLAN AP4) ----------------------------

mod equivalent_ref_keys_tests {
    use super::super::equivalent_ref_keys;

    #[test]
    fn crr_kuerzel_to_long_form() {
        assert_eq!(
            equivalent_ref_keys("CRR:Art.395"),
            vec!["CRR:Art.395".to_string(), "EU:2013/575:Art.395".to_string()]
        );
    }

    #[test]
    fn crr_long_form_to_kuerzel() {
        assert_eq!(
            equivalent_ref_keys("EU:2013/575:Art.395"),
            vec!["EU:2013/575:Art.395".to_string(), "CRR:Art.395".to_string()]
        );
    }

    #[test]
    fn dora_kuerzel_to_long_form() {
        assert_eq!(
            equivalent_ref_keys("DORA:Art.28"),
            vec!["DORA:Art.28".to_string(), "EU:2022/2554:Art.28".to_string()]
        );
    }

    #[test]
    fn dora_long_form_to_kuerzel() {
        assert_eq!(
            equivalent_ref_keys("EU:2022/2554:Art.28"),
            vec!["EU:2022/2554:Art.28".to_string(), "DORA:Art.28".to_string()]
        );
    }

    #[test]
    fn non_alias_key_returns_only_itself_no_duplicate() {
        // A paragraph key (no Kürzel↔EU alias defined for bare §-forms).
        assert_eq!(equivalent_ref_keys("KWG:§25a"), vec!["KWG:§25a".to_string()]);
        // An EU regulation base key without an article.
        assert_eq!(equivalent_ref_keys("EU:2022/2554"), vec!["EU:2022/2554".to_string()]);
        // An article of an act with no known alias.
        assert_eq!(equivalent_ref_keys("VAG:Art.5"), vec!["VAG:Art.5".to_string()]);
        // A CRR/DORA article number that only accidentally shares the prefix
        // shape of another act's long-form regulation number must still not
        // match — different article numbers are simply different keys, not
        // asserted here beyond exact-key round-trips above.
    }
}

// --- German § long-form act name (TOOL_TIER_PLAN.md Teil B, ISSUES fix) -----
// "§ N des <Langform>[es|s]" with NO Kürzel in the text — e.g. "§ 6 des
// Geldwäschegesetzes" → GWG:§6. Only `§` matches; `Art.` long forms are the
// unrelated EU-regulation binding handled separately.

#[test]
fn long_form_positive_cases() {
    let ks = keyset("die §§ 13 bis 13c, 15 und 17 bis 22 des Kreditwesengesetzes");
    for expected in [
        "KWG:§13", "KWG:§13a", "KWG:§13b", "KWG:§13c", "KWG:§15", "KWG:§17", "KWG:§18",
        "KWG:§19", "KWG:§20", "KWG:§21", "KWG:§22",
    ] {
        assert!(ks.contains(&expected.to_string()), "expected {expected} in {ks:?}");
    }

    assert!(one("nach § 6 des Geldwäschegesetzes", "GWG:§6"));
}

#[test]
fn long_form_negative_cases() {
    // Not a recognized long form at all.
    assert!(keys("nach § 3 des Vertrages").is_empty());
    assert!(keys("gemäß § 5 des Gesetzes").is_empty());
    // A Verordnung (no EU regulation number) is not a German-law long form.
    assert!(keys("§ 7 der Verordnung").is_empty());
}

#[test]
fn long_form_custom_lexicon() {
    use super::{parse_refs_with, RefLexicon};

    let custom = RefLexicon::new(
        vec!["enwg".to_string()],
        vec![("Energiewirtschaftsgesetz".to_string(), "ENWG".to_string())],
    );

    let ks_kuerzel: Vec<String> = parse_refs_with("§ 14 EnWG", &custom)
        .into_iter()
        .map(|r| r.ref_key)
        .collect();
    assert_eq!(ks_kuerzel, vec!["ENWG:§14".to_string()]);

    let ks_long_form: Vec<String> = parse_refs_with("§ 14 des Energiewirtschaftsgesetzes", &custom)
        .into_iter()
        .map(|r| r.ref_key)
        .collect();
    assert_eq!(ks_long_form, vec!["ENWG:§14".to_string()]);

    // Neither form is recognized by the built-in lexicon (no ENWG entry).
    assert!(keys("§ 14 EnWG").is_empty());
    assert!(keys("§ 14 des Energiewirtschaftsgesetzes").is_empty());
}

#[test]
fn long_form_extra_enumeration_precision_guard() {
    // "und" followed by non-numeric text must not be consumed by `extra` —
    // the grammar requires a digit right after "und"/",".
    assert!(one(
        "§ 25a KWG und die dazugehörigen Verwaltungsvorschriften",
        "KWG:§25a"
    ));
}
