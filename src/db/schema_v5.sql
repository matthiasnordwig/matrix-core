-- Matrix Rust Core — migration v5
-- The token "quality window" (pre-chunk size) belongs to the ENDPOINT, not the
-- chunking profile: it is bounded by the endpoint's per-call capacity.

ALTER TABLE llm_endpoints ADD COLUMN window_tokens INTEGER NOT NULL DEFAULT 1500;

-- Seed each endpoint's window to ~60% of its context window (quality buffer).
UPDATE llm_endpoints SET window_tokens = MAX(256, CAST(context_window * 0.6 AS INTEGER));

-- The profile no longer carries the window.
ALTER TABLE chunking_profiles DROP COLUMN window_tokens;
