# refs/ — Lokaler Kontext

Volle Doku: [HANDBUCH.md](../../../HANDBUCH.md), Abschnitt „refs/" (AP2,
RETRIEVAL_QUALITY_PLAN.md).

Deterministischer (kein LLM, kein Netz) Parser für Normverweise:
`parse_refs(text) -> Vec<NormRef>` (delegiert an `parse_refs_with(text,
&RefLexicon)` — das Kürzel-/Langform-Vokabular ist seit TOOL_TIER Teil B ein
Wert: `RefLexicon::builtin()` = KNOWN + BUILTIN_LONG_FORMS, DB-geladene
Registry via `db::Database::ref_lexicon()`/Tabelle `ref_abbreviations`;
`builtin_seed_entries()` ist die Seed-Quelle für services::seed).
Normalisierte `ref_key`s: `KWG:§25a`, `GWG:§6`, `DORA:Art.28`,
`EU:2022/2554`, `MARISK:AT4.3.2`, `MARISK:BTO1.1`, EU-gebundene Artikel
`EU:2013/575:Art.395` (Langform „Artikel 395 der Verordnung (EU)
Nr. 575/2013", enges Fenster; geschlossene Einschübe — `Absatz 1 Buchstabe c`
etc., von `ARTICLE_RE` konsumiert — überstehen die Bindung, Freitext bricht
sie) sowie §-Langform-Bindung „§§ 13 bis 13c … des Kreditwesengesetzes" →
KWG-Keys (`LONG_FORM_AFTER_PARAGRAPH_RE`, Genitiv nur als `es`/`s`-Suffix).
Bereiche (`§§ 13 bis 13c`, `Artikel 387 bis 410`, Mehrfach-Aufzählungen via
`extra`-Gruppe) expandieren kontrolliert (Cap 30, darüber nur Endpunkte) —
s. HANDBUCH.md-Abschnitt „refs/".

**Leitregel: Präzision vor Recall.** Ein Falsch-Positiv, der auf den falschen
Chunk auflöst, verschmutzt das Retrieval — im Zweifel NICHT matchen. Der
Hauptfilter ist `law_abbrevs.rs` (geschlossene Kürzel-Liste): ein `§ N`/`Art. N`
wird nur emittiert, wenn ein bekanntes Gesetzeskürzel folgt. Anaphorische
Verweise („dieses Artikels/Absatzes"), Seitenzahlen und Datumsangaben dürfen
nie matchen (durch Negativtests abgesichert). Neue Kürzel nur echte Aktennamen
hinzufügen — ein zu breiter Eintrag (Allerweltswort) lässt Cross-Refs durch.

`ref_key` trägt nur die Norm-Identität (Gesetz + §/Art./Modul), nicht Abs./Satz/
Nr. — zwei Chunks mit „§ 25a Abs. 1" und „§ 25a Abs. 3" lösen auf dieselbe Norm
auf. Volle Spans/Sub-Teile bleiben im `NormRef`.

Konsumiert von `db/chunk_refs.rs` (Ableitung + Persistenz).

**Tests:** `tests.rs` (table-driven): ≥20 Positiv- (echte MaRisk/DORA/KWG/GwG-
Formulierungen inkl. Abs./Satz/Nr.), ≥8 Negativfälle. Bei Muster-Änderungen
`cargo test --lib refs` laufen lassen; neue Muster/Kürzel immer mit Positiv- UND
Negativfall ergänzen.
