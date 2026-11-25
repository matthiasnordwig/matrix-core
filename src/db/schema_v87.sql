-- schema_v87: a short running summary of a chat's earlier turns (Ask history that no longer fits the window).
--
--   * chat_sessions.summary — a few sentences summarizing the turns up to and including the message
--     `summary_through` (a chat_messages id); NULL = none yet. Written in the background after an answer
--     by the app (`services/.../commands/chat_summary.rs`), read when the history is fitted into a small
--     window: the summary stands in for the older turns, the newest turns go verbatim.
--
-- Plain ALTER TABLE, runs through db/mod.rs::migrate's default arm.

ALTER TABLE chat_sessions ADD COLUMN summary         TEXT NULL;
ALTER TABLE chat_sessions ADD COLUMN summary_through INTEGER NULL;
