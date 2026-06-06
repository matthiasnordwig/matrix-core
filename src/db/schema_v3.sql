-- Matrix Rust Core — migration v3
-- Per-endpoint capacity & rate-limit parameters for the chunking orchestrator.

ALTER TABLE llm_endpoints ADD COLUMN context_window        INTEGER NOT NULL DEFAULT 4096;
ALTER TABLE llm_endpoints ADD COLUMN output_reserve_tokens INTEGER NOT NULL DEFAULT 512;
ALTER TABLE llm_endpoints ADD COLUMN tpm_limit             INTEGER;  -- NULL = unbounded
ALTER TABLE llm_endpoints ADD COLUMN rpm_limit             INTEGER;  -- NULL = unbounded
ALTER TABLE llm_endpoints ADD COLUMN max_concurrency       INTEGER NOT NULL DEFAULT 4;
