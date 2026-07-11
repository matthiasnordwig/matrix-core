# refs/ — Lokaler Kontext

Volle Doku: [HANDBUCH.md](../../../HANDBUCH.md), Abschnitt „refs/" (AP2,
RETRIEVAL_QUALITY_PLAN.md).

Deterministischer (kein LLM, kein Netz) Parser für Normverweise:
`parse_refs(text) -> Vec<NormRef>`. Normalisierte `ref_key`s: `KWG:§25a`,
`GWG:§6`, `DORA:Art.28`, `EU:2022/2554`, `MARISK:AT4.3.2`, `MARISK:BTO1.1`.

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
