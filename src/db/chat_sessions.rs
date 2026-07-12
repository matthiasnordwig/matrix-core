//! CRUD for persistent chat sessions + their turns (`chat_sessions` /
//! `chat_messages`, `schema_v53`, AP6 history-awareness).
//!
//! A session bundles an ordered list of turns; `append_message` bumps the
//! parent session's `updated_at` so the session list sorts most-recent-first.
//! `delete_session` relies on the `ON DELETE CASCADE` FK to drop its messages.
//! The `tool_*` columns exist for the later tool-loop AP and are always inserted
//! as `NULL` here (`append_message` writes plain user/assistant turns only).

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{CoreError, Database, Result};

fn row_to_session(row: &Row<'_>) -> rusqlite::Result<ChatSession> {
    Ok(ChatSession {
        id: row.get("id")?,
        title: row.get("title")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_message(row: &Row<'_>) -> rusqlite::Result<ChatMessage> {
    Ok(ChatMessage {
        id: row.get("id")?,
        session_id: row.get("session_id")?,
        role: row.get("role")?,
        content: row.get("content")?,
        tool_calls_json: row.get("tool_calls_json")?,
        tool_payload_json: row.get("tool_payload_json")?,
        created_at: row.get("created_at")?,
    })
}

impl Database {
    pub fn create_chat_session(&self, title: &str) -> Result<ChatSession> {
        self.conn.execute(
            "INSERT INTO chat_sessions (title) VALUES (?1)",
            params![title],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self.chat_session(id)?.expect("row just inserted must exist"))
    }

    pub fn chat_session(&self, id: i64) -> Result<Option<ChatSession>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM chat_sessions WHERE id = ?1",
                [id],
                row_to_session,
            )
            .optional()?)
    }

    /// Most-recently-updated first (matches the UI session list ordering).
    pub fn list_chat_sessions(&self) -> Result<Vec<ChatSession>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM chat_sessions ORDER BY updated_at DESC, id DESC")?;
        let rows = stmt.query_map([], row_to_session)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn rename_chat_session(&self, id: i64, title: &str) -> Result<ChatSession> {
        self.conn.execute(
            "UPDATE chat_sessions SET title = ?2, updated_at = unixepoch() WHERE id = ?1",
            params![id, title],
        )?;
        self.chat_session(id)?
            .ok_or_else(|| CoreError::NotFound(format!("chat_session {id}")))
    }

    /// Deletes the session; its `chat_messages` cascade via the FK.
    pub fn delete_chat_session(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM chat_sessions WHERE id = ?1", [id])?
            > 0)
    }

    /// Append a plain (non-tool) turn and bump the session's `updated_at`.
    pub fn append_chat_message(
        &self,
        session_id: i64,
        role: &str,
        content: &str,
    ) -> Result<ChatMessage> {
        self.conn.execute(
            "INSERT INTO chat_messages (session_id, role, content) VALUES (?1, ?2, ?3)",
            params![session_id, role, content],
        )?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute(
            "UPDATE chat_sessions SET updated_at = unixepoch() WHERE id = ?1",
            [session_id],
        )?;
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM chat_messages WHERE id = ?1",
                [id],
                row_to_message,
            )
            .optional()?
            .expect("row just inserted must exist"))
    }

    /// The session's turns in chronological (insertion) order.
    pub fn chat_messages_for_session(&self, session_id: i64) -> Result<Vec<ChatMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM chat_messages WHERE session_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([session_id], row_to_message)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
#[path = "chat_sessions_tests.rs"]
mod chat_sessions_tests;
