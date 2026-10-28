//! Provisional collections (schema_v72): files attached to a question in Ask
//! become a collection marked `provisional`, bound to its chat
//! (`provisional_chat_id`). It is searched like any collection, but no list
//! shows it (`list_contexts` filters it out). It ends one of three ways:
//! "Keep as collection" (`keep_provisional_context`: name it, clear the mark),
//! "Discard" (`discard_provisional_context`), or the clean-up when its chat is
//! deleted (`delete_provisional_contexts_of_chat`, called by
//! `delete_chat_session`) or at start-up (`delete_orphan_provisional_contexts`:
//! no chat, or a chat without a single turn — nobody can reach it any more —
//! and older than the given age, so another process starting on the same file
//! never sweeps an attachment that is still being indexed for an unanswered chat).
//!
//! Invariant: every delete here goes through `delete_if_provisional`, which
//! re-checks the mark and deletes in one savepoint — a collection the user made
//! is never deleted by this file.
//!
//! Known window: the chat is bound only after its first saved answer, so a crash
//! before that answer leaves an unbound attachment that a start after the age
//! limit deletes.
//! Only the index is lost then (passages, vectors); the user's files on disk stay.

use rusqlite::{params, OptionalExtension};

use super::models::{Context, NewContext};
use super::{Database, Result};

impl Database {
    /// Create `c` as a provisional collection of chat `chat_id` (`None` = a chat
    /// without history). The name is made unique by appending " 2", " 3", … .
    pub fn create_provisional_context(&self, c: &NewContext, chat_id: Option<i64>) -> Result<Context> {
        self.in_savepoint("provisional_create", || {
            let mut spec = c.clone();
            spec.name = self.free_context_name(&c.name)?;
            let ctx = self.create_context(&spec)?;
            self.conn.execute(
                "UPDATE contexts SET provisional = 1, provisional_chat_id = ?2 WHERE id = ?1",
                params![ctx.id, chat_id],
            )?;
            Ok(ctx)
        })
    }

    /// Whether `id` is a provisional collection.
    pub fn is_provisional_context(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT provisional FROM contexts WHERE id = ?1", [id], |r| r.get::<_, bool>(0))
            .optional()?
            .unwrap_or(false))
    }

    /// The provisional collection of chat `chat_id`, if it has one (the newest).
    pub fn provisional_context_for_chat(&self, chat_id: i64) -> Result<Option<Context>> {
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM contexts WHERE provisional = 1 AND provisional_chat_id = ?1 ORDER BY id DESC LIMIT 1",
                [chat_id],
                |r| r.get(0),
            )
            .optional()?;
        match id {
            Some(id) => self.context(id),
            None => Ok(None),
        }
    }

    /// Bind provisional collection `id` to chat `chat_id` (a chat saved after the
    /// files were attached). Returns false when `id` is not provisional.
    pub fn bind_provisional_context(&self, id: i64, chat_id: i64) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE contexts SET provisional_chat_id = ?2 WHERE id = ?1 AND provisional = 1",
            params![id, chat_id],
        )? > 0)
    }

    /// "Keep as collection": name it `name` and make it an ordinary collection.
    /// Returns false when `id` is not provisional (nothing changes).
    pub fn keep_provisional_context(&self, id: i64, name: &str) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE contexts SET name = ?2, provisional = 0, provisional_chat_id = NULL, updated_at = unixepoch()
             WHERE id = ?1 AND provisional = 1",
            params![id, name],
        )? > 0)
    }

    /// "Discard": delete provisional collection `id` with all its data. Returns
    /// false (and deletes nothing) when `id` is not provisional.
    pub fn discard_provisional_context(&self, id: i64) -> Result<bool> {
        self.delete_if_provisional(id)
    }

    /// The provisional collections of chat `chat_id` go with the chat.
    pub fn delete_provisional_contexts_of_chat(&self, chat_id: i64) -> Result<usize> {
        self.delete_provisional_where("provisional_chat_id = ?1", Some(chat_id))
    }

    /// Start-up sweep: provisional collections created at least `min_age_secs`
    /// ago without a chat, or whose chat has no turn (an attachment nobody asked
    /// about can no longer be reopened). The age spares a young attachment that
    /// a running app is still filling before its first answer binds it.
    pub fn delete_orphan_provisional_contexts(&self, min_age_secs: i64) -> Result<usize> {
        self.delete_provisional_where(
            "created_at <= unixepoch() - ?1
             AND (provisional_chat_id IS NULL
              OR NOT EXISTS (SELECT 1 FROM chat_messages m WHERE m.session_id = contexts.provisional_chat_id))",
            Some(min_age_secs),
        )
    }

    /// Whether any collection — provisional ones included — is called `name`
    /// (`contexts.name` is UNIQUE across both).
    pub fn context_name_taken(&self, name: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM contexts WHERE name = ?1", [name], |_| Ok(()))
            .optional()?
            .is_some())
    }

    fn free_context_name(&self, base: &str) -> Result<String> {
        if !self.context_name_taken(base)? {
            return Ok(base.to_string());
        }
        let mut n = 2;
        loop {
            let candidate = format!("{base} {n}");
            if !self.context_name_taken(&candidate)? {
                return Ok(candidate);
            }
            n += 1;
        }
    }

    fn delete_provisional_where(&self, filter: &str, arg: Option<i64>) -> Result<usize> {
        let sql = format!("SELECT id FROM contexts WHERE provisional = 1 AND {filter}");
        let ids: Vec<i64> = {
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = match arg {
                Some(a) => stmt.query_map([a], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?,
                None => stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?,
            };
            rows
        };
        let mut n = 0;
        for id in ids {
            if self.delete_if_provisional(id)? {
                n += 1;
            }
        }
        Ok(n)
    }

    /// The only delete in this file: re-reads the mark and deletes in one
    /// savepoint, so no other writer on the same file (e.g. the stand-alone MCP
    /// program) can keep the collection in between.
    fn delete_if_provisional(&self, id: i64) -> Result<bool> {
        self.in_savepoint("provisional_delete", || {
            if !self.is_provisional_context(id)? {
                return Ok(false);
            }
            self.delete_context(id)
        })
    }
}
