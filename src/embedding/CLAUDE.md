# embedding/ — Lokaler Kontext

Volle Doku: [HANDBUCH.md](../../../HANDBUCH.md), Abschnitt 1.2 „embedding/".

`trait QueryEmbedder` (mod.rs) wird von `onnx.rs::OrtEmbedder` implementiert.
Retrieval in `retrieval.rs` gruppiert nach Embedding-Raum — **kein** globaler
Cross-Model-Vektor, siehe Designentscheidungen in HANDBUCH.md Abschnitt 2.

Vor dem Lesen ganzer Dateien: `grep -n "^pub fn \|^fn \|^pub struct \|^pub trait " *.rs`.

**Tests:** `tests.rs` deckt `retrieve`/`retrieve_with` über gemischte Embedding-Räume
ab (Fake-Embedder, zwei Modelle/Dimensionen — verifiziert, dass nie cross-space
verglichen wird). Bei Änderungen: `cargo test --lib embedding` laufen lassen.
