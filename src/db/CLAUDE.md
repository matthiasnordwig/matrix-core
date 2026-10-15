# db/ — Lokaler Kontext

Volle Doku: [`handbuch/db.md`](../../../handbuch/db.md) (Schema-Übersicht, Datenflüsse, Navigation;
Index: `HANDBUCH.md`). Einzelne Dateien: Kopfkommentar der Datei bzw. `handbuch/files.md`.

SQLite-Repository (rusqlite, bundled/statisch für iOS). Migrationen sind
`schema_vN.sql`, `mod.rs::migrate()` läuft sie über `PRAGMA user_version`
sequenziell durch — neue Migration = neue Datei mit nächster Nummer, alte
niemals nachträglich ändern. `models.rs` bündelt **alle** Domänen-Typen (kein
Modell in den `*.rs`-Dateien der einzelnen CRUD-Module); einzige Auslagerung: Retrieval-Treffer
(`ScoredChunk`, `HitProvenance`, `ListHit`) in `scored.rs`, von `models.rs` re-exportiert.

`ontology/` ist ein eigenes Ordner-Modul — jede Datei dort hat ein eigenes `impl Database { ... }`.

Vor dem Lesen ganzer Dateien: `grep -n "pub fn " *.rs ontology/*.rs`.

**Tests:** `tests.rs` (CRUD-Round-Trips, FK, Vektor-Blob, Cosine, Eval),
`ontology/tests.rs` + `ontology/tests/*` (Ontologie inkl. Lens-Join, Communities, Merge-Log),
dazu `*_tests.rs` je Thema (FTS, chunk_refs, chunks, chat_sessions, ref_abbreviations,
reranker, rename, embedding_kind, page_images, transfer, context_copy, secret_fields,
llm_endpoint = Endpunkt-Flags/Pool-Invariante/Reasoning-Listen, vision = schema_v62).
Test-`NewContext` am besten als `NewContext { …, ..Default::default() }`. Bei Änderungen `cargo test --lib db` laufen lassen, bei
neuer CRUD-Logik ergänzen.
