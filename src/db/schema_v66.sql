-- schema_v66: which model, agentic setting and grid template a grid run used.
--
--   * llm_endpoint_id / pool_id — the selection the run was started with
--     (exactly one set). No FK: the endpoint may be deleted later; history
--     then still shows `model_label`, and loading just skips restoring it.
--   * model_label — endpoint/pool name (+ model id) at run time, for display.
--   * tool_call_budget — 0 = classic retrieval, > 0 = agentic tool loop.
--   * template_name — grid profile name (marked when edited ad hoc).
--   * context_mode — Classic / Hybrid / Strict Graph / …
--
-- All NULL for runs created before this version ("unknown", not "classic").
-- Plain ALTER TABLE ADD COLUMN, runs through db/mod.rs::migrate's default arm.

ALTER TABLE grid_run_meta ADD COLUMN llm_endpoint_id INTEGER NULL;
ALTER TABLE grid_run_meta ADD COLUMN pool_id INTEGER NULL;
ALTER TABLE grid_run_meta ADD COLUMN model_label TEXT NULL;
ALTER TABLE grid_run_meta ADD COLUMN tool_call_budget INTEGER NULL;
ALTER TABLE grid_run_meta ADD COLUMN template_name TEXT NULL;
ALTER TABLE grid_run_meta ADD COLUMN context_mode TEXT NULL;
