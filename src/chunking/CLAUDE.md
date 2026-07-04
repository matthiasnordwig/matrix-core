# chunking/ — Lokaler Kontext

Volle Doku: [HANDBUCH.md](../../../HANDBUCH.md), Abschnitt 1.2 „chunking/".

Index-basierter Ansatz (nicht signatur-/string-basiert): `sentences.rs` →
`window.rs` → LLM → `signatures.rs::assemble()`. `structural.rs` ist der
LLM-freie Layout-Pfad für PDFs (separate Heuristiken, siehe HANDBUCH für Details
zu Line-Gap/Forward-/Backward-Merge).

Vor dem Lesen ganzer Dateien: `grep -n "^pub fn \|^fn " *.rs` zeigt alle
Signaturen im Ordner.
