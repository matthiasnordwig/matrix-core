//! Chunk kinds that matter beyond the chunker: `metadata.kind` of spreadsheet
//! chunks (written by the services crate's sheet chunker). A `sheet_profile`
//! chunk is a computed overview of a sheet (column types, value ranges), not
//! text a person wrote, so the graph extraction must not read it: the
//! extraction work-list and its counts ([`Database::list_extractable_chunks`],
//! [`Database::count_extractable_chunks`], `count_unextracted_chunks`) leave it
//! out, which keeps "all chunks extracted" true without ever marking it
//! extracted. Retrieval and embedding treat it like any chunk;
//! [`Database::find_sheet_profile_chunk`] fetches the profile that belongs to a
//! sheet's row chunks.

use rusqlite::{params, OptionalExtension};

use super::chunks::row_to_chunk;
use super::models::Chunk;
use super::{Database, Result};

/// `metadata.kind` of the computed sheet overview chunk.
pub const KIND_SHEET_PROFILE: &str = "sheet_profile";
/// `metadata.kind` of the chunks holding a sheet's cell text.
pub const KIND_SHEET_ROWS: &str = "sheet_rows";

/// SQL condition (chunk table aliased `c`): the chunk is no computed overview.
/// `json_valid` first — chunks of older imports may carry non-JSON metadata.
pub(crate) const EXTRACTABLE_SQL: &str =
    "COALESCE(CASE WHEN json_valid(c.metadata) THEN json_extract(c.metadata, '$.kind') END, '') <> 'sheet_profile'";

/// The chunk's `metadata.kind`, if its metadata is a JSON object that has one.
pub fn chunk_kind(metadata: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(metadata).ok()?;
    v.get("kind")?.as_str().map(str::to_string)
}

/// Whether the graph extraction reads a chunk with this `metadata`.
pub fn is_extractable(metadata: &str) -> bool {
    chunk_kind(metadata).as_deref() != Some(KIND_SHEET_PROFILE)
}

impl Database {
    /// Non-omitted chunks of a context that the graph extraction reads
    /// (everything but `sheet_profile` chunks), in document order.
    pub fn list_extractable_chunks(&self, context_id: i64) -> Result<Vec<Chunk>> {
        let sql = format!(
            "SELECT c.* FROM chunks c WHERE c.context_id = ?1 AND c.is_omitted = 0 AND {EXTRACTABLE_SQL} \
             ORDER BY c.document_id, c.chunk_index"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([context_id], row_to_chunk)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Count of [`Self::list_extractable_chunks`], without loading texts.
    pub fn count_extractable_chunks(&self, context_id: i64) -> Result<i64> {
        let sql = format!("SELECT COUNT(*) FROM chunks c WHERE c.context_id = ?1 AND c.is_omitted = 0 AND {EXTRACTABLE_SQL}");
        Ok(self.conn.query_row(&sql, [context_id], |row| row.get(0))?)
    }

    /// The `sheet_profile` chunk of `sheet` in `document_id` (`region` 0 = the sheet has one region).
    pub fn find_sheet_profile_chunk(&self, document_id: i64, sheet: &str, region: i64) -> Result<Option<Chunk>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM chunks WHERE document_id = ?1 AND is_omitted = 0 AND json_valid(metadata)
                   AND json_extract(metadata, '$.kind') = 'sheet_profile'
                   AND json_extract(metadata, '$.sheet') = ?2
                   AND COALESCE(json_extract(metadata, '$.region'), 0) = ?3
                 ORDER BY chunk_index LIMIT 1",
                params![document_id, sheet, region],
                row_to_chunk,
            )
            .optional()?)
    }
}

#[cfg(test)]
#[path = "chunk_kinds_tests.rs"]
mod chunk_kinds_tests;
