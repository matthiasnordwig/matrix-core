//! Staging tables: `prechunks` (resumable LLM orchestration) and `chunks`
//! (the physical STAGING TABLE written after assembly). Chunks added to a
//! finished document later (OCR text of scanned PDF pages, generated page
//! descriptions) go in at their page's place: [`Database::insert_chunks_in_order`]
//! renumbers `chunk_index`; [`Database::page_description_marks`] lists the
//! page-description chunks (`metadata.described_page`).

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{Database, Result};

/// Result of [`Database::complete_section_chunks`]: the contiguous run of
/// same-section continuation chunks found (in `chunk_index` order), plus
/// where the section goes on beyond what was returned.
#[derive(Debug, Clone)]
pub struct SectionContinuation {
    pub chunks: Vec<Chunk>,
    /// Chunk id of the NEXT same-section chunk beyond the cap (None if the
    /// section ends within the cap).
    pub continues_at: Option<i64>,
}

/// The chunk's structural `section` (JSON field `section` in `metadata`,
/// written by the structural chunker — see `chunking/structural.rs`), if
/// present and non-empty. `metadata` is a TEXT column holding a JSON object;
/// a malformed/non-object value is treated the same as "no section" (no
/// guessing at continuation).
pub(crate) fn chunk_section(c: &Chunk) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(&c.metadata).ok()?;
    let section = value.get("section")?.as_str()?;
    if section.is_empty() {
        None
    } else {
        Some(section.to_string())
    }
}

fn row_to_prechunk(row: &Row<'_>) -> rusqlite::Result<Prechunk> {
    Ok(Prechunk {
        id: row.get("id")?,
        document_id: row.get("document_id")?,
        idx: row.get("idx")?,
        start_sentence: row.get("start_sentence")?,
        end_sentence: row.get("end_sentence")?,
        char_start: row.get("char_start")?,
        char_end: row.get("char_end")?,
        text: row.get("text")?,
        llm_status: row.get("llm_status")?,
        llm_response: row.get("llm_response")?,
        attempts: row.get("attempts")?,
        updated_at: row.get("updated_at")?,
    })
}

pub(super) fn row_to_chunk(row: &Row<'_>) -> rusqlite::Result<Chunk> {
    Ok(Chunk {
        id: row.get("id")?,
        context_id: row.get("context_id")?,
        document_id: row.get("document_id")?,
        chunk_index: row.get("chunk_index")?,
        char_start: row.get("char_start")?,
        char_end: row.get("char_end")?,
        text: row.get("text")?,
        signature: row.get("signature")?,
        is_omitted: row.get("is_omitted")?,
        metadata: row.get("metadata")?,
        created_at: row.get("created_at")?,
    })
}

impl Database {
    // --- prechunks ---------------------------------------------------------

    pub fn create_prechunk(&self, p: &NewPrechunk) -> Result<Prechunk> {
        self.conn.execute(
            "INSERT INTO prechunks
                (document_id, idx, start_sentence, end_sentence, char_start, char_end, text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                p.document_id,
                p.idx,
                p.start_sentence,
                p.end_sentence,
                p.char_start,
                p.char_end,
                p.text,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self.prechunk(id)?.expect("row just inserted must exist"))
    }

    pub fn prechunk(&self, id: i64) -> Result<Option<Prechunk>> {
        Ok(self
            .conn
            .query_row("SELECT * FROM prechunks WHERE id = ?1", [id], row_to_prechunk)
            .optional()?)
    }

    /// Record the LLM result for a pre-chunk (resumability checkpoint).
    pub fn set_prechunk_result(
        &self,
        id: i64,
        status: PrechunkStatus,
        llm_response: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE prechunks
                SET llm_status = ?2, llm_response = ?3,
                    attempts = attempts + 1, updated_at = unixepoch()
             WHERE id = ?1",
            params![id, status, llm_response],
        )?;
        Ok(())
    }

