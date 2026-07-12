-- TOOL_TIER_PLAN.md AP1 — "Advanced tool use" endpoint lever.
--
-- Model strength is a property of the endpoint (the model behind it), not of a
-- single chat. `tools_advanced` gates the richer tool-loop mechanics (extra
-- tool `find_citing_chunks`, larger TOOL_TEXT_CAP_CHARS, ToolChoice::Auto from
-- round 0 instead of a forced first `search_context`) for endpoints whose
-- model handles a larger tool surface reliably. Default 0 = today's behaviour
-- for every existing endpoint, so the doc_ids Golden-Set gate (basic tier)
-- stays valid unchanged.
--
-- Generic migration path (no Rust rebuild hook): a plain ALTER TABLE ADD COLUMN
-- runs through db/mod.rs::migrate's default arm, same as schema_v54's
-- `supports_tools`.

ALTER TABLE llm_endpoints ADD COLUMN tools_advanced INTEGER NOT NULL DEFAULT 0;
