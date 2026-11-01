//! Passages that repeat one already held in the collection (a mail filed twice, a block quoted in
//! every reply). The repeat keeps its row, marked omitted, so its document stays whole; search and
//! the embedder skip it, and `duplicate_of` names the passage that stands for it.
//!
//! Deleting the standing passage hands its vector to a repeat in another document, so nothing has
//! to be embedded again and the text stays findable.

use std::collections::HashMap;

use rusqlite::{params, OptionalExtension};

use super::models::{Chunk, NewChunk};
use super::{Database, Result};

impl Database {
    /// Stores `c`; with a `key` an earlier passage of that key in the collection makes it a repeat.
    pub fn create_chunk_once(&self, c: &NewChunk, key: Option<&str>) -> Result<Chunk> {
        let Some(key) = key else { return self.create_chunk(c) };
        let held: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM chunks WHERE context_id = ?1 AND text_key = ?2 AND duplicate_of IS NULL LIMIT 1",
                params![c.context_id, key],
                |r| r.get(0),
            )
            .optional()?;
        let id = self.create_chunk(c)?.id;
        match held {
            Some(first) => self.conn.execute("UPDATE chunks SET is_omitted = 1, duplicate_of = ?2 WHERE id = ?1", params![id, first])?,
            None => self.conn.execute("UPDATE chunks SET text_key = ?2 WHERE id = ?1", params![id, key])?,
        };
        Ok(self.chunk(id)?.expect("row just inserted must exist"))
    }

    /// Where else each passage is filed: document names by chunk id, for hits to name all of them.
    pub fn duplicate_homes(&self, chunk_ids: &[i64]) -> Result<HashMap<i64, Vec<String>>> {
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(d.zip_entry, d.name) FROM chunks c JOIN documents d ON d.id = c.document_id \
             WHERE c.duplicate_of = ?1 ORDER BY c.id",
        )?;
        let mut homes = HashMap::new();
        for &id in chunk_ids {
            let names = stmt.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
            if !names.is_empty() {
                homes.insert(id, names);
            }
        }
        Ok(homes)
    }

    /// Before document `id` goes: its passages with repeats elsewhere hand over to the first of them.
    pub(super) fn promote_duplicates(&self, id: i64) -> Result<()> {
        let held: Vec<i64> = {
            let mut stmt = self.conn.prepare(
                "SELECT DISTINCT c.id FROM chunks c JOIN chunks r ON r.duplicate_of = c.id \
                 WHERE c.document_id = ?1 AND r.document_id <> ?1",
            )?;
            let rows = stmt.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<i64>>>()?;
            rows
        };
        for old in held {
            let (heir, document): (i64, i64) = self.conn.query_row(
                "SELECT id, document_id FROM chunks WHERE duplicate_of = ?1 AND document_id <> ?2 ORDER BY id LIMIT 1",
                params![old, id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            self.conn.execute(
                "UPDATE chunks SET is_omitted = 0, duplicate_of = NULL, \
                 text_key = (SELECT text_key FROM chunks WHERE id = ?2) WHERE id = ?1",
                params![heir, old],
            )?;
            self.conn.execute("UPDATE chunks SET text_key = NULL WHERE id = ?1", [old])?;
            self.conn.execute(
                "UPDATE chunks SET duplicate_of = ?1 WHERE duplicate_of = ?2 AND document_id <> ?3",
                params![heir, old, id],
            )?;
            self.conn.execute(
                "UPDATE embeddings SET chunk_id = ?1, document_id = ?2 WHERE chunk_id = ?3",
                params![heir, document, old],
            )?;
        }
        Ok(())
    }

    pub fn set_document_chunker(&self, id: i64, chunker: &str) -> Result<()> {
        self.conn.execute("UPDATE documents SET chunker = ?2 WHERE id = ?1", params![id, chunker])?;
        Ok(())
    }
}
