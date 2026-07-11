-- schema_v49: chunk_refs — outgoing legal-norm references per chunk
-- (RETRIEVAL_QUALITY_PLAN.md AP2). Additive only; never touches `chunks`.
--
-- Each row is one (chunk, normalized ref_key) edge derived deterministically
-- from `chunks.text` by `core::refs::parse_refs` (no LLM). Retrieval expansion
-- resolves a hit's outgoing refs to a target chunk and follows them. Rows are
-- cascade-deleted with their chunk or context; the derivation routine is
-- idempotent (delete-then-reinsert per chunk/context), so no UNIQUE is needed
-- for correctness, but the composite index makes the (context_id, ref_key)
-- resolution lookup cheap.

CREATE TABLE chunk_refs (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    chunk_id   INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    context_id INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    ref_key    TEXT NOT NULL
);

CREATE INDEX idx_chunk_refs_ctx_key ON chunk_refs(context_id, ref_key);
CREATE INDEX idx_chunk_refs_chunk ON chunk_refs(chunk_id);
