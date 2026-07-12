-- Tool-Calls (agentic vector search), AP8.
--
-- Two additive columns, both nullable/defaulted so old JSON payloads (web
-- adapter / stored context bundles) still deserialize:
--   * llm_endpoints.supports_tools — endpoint capability flag (analog to
--     is_reasoning). Only endpoints with this set get a `tools` manifest and
--     the recursive tool loop; without it, chat falls back to single-shot RAG.
--   * documents.description — per-file description surfaced in the tool manifest
--     (contexts.description already exists; this is its file-level counterpart).
--
-- Generic migration path (no Rust rebuild hook): a plain ALTER TABLE ADD COLUMN
-- runs through db/mod.rs::migrate's default arm.

ALTER TABLE llm_endpoints ADD COLUMN supports_tools BOOLEAN NOT NULL DEFAULT 0;
ALTER TABLE documents     ADD COLUMN description    TEXT;
