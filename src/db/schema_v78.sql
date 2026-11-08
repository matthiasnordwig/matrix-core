-- schema_v78: local GGUF chat models choose their context at every load (services
-- `local_context.rs`: the model's limit, what memory is free now, or a fixed size). 0 = automatic,
-- > 0 = fixed. Every stored size so far came from a preset or the downloader's estimate for this
-- machine — not distinguishable from one set on purpose — so all start on automatic.
-- The chat's search-call budget is gone with it (the loop ends by context fullness).
UPDATE llm_endpoints SET context_window = 0 WHERE provider = 'gguf';
DELETE FROM app_settings WHERE key = 'chat_tool_call_budget_default';
