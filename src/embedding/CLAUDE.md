# embedding/ — Lokaler Kontext

Volle Doku: [HANDBUCH.md](../../../HANDBUCH.md), Abschnitt 1.2 „embedding/".

`trait QueryEmbedder` (mod.rs) wird von `onnx.rs::OrtEmbedder` implementiert.
Retrieval in `retrieval.rs` gruppiert nach Embedding-Raum — **kein** globaler
Cross-Model-Vektor, siehe Designentscheidungen in HANDBUCH.md Abschnitt 2.
Hybrid (AP1): `retrieve_hybrid_with`/`retrieve_hybrid_batch` fusionieren Vektor- und
FTS5/BM25-Ränge per RRF (`rrf_fuse`, k=60, reine Fn) — **pro Kontext/Raum, nur über
Ränge**, damit die Cross-Space-Isolation erhalten bleibt. FTS-Seite in `db/fts.rs`.

Rerank (AP3): `rerank.rs`. Reine `rank_merge(scores, top_k)` (immer kompiliert,
getestet). `OrtReranker` (hinter `onnx`-Feature, wie `onnx.rs`): `load(model_dir)` +
`score_pairs(query, docs)` — (query,doc)-**Paar** → einzelner Logit (XLMRoberta,
num_labels=1). EP-Wahl spiegelt `onnx.rs` exakt (iOS→CoreML, sonst CPU); **keine**
neuen ort-Features/EPs. Smoke: `core/examples/rerank_smoke.rs`. Settings-Keys
(`reranker_model_dir`/`reranker_enabled_default`) in `db/settings.rs`.

Vor dem Lesen ganzer Dateien: `grep -n "^pub fn \|^fn \|^pub struct \|^pub trait " *.rs`.

**Tests:** `tests.rs` deckt `retrieve`/`retrieve_with` über gemischte Embedding-Räume
ab (Fake-Embedder, zwei Modelle/Dimensionen — verifiziert, dass nie cross-space
verglichen wird), dazu `rrf_fuse` (reine Fusions-Math), `hybrid_fanout` und
`retrieve_hybrid_with`/`_batch` (Hybrid über gemischte Räume, Cross-Space-Isolation).
Bei Änderungen: `cargo test --lib embedding` laufen lassen.