    /// All pre-chunks of a document, ordered.
    pub fn prechunks_for_document(&self, document_id: i64) -> Result<Vec<Prechunk>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM prechunks WHERE document_id = ?1 ORDER BY idx")?;
        let rows = stmt.query_map([document_id], row_to_prechunk)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// All pre-chunks across a context's documents (for inspecting raw LLM output).
    pub fn prechunks_for_context(&self, context_id: i64) -> Result<Vec<Prechunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.* FROM prechunks p
             JOIN documents d ON d.id = p.document_id
             WHERE d.context_id = ?1
             ORDER BY p.document_id, p.idx",
        )?;
        let rows = stmt.query_map([context_id], row_to_prechunk)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Pre-chunks still awaiting an LLM result — the work-list to resume after a crash.
    pub fn pending_prechunks(&self, document_id: i64) -> Result<Vec<Prechunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM prechunks
             WHERE document_id = ?1 AND llm_status IN ('pending', 'error')
             ORDER BY idx",
        )?;
        let rows = stmt.query_map([document_id], row_to_prechunk)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // --- chunks (STAGING TABLE) -------------------------------------------

    pub fn create_chunk(&self, c: &NewChunk) -> Result<Chunk> {
        self.conn.execute(
            "INSERT INTO chunks
                (context_id, document_id, chunk_index, char_start, char_end,
                 text, signature, is_omitted, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                c.context_id,
                c.document_id,
                c.chunk_index,
                c.char_start,
                c.char_end,
                c.text,
                c.signature,
                c.is_omitted,
                c.metadata,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self.chunk(id)?.expect("row just inserted must exist"))
    }

    pub fn chunk(&self, id: i64) -> Result<Option<Chunk>> {
        Ok(self
            .conn
            .query_row("SELECT * FROM chunks WHERE id = ?1", [id], row_to_chunk)
            .optional()?)
    }

    /// Batch fetch (avoids N+1 when a caller needs many chunks by id, e.g.
    /// Grid history loading). Order is unspecified — callers index by `id`.
    pub fn chunks_by_ids(&self, ids: &[i64]) -> Result<Vec<Chunk>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat("?").take(ids.len()).collect::<Vec<_>>().join(",");
        let sql = format!("SELECT * FROM chunks WHERE id IN ({placeholders})");
        let mut stmt = self.conn.prepare(&sql)?;
        let params = rusqlite::params_from_iter(ids.iter());
        let rows = stmt.query_map(params, row_to_chunk)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Staged chunks for a context, in chronological order. `include_omitted`
    /// controls whether `leave_out` chunks are returned (default UI hides them).
    pub fn list_chunks(&self, context_id: i64, include_omitted: bool) -> Result<Vec<Chunk>> {
        let sql = if include_omitted {
            "SELECT * FROM chunks WHERE context_id = ?1 ORDER BY document_id, chunk_index"
        } else {
            "SELECT * FROM chunks WHERE context_id = ?1 AND is_omitted = 0 \
             ORDER BY document_id, chunk_index"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([context_id], row_to_chunk)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Non-omitted chunks in a context that still need a vector for `model_id` —
    /// the work-list for the embedding stage. A chunk qualifies if it has no
    /// embedding yet OR its stored embedding was produced by a *different* model
    /// (stale after switching the context's embedding model). Re-embedding
    /// overwrites via `INSERT OR REPLACE`, so `embed` is idempotent and
    /// self-correcting across model changes.
    pub fn chunks_needing_embedding(&self, context_id: i64, model_id: i64) -> Result<Vec<Chunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.* FROM chunks c
             LEFT JOIN embeddings e ON e.chunk_id = c.id
             WHERE c.context_id = ?1 AND c.is_omitted = 0
               AND (e.chunk_id IS NULL OR e.embedding_model_id != ?2)
             ORDER BY c.document_id, c.chunk_index",
        )?;
        let rows = stmt.query_map([context_id, model_id], row_to_chunk)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// All embeddable (non-omitted) chunks of a context, regardless of whether
    /// they already have a vector — used by the "re-embed all" path. Safe to
    /// re-embed in place: `insert_embedding` is `INSERT OR REPLACE`, so each
    /// chunk's vector is overwritten, never duplicated or transiently missing.
    /// Mirrors `chunks_needing_embedding`'s ordering.
    pub fn chunks_embeddable(&self, context_id: i64) -> Result<Vec<Chunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.* FROM chunks c
             WHERE c.context_id = ?1 AND c.is_omitted = 0
             ORDER BY c.document_id, c.chunk_index",
        )?;
        let rows = stmt.query_map([context_id], row_to_chunk)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn count_chunks(&self, context_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM chunks WHERE context_id = ?1",
            [context_id],
            |row| row.get(0),
        )?)
    }

    /// Non-omitted chunks of a context (what `list_chunks(.., false).len()`
    /// returns), counted in SQL without loading chunk texts.
    pub fn count_active_chunks(&self, context_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM chunks WHERE context_id = ?1 AND is_omitted = 0",
            [context_id],
            |row| row.get(0),
        )?)
    }

    /// Chunk count per document of a context (omitted chunks included, like
    /// `count_chunks`); documents without chunks are absent from the map.
    pub fn count_chunks_per_document(
        &self,
        context_id: i64,
    ) -> Result<std::collections::HashMap<i64, i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT document_id, COUNT(*) FROM chunks WHERE context_id = ?1 GROUP BY document_id",
        )?;
        let rows = stmt.query_map([context_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()?)
    }

    pub fn delete_chunk(&self, id: i64) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM chunks WHERE id = ?1", [id])? > 0)
    }

    /// Replace a chunk's text (the FTS5 sync trigger reindexes it).
    pub fn update_chunk_text(&self, id: i64, text: &str) -> Result<bool> {
        Ok(self
            .conn
            .execute("UPDATE chunks SET text = ?2 WHERE id = ?1", rusqlite::params![id, text])?
            > 0)
    }

    /// Remove all chunks of a document (used before re-staging on a re-run).
    pub fn delete_chunks_for_document(&self, document_id: i64) -> Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM chunks WHERE document_id = ?1", [document_id])?)
    }

    /// Follow a chunk's structural `section` (the chunker-written
    /// `metadata.section` JSON field, e.g. `"Artikel 395 (1)"`) forward through
    /// contiguous same-document, same-section chunks — the fix for
    /// fragmented §§/Artikel that the structural chunker splits across
    /// multiple staging chunks.
    ///
    /// Starting from `chunk_id`, walks `chunk_index + 1, + 2, …` of the same
    /// document as long as each next chunk exists, is not omitted, and its
    /// `metadata.section` is *exactly* (string-equal) the start chunk's
    /// non-empty section — a gap (missing index, omitted, or a different/empty
    /// section) ends the run. `max_extra` caps how many continuation chunks are
    /// returned (`0` is legal: no chunks, but `continues_at` is still reported).
    /// If the section still matches beyond the cap, that chunk's id is returned
    /// as `continues_at` so a caller can cheaply learn "this section continues"
    /// without paying for the extra chunks.
    ///
    /// A missing/empty starting section returns an empty result outright — no
    /// guessing at continuation for unstructured content.
    pub fn complete_section_chunks(
        &self,
        chunk_id: i64,
        max_extra: usize,
    ) -> Result<SectionContinuation> {
        let empty = SectionContinuation { chunks: Vec::new(), continues_at: None };
        let Some(start) = self.chunk(chunk_id)? else {
            return Ok(empty);
        };
        let Some(section) = chunk_section(&start) else {
            return Ok(empty);
        };

        let mut chunks = Vec::new();
        let mut next_index = start.chunk_index + 1;
        loop {
            let Some(candidate) = self.chunk_by_document_index(start.document_id, next_index)? else {
                break;
            };
            if candidate.is_omitted {
                break;
            }
            match chunk_section(&candidate) {
                Some(s) if s == section => {}
                _ => break,
            }
            if chunks.len() >= max_extra {
                return Ok(SectionContinuation { chunks, continues_at: Some(candidate.id) });
            }
            chunks.push(candidate);
            next_index += 1;
        }
        Ok(SectionContinuation { chunks, continues_at: None })
    }

    /// Fetch a chunk by (document_id, chunk_index) — the index is unique per
    /// document (`staging_chunk_index_is_unique`), so this is a point lookup.
    fn chunk_by_document_index(&self, document_id: i64, chunk_index: i64) -> Result<Option<Chunk>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM chunks WHERE document_id = ?1 AND chunk_index = ?2",
                params![document_id, chunk_index],
                row_to_chunk,
            )
            .optional()?)
    }

    pub fn list_chunks_by_page(&self, context_id: Option<i64>, page_number: i64) -> Result<Vec<Chunk>> {
        let sql = match context_id {
            Some(_) => "SELECT * FROM chunks WHERE context_id = ?1 AND is_omitted = 0 AND (CAST(json_extract(metadata, '$.page') AS INTEGER) = ?2 OR CAST(json_extract(metadata, '$.Page') AS INTEGER) = ?2 OR CAST(json_extract(metadata, '$.seite') AS INTEGER) = ?2 OR CAST(json_extract(metadata, '$.Seite') AS INTEGER) = ?2) ORDER BY document_id, chunk_index",
            None => "SELECT * FROM chunks WHERE is_omitted = 0 AND (CAST(json_extract(metadata, '$.page') AS INTEGER) = ?1 OR CAST(json_extract(metadata, '$.Page') AS INTEGER) = ?1 OR CAST(json_extract(metadata, '$.seite') AS INTEGER) = ?1 OR CAST(json_extract(metadata, '$.Seite') AS INTEGER) = ?1) ORDER BY document_id, chunk_index",
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = match context_id {
            Some(cid) => stmt.query_map(rusqlite::params![cid, page_number], row_to_chunk)?,
            None => stmt.query_map(rusqlite::params![page_number], row_to_chunk)?,
        };
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

/// One place in a document's new chunk order, see [`Database::insert_chunks_in_order`].
#[derive(Debug, Clone, Copy)]
pub enum ChunkSlot<'a> {
    /// An existing chunk of the document, by id.
    Existing(i64),
    /// A chunk to insert here.
    New(&'a NewChunk),
}

/// A chunk that stands for a generated description of one PDF page (or the
/// note that the model declined it), see [`Database::page_description_marks`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageDescriptionMark {
    pub chunk_id: i64,
    pub document_id: i64,
    /// 1-based page number (`metadata.described_page`).
    pub page: i64,
    /// `metadata.page_description` of a "declined" marker chunk; `None` for a description.
    pub declined: Option<String>,
}

impl Database {
    /// Insert chunks between a document's existing ones in one transaction:
    /// `order` lists EVERY existing chunk of the document (by id) and the new
    /// ones in the wanted order; `chunk_index` is renumbered 0.. along it (the
    /// existing chunks keep their ids, vectors and graph results). Returns the
    /// inserted chunks. An `order` that misses an existing chunk or names a
    /// foreign one is `InvalidState` and changes nothing.
    pub fn insert_chunks_in_order(&self, document_id: i64, order: &[ChunkSlot]) -> Result<Vec<Chunk>> {
        let existing: Vec<i64> = {
            let mut stmt = self.conn.prepare("SELECT id FROM chunks WHERE document_id = ?1")?;
            let rows = stmt.query_map([document_id], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<i64>>>()?
        };
        let mut named: Vec<i64> =
            order.iter().filter_map(|s| if let ChunkSlot::Existing(id) = s { Some(*id) } else { None }).collect();
        named.sort_unstable();
        let mut have = existing.clone();
        have.sort_unstable();
        if named != have {
            return Err(super::CoreError::InvalidState(format!(
                "chunk order of document {document_id} must name each of its {} chunks exactly once",
                existing.len()
            )));
        }
        let tx = self.conn.unchecked_transaction()?;
        // Out of the way first: (context, document, chunk_index) is unique.
        tx.execute("UPDATE chunks SET chunk_index = -1 - chunk_index WHERE document_id = ?1", [document_id])?;
        let mut inserted = Vec::new();
        for (i, slot) in order.iter().enumerate() {
            match slot {
                ChunkSlot::Existing(id) => {
                    tx.execute("UPDATE chunks SET chunk_index = ?2 WHERE id = ?1", params![id, i as i64])?;
                }
                ChunkSlot::New(c) => {
                    let c = NewChunk { document_id, chunk_index: i as i64, ..(*c).clone() };
                    inserted.push(self.create_chunk(&c)?);
                }
            }
        }
        tx.commit()?;
        Ok(inserted)
    }

    /// Every chunk of a document in `chunk_index` order with its position
    /// (byte range, `metadata.page`), without text — where later chunks go in.
    pub fn chunk_spans(&self, document_id: i64) -> Result<Vec<crate::embedding::page_select::ChunkSpan>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, char_start, char_end,
                    CASE WHEN json_valid(metadata) THEN CAST(json_extract(metadata, '$.page') AS INTEGER) END
               FROM chunks WHERE document_id = ?1 ORDER BY chunk_index",
        )?;
        let rows = stmt.query_map([document_id], |r| {
            let page: Option<i64> = r.get(3)?;
            Ok(crate::embedding::page_select::ChunkSpan {
                chunk_id: r.get(0)?,
                char_start: r.get(1)?,
                char_end: r.get(2)?,
                page: page.filter(|p| *p > 0),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The page-description chunks of a context (generated descriptions of PDF
    /// graphics pages and "declined" markers): every chunk whose metadata has
    /// `described_page`, by document and page.
    pub fn page_description_marks(&self, context_id: i64) -> Result<Vec<PageDescriptionMark>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, document_id, CAST(json_extract(metadata, '$.described_page') AS INTEGER),
                    json_extract(metadata, '$.page_description')
               FROM chunks
              WHERE context_id = ?1 AND json_valid(metadata)
                AND json_extract(metadata, '$.described_page') IS NOT NULL
              ORDER BY document_id, 3, id",
        )?;
        let rows = stmt.query_map([context_id], |r| {
            Ok(PageDescriptionMark { chunk_id: r.get(0)?, document_id: r.get(1)?, page: r.get(2)?, declined: r.get(3)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
#[path = "chunks_tests.rs"]
pub(crate) mod chunks_tests;
