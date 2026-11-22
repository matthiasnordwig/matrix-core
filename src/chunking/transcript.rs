//! Passages from time segments (words with time and speaker, slide/scene/sound units) within the
//! profile's `max_chunk_chars`. Boundaries: change of speaker, a long pause (threshold relative to the
//! file's own pauses), end of sentence (`sentences.rs`) — never inside a sentence, never at a character
//! count; an over-long sentence stays whole. A slide is a passage of its own, scenes and sounds are
//! generated passages. Output carries byte ranges into the text of `media/text.rs` and the `media`
//! metadata (`t0`, `t1`, `kind`, `speakers`, `frame`). Flow: `handbuch/medien.md`.
//! Scaffold: filled by its work package (`PLAN-ton-video-aufnahme.md` AP2).
