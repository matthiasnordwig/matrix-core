-- Matrix Rust Core — migration v6
-- Embedding endpoints get rate/throughput limits, analogous to llm_endpoints.
-- Remote (Ollama/OpenAI-compatible) endpoints honor these; local ONNX ignores
-- them entirely (its only constraint is thermal). max_concurrency defaults to 1.

ALTER TABLE embedding_models ADD COLUMN tpm_limit INTEGER;
ALTER TABLE embedding_models ADD COLUMN rpm_limit INTEGER;
ALTER TABLE embedding_models ADD COLUMN max_concurrency INTEGER NOT NULL DEFAULT 1;
