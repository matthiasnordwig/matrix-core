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

`chunks.rs` trägt neben Prechunk-/Chunk-CRUD auch `complete_section_chunks(chunk_id,
max_extra)` → `SectionContinuation{chunks, continues_at}` (TOOL_CALLS_V2 AP2):
Folge-Chunks desselben Dokuments mit fortlaufendem `chunk_index` und exakt
gleicher non-empty `metadata.section`, gegen Fragmentierung mehrteiliger
§§/Artikel; `max_extra=0` = reiner Existenz-Check. `chunk_section(&Chunk) ->
Option<String>` (parst `metadata` als JSON, non-empty `section` oder None) ist
`pub(crate)` und wird auch von `chunk_refs.rs` genutzt. Tests: `db/chunks_tests.rs`.

`chunk_refs.rs` (`schema_v49`, AP2): Normverweis-Kanten `chunk_refs(chunk_id,
context_id, ref_key)` (Index `(context_id, ref_key)`, FK-Cascade auf chunks+contexts).
Ableitung aus `chunks.text` über `crate::refs::parse_refs`: `set_chunk_refs`
(pro Chunk, delete-then-insert = idempotent), `rebuild_chunk_refs(context_id)`
(kontextweit, idempotent). Auflösung `resolve_ref_target` bevorzugt die
Definitions-Stelle (Signatur trägt den Ref wortgrenzen-genau, Text beginnt
damit, oder `metadata.section` trägt ihn wortgrenzen-genau — strukturelle
Chunker wie beim CRR-PDF legen die Artikel-Überschrift nur dort ab, Befund
2026-07-12; Kürzel-Konflikt disqualifiziert in allen drei Fällen), sonst
ref-dichtester/frühester Erwähnungs-Chunk (`pick_definition_site`, reine Fn).
EU-gebundene Artikel-Keys (`EU:2013/575:Art.395`) haben eine zweite
Kandidatenquelle (`eu_article_def_candidates`): definitions-förmige Chunks aus
dem Dokument der Verordnung selbst (identifiziert über frühe Chunks mit dem
Basis-Key, Definitionsform auch über `metadata.section`, SQL-Prefilter
inkludiert `metadata`) verdrängen Zitier-Erwähnungen — Details HANDBUCH.md
§1.2 `chunk_refs.rs`.
Reine Expansion-Kapp-Logik `expand_with_refs` (≤1 Zusatz je Treffer, gesamt
≤⌈top_k/2⌉, nie Primärtreffer/Duplikate). Von `services::commands::retrieval`
konsumiert.

`schema_v51` (MODEL_INFRA_PLAN AP4b): weitet `embedding_models.kind`s CHECK auf
`local_gguf` (GGUF/llama.cpp-Embedder). Table-Rebuild im Rust-Hook
`migrate_v50_to_v51_embedding_kind` (die `.sql` ist No-op), **FK-Enforcement aus**
um den Rebuild (sonst löschen die ON-DELETE-Aktionen der Kinder
`contexts`/`embeddings`/`ontology_type_vector_cache` deren Zeilen), `id`/Spalten
verbatim erhalten, `foreign_key_check`-Guard, idempotent. Tests:
`embedding_kind_tests.rs`.

`registries.rs` (`schema_v50`, MODEL_INFRA_PLAN AP2): zusätzlich zu `embedding_models`/
`llm_endpoints` jetzt CRUD für `reranker_models` (`RerankerModel`, kind local_onnx|remote_api,
`model_dir`/`api_config`/`execution_provider`) + `active_reranker_model()` (via Settings-Key
`active_reranker_id`). Auswahl **global** (kein Pro-Kontext-Bezug). `schema_v50` + der
`target==50`-Hook in `mod.rs::migrate` migrieren ein altes `reranker_model_dir`-Setting in
eine aktive `local_onnx`-Zeile (idempotent). `settings.rs` hat dafür `clear_setting`.

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
`pick_definition_site`/`expand_with_refs`-Kapp-Logik). `reranker_models`/`schema_v50`
(MODEL_INFRA_PLAN AP2) ist über `db/reranker_tests.rs` abgedeckt: CRUD-Round-Trip
(lokal+remote), `active_reranker_model()`-Helper + `delete`-räumt-`active_reranker_id`,
und die `schema_v50`-Migration (altes `reranker_model_dir` → aktive `local_onnx`-Zeile,
idempotent, No-op ohne/mit leerem Alt-Setting). Bei Änderungen:
`cargo test --lib db` laufen lassen, bei neuer CRUD-Logik ergänzen.
