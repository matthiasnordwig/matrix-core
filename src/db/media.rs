//! CRUD of the time-based documents (schema_v85): `media_documents` (kind, duration, speakers, phase
//! state for resuming) and `media_segments` (speech, subtitles, slides, scenes, sounds with their time
//! range), plus `sync_frame_ranges`, which points every still of a video (`page_images` with
//! `t_start_ms`/`t_end_ms`) at the byte range of the text of its time after each re-cut, so an image hit
//! keeps landing on the passage of the same moment. Flow and invariants: `handbuch/medien.md`.
//! Scaffold: filled by its work package (`PLAN-ton-video-aufnahme.md` AP1).
