-- schema_v83: the settings a grid run was started with, as one key.
--
--   * settings_key — hex SHA-256 over every setting that shapes the run's answers (question, answer
--     columns, columns sent to the model, sources, model, context mode, top-k …; services
--     `commands/grid_run_settings.rs`). Written by `grid_chat_batch` once per start. A start under an
--     existing run_id whose key differs gets a new run_id instead, so the earlier run keeps its
--     answers; with the same key (resume, "Retry failed") the run continues.
--
-- NULL = the run is older than this version (a start with settings then always opens a new run).
-- Plain ALTER TABLE ADD COLUMN, runs through db/mod.rs::migrate's default arm.

ALTER TABLE grid_run_meta ADD COLUMN settings_key TEXT NULL;
