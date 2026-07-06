-- Grid run metadata: the system prompt is byte-identical across all rows of
-- a run (AP7 point 2), so storing it once here instead of duplicating it
-- inside every row's `prompt_snapshot` cuts DB bloat substantially on large
-- grids. No rebuild needed — brand-new table.
CREATE TABLE grid_run_meta (
    run_id        TEXT PRIMARY KEY,
    system_prompt TEXT NOT NULL
);
