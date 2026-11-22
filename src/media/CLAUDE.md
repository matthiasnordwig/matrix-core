# media/ — Lokaler Kontext

Volle Doku: [`handbuch/medien.md`](../../../handbuch/medien.md) (Ablauf Ton/Video, Datenmodell schema_v85,
Entscheidungen; Index: `HANDBUCH.md`).

Reine Logik zeitbasierter Dokumente ohne Netz/OS: gespeicherter Text (`text.rs`), Sprecher-Clustering
(`diarize_cluster.rs`), Standbild-Analyse (`scenes`, `frame_kind`, `slide_track`, `quad`, `rectify`).
Dekodieren, Modelle und der Job liegen in `app/crates/services/src/{media,speech}/`. Passagen aus Zeitsegmenten:
`chunking/transcript.rs`; CRUD: `db/media.rs`, Zeit/Herkunft je Dokument: `db/doc_time.rs`.
