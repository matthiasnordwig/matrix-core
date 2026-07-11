# db/ — Lokaler Kontext

Volle Doku: [HANDBUCH.md](../../../HANDBUCH.md), Abschnitt 1.2 „db/".

SQLite-Repository (rusqlite, bundled/statisch für iOS). Migrationen sind
`schema_vN.sql`, `mod.rs::migrate()` läuft sie über `PRAGMA user_version`
sequenziell durch — neue Migration = neue Datei mit nächster Nummer, alte
niemals nachträglich ändern. `models.rs` bündelt **alle** Domänen-Typen (kein
Modell in den `*.rs`-Dateien der einzelnen CRUD-Module).

`ontology/` ist ein eigenes Ordner-Modul (statt einzelner Datei, war auf 1054
Zeilen gewachsen) — jede Datei dort hat ein eigenes `impl Database { ... }`.

`fts.rs` (`schema_v48`): FTS5-Keyword-Index `chunks_fts` (externe Content-Tabelle
über `chunks(text)`, Sync-Trigger + Backfill in der Migration). `keyword_search_context`
liefert BM25-Ränge, `escape_fts_query` quotet Terme defensiv (§/Punkte/Spaces).
Wird von `embedding/retrieval.rs`' Hybrid-Pfad (RRF) konsumiert.

`chunk_refs.rs` (`schema_v49`, AP2): Normverweis-Kanten `chunk_refs(chunk_id,
context_id, ref_key)` (Index `(context_id, ref_key)`, FK-Cascade auf chunks+contexts).
Ableitung aus `chunks.text` über `crate::refs::parse_refs`: `set_chunk_refs`
(pro Chunk, delete-then-insert = idempotent), `rebuild_chunk_refs(context_id)`
(kontextweit, idempotent). Auflösung `resolve_ref_target` bevorzugt die
Definitions-Stelle (Signatur trägt den Ref bzw. Text beginnt damit), sonst
ref-dichtester/frühester Erwähnungs-Chunk (`pick_definition_site`, reine Fn).
Reine Expansion-Kapp-Logik `expand_with_refs` (≤1 Zusatz je Treffer, gesamt
≤⌈top_k/2⌉, nie Primärtreffer/Duplikate). Von `services::commands::retrieval`
konsumiert.

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
sowie `raw_*_type`-Mirroring bei Insert/manueller Kuration ab. Das Retrieval-Eval
(`eval.rs`, `schema_v47`) ist über Golden-Set/Entry- und Run/Result-Round-Trips
inkl. FK-Cascade abgedeckt. `ontology/schema_suggestions.rs`
hat eigene `#[cfg(test)] mod tests` am Dateiende (Häufigkeits-Threshold). FTS5
(`fts.rs`/`schema_v48`) ist über `db/fts_tests.rs` abgedeckt: Verfügbarkeits-Smoke-Test,
INSERT/UPDATE/DELETE-Trigger-Sync, Kontext-Scoping + Escaping; `fts.rs` selbst hat
`escape_fts_query`-Unit-Tests am Dateiende. `chunk_refs.rs`/`schema_v49` ist über
`db/chunk_refs_tests.rs` abgedeckt (Round-Trip, Idempotenz von `set_chunk_refs`/
`rebuild_chunk_refs`, Cascade, Definitions-Stellen-Auflösung + reine
`pick_definition_site`/`expand_with_refs`-Kapp-Logik). Bei Änderungen:
`cargo test --lib db` laufen lassen, bei neuer CRUD-Logik ergänzen.
