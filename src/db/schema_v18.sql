CREATE TABLE ontology_phase_metrics (
    id INTEGER PRIMARY KEY,
    phase_name TEXT NOT NULL,
    model_name TEXT NOT NULL,
    ms_per_chunk REAL NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

ALTER TABLE ontology_profiles ADD COLUMN extract_prompt TEXT;
ALTER TABLE ontology_profiles ADD COLUMN dedup_prompt TEXT;
ALTER TABLE ontology_profiles ADD COLUMN community_prompt TEXT;
