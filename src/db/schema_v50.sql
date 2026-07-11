-- MODEL_INFRA_PLAN.md AP2 "Reranker als vollwertiges Modell (lokal + extern)".
-- Promotes the reranker from a single `reranker_model_dir` setting to a
-- first-class registry, mirroring `embedding_models` (kind local_onnx|remote_api,
-- api_config, execution_provider). A separate table on purpose: rerank API
-- semantics differ from embedding (query+documents -> relevance scores).
--
-- Selection is a single GLOBAL setting `active_reranker_id` (context-independent:
-- the reranker works on raw chunk text, not in any embedding space, so there is
-- deliberately NO per-context binding — do not add one). The row-level
-- migration of an existing `reranker_model_dir` into an active local_onnx row
-- lives in the `target == 50` Rust hook in `mod.rs::migrate` (needs to read the
-- old setting), so this file only creates the table.
CREATE TABLE reranker_models (
    id                 INTEGER PRIMARY KEY,
    name               TEXT NOT NULL,
    kind               TEXT NOT NULL CHECK (kind IN ('local_onnx', 'remote_api')),
    model_dir          TEXT,
    -- JSON: {base_url, model, key_ref, api_format}. key_ref carries the API key
    -- value inline (same as remote embedding models' inline api_key); api_format
    -- v1 only knows the Jina/Cohere/TEI-compatible format.
    api_config         TEXT,
    execution_provider TEXT CHECK (execution_provider IN ('ane', 'coreml', 'cpu')),
    created_at         INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
