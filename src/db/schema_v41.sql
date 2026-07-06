-- Store the run's grid-profile JSON schema (`{mode,fields}` string, NULL for
-- plain-text profiles) alongside its system prompt. History loading gates row
-- explosion on the mode the RUN was created with — not on whatever profile the
-- user happens to have selected while loading — so a past array-explode run
-- displays exploded again regardless of the current dropdown. NULL for runs
-- created before this column existed (the frontend falls back to lenient
-- shape-based explosion for those).
ALTER TABLE grid_run_meta ADD COLUMN json_schema TEXT;

-- Mark pre-existing rows with an empty-string sentinel so the frontend can tell
-- a legacy run (mode unknown → lenient shape-based explosion, matching old
-- behavior) apart from a v41+ plain-text run (json_schema stays NULL → never
-- explodes). New inserts write a real schema for JSON profiles or NULL for
-- plain profiles, never ''.
UPDATE grid_run_meta SET json_schema = '' WHERE json_schema IS NULL;
