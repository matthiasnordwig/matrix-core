# chunking/ — Lokaler Kontext

Volle Doku: [HANDBUCH.md](../../../HANDBUCH.md), Abschnitt 1.2 „chunking/".

Index-basierter Ansatz (nicht signatur-/string-basiert): `sentences.rs` →
`window.rs` → LLM → `signatures.rs::assemble()`. `structural.rs` ist der
LLM-freie Layout-Pfad für PDFs (separate Heuristiken, siehe HANDBUCH für Details
zu Line-Gap/Forward-/Backward-Merge).

Vor dem Lesen ganzer Dateien: `grep -n "^pub fn \|^fn " *.rs` zeigt alle
Signaturen im Ordner.

**Tests:** `tests.rs` (`#[cfg(test)] mod tests;` in `mod.rs`) deckt Satz-/Segment-Split,
Abkürzungs-Guard, Fenster-Rendering und `assemble()` ab (Start-Indizes, `leave_out`,
Heading-Vererbung, lenient JSON). `structural.rs` (Line-Gap/Heading-Regex/Backward-
Merge) wird per synthetischen Fixture-PDFs getestet, erzeugt zur Testzeit über
`pdf_oxide::api::Pdf::from_markdown` (keine separate PDF-Writer-Dependency nötig,
da dasselbe Crate schreibt und liest). Bei Änderungen an obigem: Tests zuerst
laufen lassen (`cargo test --lib chunking`), bei neuer Logik ergänzen.
