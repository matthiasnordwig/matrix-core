# embedding/ — Lokaler Kontext

Volle Doku: [HANDBUCH.md](../../../HANDBUCH.md), Abschnitt 1.2 „embedding/".

`trait QueryEmbedder` (mod.rs) wird von `onnx.rs::OrtEmbedder` implementiert.
Retrieval in `retrieval.rs` gruppiert nach Embedding-Raum — **kein** globaler
Cross-Model-Vektor, siehe Designentscheidungen in HANDBUCH.md Abschnitt 2.

Vor dem Lesen ganzer Dateien: `grep -n "^pub fn \|^fn \|^pub struct \|^pub trait " *.rs`.
