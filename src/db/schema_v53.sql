-- History-Awareness (Normal-Chat), AP6.
--
-- Persistent chat sessions + turn store for the normal RAG/direct chat. Lets a
-- session be resumed (its prior turns are prepended to the LLM message array so
-- follow-up questions have context) and managed in the UI (list/rename/delete).
--
-- `chat_messages` carries two NULLABLE columns (`tool_calls_json`,
-- `tool_payload_json`) that this AP only CREATES, never populates — they are the
-- store for the later tool-loop rounds + trace inspector (BACKLOG.md "Tool-Calls
-- …" / "Inspector: Trace-View …"). Kept here so the schema is stable before the
-- tool loop lands and does not need another migration then.

CREATE TABLE chat_sessions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    title      TEXT    NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE chat_messages (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id        INTEGER NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    role              TEXT    NOT NULL,
    content           TEXT    NOT NULL,
    -- Populated only by the later tool-loop AP (NULL for plain turns).
    tool_calls_json   TEXT,
    tool_payload_json TEXT,
    created_at        INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX idx_chat_messages_session ON chat_messages(session_id, id);
