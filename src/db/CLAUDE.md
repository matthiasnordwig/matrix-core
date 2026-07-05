# db/ — Lokaler Kontext

Volle Doku: [HANDBUCH.md](../../../HANDBUCH.md), Abschnitt 1.2 „db/".

SQLite-Repository (rusqlite, bundled/statisch für iOS). Migrationen sind
`schema_vN.sql`, `mod.rs::migrate()` läuft sie über `PRAGMA user_version`
sequenziell durch — neue Migration = neue Datei mit nächster Nummer, alte
niemals nachträglich ändern. `models.rs` bündelt **alle** Domänen-Typen (kein
Modell in den `*.rs`-Dateien der einzelnen CRUD-Module).

`ontology/` ist ein eigenes Ordner-Modul (statt einzelner Datei, war auf 1054
Zeilen gewachsen) — jede Datei dort hat ein eigenes `impl Database { ... }`.

Vor dem Lesen ganzer Dateien: `grep -n "pub fn " *.rs ontology/*.rs`.

**Tests:** `tests.rs` deckt CRUD-Round-Trips, FK-Constraints, Vector-Blob-Round-Trip,
Cosine-Ranking sowie `pools.rs`' `set_pool_members`-Invariante (max. 1 gguf-Mitglied,
atomarer Replace, `position`-Reihenfolge, Cascade-Deletes) ab; `ontology/tests.rs`
deckt Profile/Nodes/Edges/Merge, Communities (inkl. NULL-Reset bei
`delete_communities_for_context`), Lifecycle-Löschung, `retrieve_graph_with`/`_batch`
(Hop-Expansion) sowie Metrics/Dedup-Cache/Quarantine ab. Bei Änderungen:
`cargo test --lib db` laufen lassen, bei neuer CRUD-Logik ergänzen.
