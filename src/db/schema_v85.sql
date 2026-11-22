-- schema_v85: time-based documents (audio, video) and when/where every document was made.
--
--   * documents.taken_at_ms / tz_offset_min / time_source / origin — when a document was recorded or made
--     (UTC milliseconds), the local-time offset then (minutes), where that time came from (`quicktime`,
--     `mp4_header`, `file_mtime`, `capture`, `mail`, …) and how it came into Matrix (`captured_in_matrix`,
--     `uploaded`, …). NULL = unknown; never the time of the copy Matrix keeps (`originals_kept`).
--     Written through `doc_time.rs`.
--   * media_documents — one row per audio/video document: kind, duration, picture size, main language,
--     which engine wrote which stretch (`engines`, JSON), the speakers (`speakers`, JSON [{id,label,name}]),
--     the state of every processing phase for resuming (`phases`, JSON phase → {state, done_ms}) and
--     whether the whole file is done (`complete`).
--   * media_segments — the expensive results (speech with words and speaker, subtitles, slides, scenes,
--     sounds) kept apart from the passages, so re-cutting, renaming a speaker or a profile change never
--     transcribes again. Searches read only `chunks`.
--   * page_images.t_start_ms / t_end_ms — a still of a video is a page image visible from … to.
--
-- Plain ALTER TABLE / CREATE TABLE, runs through db/mod.rs::migrate's default arm.

ALTER TABLE documents ADD COLUMN taken_at_ms   INTEGER NULL;
ALTER TABLE documents ADD COLUMN tz_offset_min INTEGER NULL;
ALTER TABLE documents ADD COLUMN time_source   TEXT NULL;
ALTER TABLE documents ADD COLUMN origin        TEXT NULL;

CREATE TABLE media_documents (
    document_id INTEGER PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,              -- audio | video
    duration_ms INTEGER NOT NULL,
    width       INTEGER NULL,
    height      INTEGER NULL,
    language    TEXT NULL,
    engines     TEXT NOT NULL DEFAULT '{}',
    speakers    TEXT NOT NULL DEFAULT '[]',
    phases      TEXT NOT NULL DEFAULT '{}',
    complete    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE media_segments (
    id            INTEGER PRIMARY KEY,
    document_id   INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    t_start_ms    INTEGER NOT NULL,
    t_end_ms      INTEGER NOT NULL,
    kind          TEXT NOT NULL,            -- speech | subtitle | slide | scene | sound
    speaker       INTEGER NULL,
    text          TEXT NOT NULL,
    words         TEXT NULL,
    language      TEXT NULL,
    confidence    REAL NULL,
    page_image_id INTEGER NULL REFERENCES page_images(id) ON DELETE SET NULL,
    generated     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_media_segments_doc_time ON media_segments(document_id, t_start_ms);

ALTER TABLE page_images ADD COLUMN t_start_ms INTEGER NULL;
ALTER TABLE page_images ADD COLUMN t_end_ms   INTEGER NULL;
