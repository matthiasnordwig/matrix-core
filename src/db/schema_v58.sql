-- Agentic tool-loop eval trace: persisted trace envelope per eval run result.
--
-- `trace_json` holds the serialized {v:1, fell_back, prompt_snapshot, trace}
-- envelope produced by an agentic (tool-loop) eval entry so the frontend can
-- re-render the exact tool-call trace (PromptInspector) after the fact.
-- NULL for classic (non-agentic) runs and for rows persisted before v58.
ALTER TABLE eval_run_results ADD COLUMN trace_json TEXT;
