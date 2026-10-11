-- Contexts left as 'embedded' after their embedding model was deleted.
--
-- Deleting an embedding model cascades its vectors away and sets
-- `contexts.embedding_model_id` to NULL, but the context status stayed
-- 'embedded' — agents (MCP list_contexts) and the UI then treated a context
-- without a single vector as searchable. `delete_embedding_model` now resets
-- the status itself; this repairs databases where it already happened.
UPDATE contexts SET status = 'staged'
 WHERE status = 'embedded' AND embedding_model_id IS NULL;
