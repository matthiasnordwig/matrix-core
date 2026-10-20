# db/ — Lokaler Kontext

Volle Doku: [`handbuch/db.md`](../../../handbuch/db.md) (Schema-Übersicht, Datenflüsse, Navigation;
Index: `HANDBUCH.md`). Einzelne Dateien: Kopfkommentar der Datei bzw. `handbuch/files.md`.

SQLite-Repository (rusqlite, bundled/statisch für iOS). Migrationen sind
`schema_vN.sql`, `mod.rs::migrate()` läuft sie über `PRAGMA user_version`
sequenziell durch — neue Migration = neue Datei mit nächster Nummer, alte
niemals nachträglich ändern. `models.rs` bündelt **alle** Domänen-Typen (kein
Modell in den `*.rs`-Dateien der einzelnen CRUD-Module); Auslagerungen (models.rs steht am Deckel):
Retrieval-Treffer (`ScoredChunk`, `HitProvenance`, `ListHit`) in `scored.rs`, Tabellenblatt-Typen in
`sheet_models.rs`, Grid-Verlauf (`GridRun`, `GridRunMeta`, `GridRunInfo`) in `grid_run_models.rs`,
alle von `models.rs` re-exportiert. `sheet_query.rs` ist die einzige Stelle, die
fremdes SQL ausführt (eigene Nur-Lese-Verbindung + Authorizer) — Sperren dort nie lockern, ohne
`sheet_query_tests.rs` zu erweitern.

`ontology/` ist ein eigenes Ordner-Modul — jede Datei dort hat ein eigenes `impl Database { ... }`.
Chunks eines Dokuments neu anlegen nur über `replace_document_chunks` (`ontology/evidence_carry_db.rs`), sonst gehen
die Belege des Graphen verloren; Transaktionen, die darin laufen können, als Savepoint (`savepoint.rs`).

Vor dem Lesen ganzer Dateien: `grep -n "pub fn " *.rs ontology/*.rs`.

**Tests:** `tests.rs` (CRUD-Round-Trips, FK, Vektor-Blob, Cosine, Eval),
`ontology/tests.rs` + `ontology/tests/*` (Ontologie inkl. Lens-Join, Communities, Merge-Log, Belege umhängen/parken = `evidence_carry{,_db}.rs`),
dazu `*_tests.rs` je Thema (FTS, chunk_refs, chunks, chat_sessions, ref_abbreviations,
reranker, rename, embedding_kind, page_images, transfer, context_copy, secret_fields,
llm_endpoint = Endpunkt-Flags/Pool-Invariante/Reasoning-Listen, vision = schema_v62, mmproj = schema_v65, grid_run_info = schema_v66, graph_vector_migration = schema_v67 (Herkunft der Graph-Vektoren; Leser-Filter in `ontology/tests/graph_vectors.rs`),
sheet_tables = schema_v63/Anlage/Löschen, sheet_query = Sperren/Zeitlimit/Deckel).
Test-`NewContext` am besten als `NewContext { …, ..Default::default() }`. Bei Änderungen `cargo test --lib db` laufen lassen, bei
neuer CRUD-Logik ergänzen.
