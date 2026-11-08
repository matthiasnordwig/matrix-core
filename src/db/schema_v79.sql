-- schema_v79: Tables lose their fixed search count too — every row searches until the model is
-- done or its context is full, like the chat (services `tool_loop/budget.rs`). The one optional
-- fixed maximum for both is the new setting `agentic_max_searches` (missing = none); the old
-- per-row default (1) is not carried over.
DELETE FROM app_settings WHERE key = 'grid_tool_call_budget_default';
