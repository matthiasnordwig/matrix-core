-- Matrix Rust Core — migration v4
-- Per-endpoint provider flavor. 'openai' uses /v1/chat/completions (max_tokens);
-- 'ollama' uses the native /api/chat so the app can set options.num_ctx.

ALTER TABLE llm_endpoints ADD COLUMN provider TEXT NOT NULL DEFAULT 'openai';

-- Auto-detect existing local Ollama endpoints by port.
UPDATE llm_endpoints SET provider = 'ollama' WHERE base_url LIKE '%11434%';
