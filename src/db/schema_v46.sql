-- Reasoning-effort allow-lists: a data-driven, Profile-tab-maintained set of the
-- reasoning levels a given model family accepts (e.g. gpt-5 knows "minimal",
-- o-series does not). Assigned to an llm_endpoint; the ontology EXTRACTION phase
-- reads the effort level off the context and clamps it against the runtime
-- endpoint's list before sending. See services `reasoning.rs` / extract `run.rs`.
CREATE TABLE IF NOT EXISTS reasoning_effort_lists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    description TEXT,
    allowed_efforts TEXT NOT NULL DEFAULT '[]', -- JSON array of level strings
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Which allow-list applies to a given endpoint's model (NULL = none assigned).
ALTER TABLE llm_endpoints ADD COLUMN reasoning_list_id INTEGER
    REFERENCES reasoning_effort_lists(id) ON DELETE SET NULL;

-- The reasoning effort a context requests for the EXTRACTION phase only
-- (NULL = unset = provider default). Validated at send-time against the
-- extraction endpoint's assigned list.
ALTER TABLE contexts ADD COLUMN ontology_extract_reasoning_effort TEXT;
