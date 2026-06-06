-- schema_v9.sql
-- Add min/max chunk size limits and flexible structural patterns

ALTER TABLE structural_profiles ADD COLUMN min_chunk_chars INTEGER NOT NULL DEFAULT 200;
ALTER TABLE structural_profiles ADD COLUMN max_chunk_chars INTEGER NOT NULL DEFAULT 3000;

CREATE TABLE IF NOT EXISTS structural_patterns (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id   INTEGER NOT NULL REFERENCES structural_profiles(id) ON DELETE CASCADE,
    group_name   TEXT NOT NULL,
    role         TEXT NOT NULL,
    regex        TEXT NOT NULL,
    flags        TEXT NOT NULL DEFAULT 'i',
    priority     INTEGER NOT NULL DEFAULT 0,
    label        TEXT,
    sort_order   INTEGER NOT NULL DEFAULT 0
);
