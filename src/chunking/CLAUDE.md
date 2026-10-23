# chunking/ — Lokaler Kontext

Volle Doku: [`handbuch/core.md`](../../../handbuch/core.md), Abschnitt „chunking/" (Index: `HANDBUCH.md`).

Index-basierter Ansatz (nicht signatur-/string-basiert): `sentences.rs` →
`window.rs` → LLM → `signatures.rs::assemble()`. `structural.rs` ist der
LLM-freie Layout-Pfad für PDFs (separate Heuristiken, siehe `handbuch/core.md` für Details
zu Line-Gap/Forward-/Backward-Merge).

`markdown.rs` ist der LLM-freie Überschriften-Chunker für `.md` (Tests: `markdown_tests.rs`).
`outline.rs` liest die Gliederung (Pfad, ganzer Abschnitt) beim Abruf aus gespeicherten Chunks
zurück, ohne Neuzerlegung (Tests: `outline_tests.rs`).
**Index-Basis:** `char_start`/`char_end` sind **Byte-Offsets** in den Dokumenttext
(`signatures.rs`, `markdown.rs`); `markdown.rs` garantiert zusätzlich
`text[char_start..char_end] == chunk.text`. `structural.rs` zählt in seinem eigenen, nicht
gespeicherten Zeilentext — passt nicht auf `documents.extracted_text` eines PDFs.
`pages.rs`: Seitenwechsel im Chunk (`metadata.page_breaks`, Offsets in den Chunk-Text).

Vor dem Lesen ganzer Dateien: `grep -n "^pub fn \|^fn " *.rs` zeigt alle
Signaturen im Ordner.

**Tests:** `tests.rs` (`#[cfg(test)] mod tests;` in `mod.rs`) deckt Satz-/Segment-Split,
Abkürzungs-Guard, Fenster-Rendering und `assemble()` ab (Start-Indizes, `leave_out`,
Heading-Vererbung, lenient JSON). `structural.rs` (Line-Gap/Heading-Regex/Backward-
Merge) wird per synthetischen Fixture-PDFs getestet, erzeugt zur Testzeit über
`pdf_oxide::api::Pdf::from_markdown` (keine separate PDF-Writer-Dependency nötig,
da dasselbe Crate schreibt und liest). Bei Änderungen an obigem: Tests zuerst
laufen lassen (`cargo test --lib chunking`), bei neuer Logik ergänzen.
