# embedding/ — Lokaler Kontext

Volle Doku: [`handbuch/core.md`](../../../handbuch/core.md), Abschnitt „embedding/" (Index: `HANDBUCH.md`).

`trait QueryEmbedder` (mod.rs) wird von `onnx.rs::OrtEmbedder` **und**
`gguf_embed.rs::GgufEmbedder` (MODEL_INFRA_PLAN AP4b, hinter `gguf`-Feature —
Metal via `gguf-metal`) implementiert. GGUF-jina ist ein **eigener
Embedding-Raum** (nie der ONNX-jina-Raum, auch beim identischen Modell — eigene
Modell-ID, `kind local_gguf`, s. HANDBUCH §2). Pooling (`mean_pool`, rein/getestet)
+ L2-Norm; llama.cpp poolt bei `pooling_type=mean` selbst, `mean_pool` bleibt als
testbare Referenz/Fallback. Bridge: `services/localgguf.rs`.
Task-Präfixe (`query_prefix`/`document_prefix`, schema_v59) werden NUR in
`prefix.rs::apply_task_prefix` angewendet (rein/getestet), nie in den Embeddern.
Retrieval in `retrieval.rs` gruppiert nach Embedding-Raum — **kein** globaler
Cross-Model-Vektor, siehe gelockte Designentscheidungen im Index `HANDBUCH.md` §2.
Hybrid (AP1): `retrieve_hybrid_with`/`retrieve_hybrid_batch` fusionieren Vektor- und
FTS5/BM25-Ränge per RRF (`rrf_fuse_weighted`, Parameter und Füllwort-Filter der
Stichwortsuche in `fusion.rs`, k=10 gemessen) — **pro Kontext/Raum, nur über
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

Bild-Embedding (EmbeddingGemma 2): `vision_prep.rs` (rein, Gemma-4-Bildvorverarbeitung,
getestet in `tests_vision.rs`) + `vision.rs::OrtImageEmbedder` (hinter `onnx`): Bild-Encoder
→ `image_features` in dieselbe Text-Session (`OrtEmbedder::embed_with_features`) → gleicher
Vektorraum wie Text; `OrtImageEmbedder::with_text` übernimmt ein vorhandenes
`Arc<OrtEmbedder>` (die App teilt so die gecachte Text-Session, `services/localembed.rs`). Messwerkzeug `core/examples/image_probe.rs` (Seiten-Rendering über
Feature `pdf-render`). Bildsuche: `page_select.rs` (rein: Grafikseiten-Regel,
Seitenanfänge nach Textreparatur, Seite → Chunks; Tests `tests_page_select.rs`),
`page_render.rs` (Feature `pdf-render`: PDF → JPEG der Grafikseiten; Messtabelle
`core/examples/page_stats.rs`), `image_list.rs` (dritte Rangliste im Hybrid-Pfad, Tests
`tests_images.rs` — nur Bildvektoren des Kontext-Modells, Abbildung auf Chunks). Die
Vorschau-Markierung `page_image_id` setzt `retrieve_hybrid_*` nur bei Bildrang < `preview_k`
(= finales top_k; `*_preview`-Varianten für Rerank-Kandidaten-N), Ränge/Scores bleiben gleich.

Vor dem Lesen ganzer Dateien: `grep -n "^pub fn \|^fn \|^pub struct \|^pub trait " *.rs`.

**Tests:** `tests.rs` deckt `retrieve`/`retrieve_with` über gemischte Embedding-Räume
ab (Fake-Embedder, zwei Modelle/Dimensionen — verifiziert, dass nie cross-space
verglichen wird), dazu `rrf_fuse` (reine Fusions-Math), `hybrid_fanout` und
`retrieve_hybrid_with`/`_batch` (Hybrid über gemischte Räume, Cross-Space-Isolation);
Bildliste in `tests_images.rs`.
Bei Änderungen: `cargo test --lib embedding` laufen lassen.
