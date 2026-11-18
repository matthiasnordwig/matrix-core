# embedding/ — Lokaler Kontext

Volle Doku: [`handbuch/core.md`](../../../handbuch/core.md), Abschnitt „embedding/" (Index: `HANDBUCH.md`).
Zuständigkeit, Messwerte und Fallen je Datei stehen in den Kopfkommentaren.

Harte Regeln (Embedding-Raum-Isolation, gelockt in `HANDBUCH.md` §2):
- Kein globaler Cross-Model-Vektor: `retrieval.rs` gruppiert nach Embedding-Raum, gemerged werden nur Scores.
- Hybrid- und Bildfusion (`fusion.rs`, `image_list.rs`, `provenance.rs`) nur pro Kontext/Raum und nur über Ränge;
  Bildvektoren immer als eigene Rangliste, nie in derselben Kosinus-Liste wie Text.
- GGUF-jina ist ein eigener Raum, auch beim identischen Modell (eigene Modell-ID, `kind local_gguf`).
- Task-Präfixe nur in `prefix.rs::apply_task_prefix`, nie in den Embeddern.

Vor dem Lesen ganzer Dateien: `grep -n "^pub fn \|^fn \|^pub struct \|^pub trait " *.rs`.

**Tests:** bei Änderungen `cargo test --lib embedding` (in `core/`). `tests.rs` prüft mit Fake-Embedder über zwei
Modelle/Dimensionen, dass nie cross-space verglichen wird (auch hybrid); Bildliste `tests_images.rs`, Herkunft
`tests_provenance.rs`, Sprachen `tests_language.rs`. Smoke Reranker: `core/examples/rerank_smoke.rs`.
