-- schema_v72: provisional collections — files attached to a question in Ask
-- before the user made a collection of them. `provisional` = 1 keeps the
-- collection out of every list (`list_contexts`: Library, MCP/assistant,
-- samples, graph, tables); `provisional_chat_id` is the chat it belongs to
-- (NULL = a chat without history, or the chat is gone). "Keep as collection"
-- clears both; "Discard", deleting the chat and the start-up sweep delete
-- provisional collections only (`provisional.rs`).
ALTER TABLE contexts ADD COLUMN provisional INTEGER NOT NULL DEFAULT 0;
ALTER TABLE contexts ADD COLUMN provisional_chat_id INTEGER REFERENCES chat_sessions(id) ON DELETE SET NULL;
CREATE INDEX idx_contexts_provisional_chat ON contexts(provisional_chat_id) WHERE provisional = 1;
