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
deckt Profile/Nodes/Edges/Merge (inkl. `ontology_merge_log`: Verlierer-Label/-Typ
überlebt das harte Löschen der Knotenzeile, s. `tests/nodes_edges.rs`), Communities
(Per-Lens-CRUD/Members/Cascades und den `(lens_id, members_key)`-Summary-Cache in
`tests/communities.rs`, plus NULL-Reset bei `delete_communities_for_context`),
Lifecycle-Löschung, `retrieve_graph_with`/`_batch`
(Hop-Expansion, inkl. Lens-Join: aktive-Lens-Typauflösung, `reversed`-Anzeige-Swap,
`deleted`-Traversal-Ausschluss, Per-Lens-Community-Filter), Metrics/Dedup-Cache/Quarantine, das Lens-System
(`get_or_create_lens`-Upsert-in-place, `delete_lens`-Fallback auf `active_lens_id=NULL`)
sowie `raw_*_type`-Mirroring bei Insert/manueller Kuration ab. `ontology/schema_suggestions.rs`
hat eigene `#[cfg(test)] mod tests` am Dateiende (Häufigkeits-Threshold). Bei Änderungen:
`cargo test --lib db` laufen lassen, bei neuer CRUD-Logik ergänzen.
