//! When and how a document came about (schema_v85): `documents.taken_at_ms`, `tz_offset_min`,
//! `time_source`, `origin` as one [`DocTime`]. Written by whoever knows it — the file time of an
//! upload read before Matrix copies the file, the recording time a video carries, a recording made in
//! Matrix — and never from the copy Matrix keeps (`originals_kept`), whose file time is the upload.

use rusqlite::{params, OptionalExtension};

use super::models::DocTime;
use super::{Database, Result};

impl Database {
    /// Stores the time and origin of document `id` (all four columns; `None` clears one).
    pub fn set_document_time(&self, id: i64, t: &DocTime) -> Result<()> {
        self.conn.execute(
            "UPDATE documents SET taken_at_ms = ?2, tz_offset_min = ?3, time_source = ?4, origin = ?5 WHERE id = ?1",
            params![id, t.taken_at_ms, t.tz_offset_min, t.time_source, t.origin],
        )?;
        Ok(())
    }

    /// The time and origin of document `id`; `None` when there is no such document.
    pub fn document_time(&self, id: i64) -> Result<Option<DocTime>> {
        Ok(self
            .conn
            .query_row(
                "SELECT taken_at_ms, tz_offset_min, time_source, origin FROM documents WHERE id = ?1",
                [id],
                |r| Ok(DocTime { taken_at_ms: r.get(0)?, tz_offset_min: r.get(1)?, time_source: r.get(2)?, origin: r.get(3)? }),
            )
            .optional()?)
    }
}
