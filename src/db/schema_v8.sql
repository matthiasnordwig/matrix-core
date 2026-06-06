-- schema_v8.sql
-- Add structural_profiles table and link it to contexts

CREATE TABLE IF NOT EXISTS structural_profiles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    definition_triggers TEXT NOT NULL,
    heading_triggers TEXT NOT NULL,
    ignore_patterns TEXT NOT NULL,
    target_chunk_size INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

ALTER TABLE contexts ADD COLUMN structural_profile_id INTEGER REFERENCES structural_profiles(id) ON DELETE SET NULL;
