//! The stored Markdown text of a media document: `# name`, `## 12:30–14:02 · Speaker 2`,
//! `### On screen 12:28–13:40 (slide, OCR)`, `### Scene 03:10–03:45 (AI description)`,
//! `### Sounds 05:00–05:20: applause (AI)`. Times only in headings, never in passage text
//! (`Speaker 2: …`). Reports the generated ranges and the byte range of each time stretch (what
//! `db/media.rs::sync_frame_ranges` points the stills at). Flow: `handbuch/medien.md`.
//! Scaffold: filled by its work package (`PLAN-ton-video-aufnahme.md` AP2).
