# embedding/ — Lokaler Kontext

Volle Doku: [HANDBUCH.md](../../../HANDBUCH.md), Abschnitt 1.2 „embedding/".

`trait QueryEmbedder` (mod.rs) wird von `onnx.rs::OrtEmbedder` implementiert.
Retrieval in `retrieval.rs` gruppiert nach Embedding-Raum — **kein** globaler
Cross-Model-Vektor, siehe Designentscheidungen in HANDBUCH.md Abschnitt 2.
Hybrid (AP1): `retrieve_hybrid_with`/`retrieve_hybrid_batch` fusionieren Vektor- und
FTS5/BM25-Ränge per RRF (`rrf_fuse`, k=60, reine Fn) — **pro Kontext/Raum, nur über
Ränge**, damit die Cross-Space-Isolation erhalten bleibt. FTS-Seite in `db/fts.rs`.

Rerank (AP3): `rerank.rs`. Reine `rank_merge(scores, top_k)` (immer kompiliert,
getestet). `OrtReranker` (hinter `onnx`-Feature, wie `onnx.rs`): `load(model_dir,
execution_provider)` + `score_pairs(query, docs)` — (query,doc)-**Paar** → einzelner
Logit (XLMRoberta, num_labels=1). EP-Wahl (MODEL_INFRA_PLAN AP3) spiegelt `onnx.rs`:
per-Modell-Feld `execution_provider` (Default CPU, **auch auf iOS** — kein Runtime-
Force-Override), `Ane`/`Coreml` beide real über `CoreMLComputeUnits` verdrahtet
(`CPUAndNeuralEngine`/`All`). Gemessen: der Reranker (XLM-R, großes Vokabular)
**crasht** mit CoreMLs Default-Compute-Units (`All`/`Coreml`); `Ane` umgeht den
Crash, ist aber langsamer als CPU — Default bleibt CPU. Smoke:
`core/examples/rerank_smoke.rs`. Seit MODEL_INFRA_PLAN AP2 ist der Reranker ein
vollwertiges Modell (`reranker_models`, `schema_v50`, CRUD in `db/registries.rs`) —
Auswahl global über den Settings-Key `active_reranker_id` (`db/settings.rs`;
**ersetzt** `reranker_model_dir`), plus `reranker_enabled_default` (Default-Toggle).
Der Provider-Dispatch (lokal/remote) inkl. EP-Auflösung lebt in
`app/crates/services/src/commands/rerank_provider.rs` (`ActiveReranker::Local{model_dir,
execution_provider}`); Session-Caches (`localembed.rs`/`localrerank.rs`) sind seit AP3
per `(id|dir, execution_provider)` gekeyt, nicht mehr nur per Pfad/Id — eine EP-Änderung
im UI wirkt so ohne App-Neustart.

Vor dem Lesen ganzer Dateien: `grep -n "^pub fn \|^fn \|^pub struct \|^pub trait " *.rs`.

**Tests:** `tests.rs` deckt `retrieve`/`retrieve_with` über gemischte Embedding-Räume
ab (Fake-Embedder, zwei Modelle/Dimensionen — verifiziert, dass nie cross-space
verglichen wird), dazu `rrf_fuse` (reine Fusions-Math), `hybrid_fanout` und
`retrieve_hybrid_with`/`_batch` (Hybrid über gemischte Räume, Cross-Space-Isolation).
Bei Änderungen: `cargo test --lib embedding` laufen lassen.
