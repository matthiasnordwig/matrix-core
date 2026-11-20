-- schema_v81: the sources a grid run could not search, kept with the run.
--
--   * sources_not_searched — JSON array of `{id, name, reason}` (reason: not_indexed / indexing /
--     no_documents; services `commands/source_readiness.rs`), the chosen collections a search could
--     not find anything in when the run started. Written by `grid_chat_batch` once per start (a
--     re-run of the same run_id overwrites it); read by MCP `run_grid`/`grid_results` and the
--     status line of the table, also after a restart.
--
-- NULL = every source was searchable, or the run is older than this version.
-- Plain ALTER TABLE ADD COLUMN, runs through db/mod.rs::migrate's default arm.

ALTER TABLE grid_run_meta ADD COLUMN sources_not_searched TEXT NULL;
