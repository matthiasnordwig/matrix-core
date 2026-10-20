//! CRUD for `page_images` and `page_image_embeddings` (schema_v61): the JPEG
//! renders of a document's graphics pages and their image vectors.
//!
//! One row per `(document_id, page)` — inserting a page again replaces the old
//! row, and its vector goes with it (FK cascade; a new image needs a new
//! vector). Image vectors live in the embedding space of the context's model:
//! every scan here filters on `embedding_model_id`, so vectors of another
//! model (left over from a model switch) are never handed to retrieval.
//! [`Database::chunk_spans_with_vectors`] serves the page → chunk mapping of
//! hybrid retrieval (`embedding/retrieval.rs`): chunk positions plus their
//! vector, without loading chunk text.
//! [`Database::replace_image_document_text`] swaps an image document's text and
//! chunks in one transaction when a generated description arrives (the page-1
//! picture and its vector stay); `mark_image_description_declined` remembers a
//! picture the model gave no description for (chunk metadata, not text).
//! A PDF graphics page keeps its describe state on its own row
//! (`set_page_description_state`, schema_v64), so re-chunking cannot reset it.

use rusqlite::{params, OptionalExtension, Row};

use super::embeddings::{blob_to_vector, vector_to_blob};
use super::models::*;
use super::{CoreError, Database, Result};
use crate::embedding::page_select::ChunkSpan;

const PAGE_IMAGE_COLS: &str = "id, context_id, document_id, page, char_start, char_end, width, height, created_at, \
     description_state, description_model";

fn row_to_page_image(row: &Row<'_>) -> rusqlite::Result<PageImage> {
    Ok(PageImage {
        id: row.get("id")?,
        context_id: row.get("context_id")?,
        document_id: row.get("document_id")?,
        page: row.get("page")?,
        char_start: row.get("char_start")?,
        char_end: row.get("char_end")?,
        width: row.get("width")?,
        height: row.get("height")?,
        created_at: row.get("created_at")?,
        description_state: row.get("description_state")?,
        description_model: row.get("description_model")?,
    })
}

fn blob_err(len: usize, e: CoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        len,
        rusqlite::types::Type::Blob,
        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())),
    )
}

/// `(position, is_omitted, vector)` of one chunk, see
/// [`Database::chunk_spans_with_vectors`].
pub(crate) type ChunkSpanRow = (ChunkSpan, bool, Option<Vec<f32>>);

impl Database {
    /// Store a graphics-page image; replaces an existing row for the same
    /// `(document_id, page)` (its vector is dropped with it).
    pub fn insert_page_image(&self, p: &NewPageImage) -> Result<PageImage> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM page_images WHERE document_id = ?1 AND page = ?2",
            params![p.document_id, p.page],
        )?;
        tx.execute(
            "INSERT INTO page_images
                (context_id, document_id, page, char_start, char_end, width, height, image)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![p.context_id, p.document_id, p.page, p.char_start, p.char_end, p.width, p.height, p.image],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        self.page_image(id)?
            .ok_or_else(|| CoreError::NotFound(format!("page_image {id}")))
    }

    /// One page image (without its bytes).
    pub fn page_image(&self, id: i64) -> Result<Option<PageImage>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {PAGE_IMAGE_COLS} FROM page_images WHERE id = ?1"),
                [id],
                row_to_page_image,
            )
            .optional()?)
    }

    /// A document's page images in page order (without bytes).
    pub fn list_page_images_for_document(&self, document_id: i64) -> Result<Vec<PageImage>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {PAGE_IMAGE_COLS} FROM page_images WHERE document_id = ?1 ORDER BY page"
        ))?;
        let rows = stmt.query_map([document_id], row_to_page_image)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// A context's page images, by document then page (without bytes).
    pub fn list_page_images_for_context(&self, context_id: i64) -> Result<Vec<PageImage>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {PAGE_IMAGE_COLS} FROM page_images WHERE context_id = ?1 ORDER BY document_id, page"
        ))?;
        let rows = stmt.query_map([context_id], row_to_page_image)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The JPEG bytes of one page image.
    pub fn page_image_bytes(&self, id: i64) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn
            .query_row("SELECT image FROM page_images WHERE id = ?1", [id], |r| r.get(0))
            .optional()?)
    }

    /// Store (or replace) the vector of a page image. The context comes from
    /// the page image row; an unknown id is `NotFound`.
    pub fn upsert_page_image_embedding(
        &self,
        page_image_id: i64,
        embedding_model_id: i64,
        vector: &[f32],
    ) -> Result<()> {
        let n = self.conn.execute(
            "INSERT OR REPLACE INTO page_image_embeddings
                (page_image_id, context_id, embedding_model_id, dim, vector)
             SELECT id, context_id, ?2, ?3, ?4 FROM page_images WHERE id = ?1",
            params![page_image_id, embedding_model_id, vector.len() as i64, vector_to_blob(vector)],
        )?;
        if n == 0 {
            return Err(CoreError::NotFound(format!("page_image {page_image_id}")));
        }
        Ok(())
    }

    /// Replace an image document's text and chunks in one savepoint (new
    /// generated description): `documents.extracted_text`/`byte_size` get
    /// `text`, the old chunks are replaced by `chunks` with the graph's
    /// evidence carried over by text (`replace_document_chunks`; vectors go
    /// with the old chunks), and the page-1 picture keeps its row and image
    /// vector — only its text range becomes the whole new text. Returns the
    /// number of chunks inserted and the carry-over counts; an unknown
    /// document is `NotFound`.
    pub fn replace_image_document_text(
        &self,
        document_id: i64,
        text: &str,
        chunks: &[NewChunk],
    ) -> Result<(usize, crate::db::ontology::evidence_carry::EvidenceCarryReport)> {
        self.replace_document_chunks(document_id, || {
            let n = self.conn.execute(
                "UPDATE documents SET extracted_text = ?2, byte_size = ?3 WHERE id = ?1",
                params![document_id, text, text.len() as i64],
            )?;
            if n == 0 {
                return Err(CoreError::NotFound(format!("document {document_id}")));
            }
            for c in chunks {
                self.create_chunk(c)?;
            }
            self.conn.execute(
                "UPDATE page_images SET char_start = 0, char_end = ?2 WHERE document_id = ?1 AND page = 1",
                params![document_id, text.len() as i64],
            )?;
            Ok(chunks.len())
        })
    }

    /// Remember on an image document's chunks that its image model gave no
    /// description (`metadata.image_description = note`), so a later run does
    /// not send — and bill — the picture again. Hidden from the chunk text; goes
    /// away with the chunks when a description is stored. Returns chunks marked.
    pub fn mark_image_description_declined(&self, document_id: i64, note: &str) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE chunks SET metadata = CASE WHEN json_valid(metadata)
                 THEN json_set(metadata, '$.image_description', ?2)
                 ELSE json_object('image_description', ?2) END
             WHERE document_id = ?1",
            params![document_id, note],
        )?)
    }

    /// Documents of a context marked by [`Self::mark_image_description_declined`].
    pub fn image_description_declined_documents(&self, context_id: i64) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT document_id FROM chunks WHERE context_id = ?1 AND json_valid(metadata)
                 AND json_extract(metadata, '$.image_description') IS NOT NULL ORDER BY document_id",
        )?;
        let rows = stmt.query_map([context_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<i64>>>()?)
    }

    /// Record the describe job's outcome for a page image (`described` /
    /// `declined`, see schema_v64) and the model that answered.
    pub fn set_page_description_state(&self, page_image_id: i64, state: &str, model: &str) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE page_images SET description_state = ?2, description_model = ?3 WHERE id = ?1",
            params![page_image_id, state, model],
        )?;
        if n == 0 {
            return Err(CoreError::NotFound(format!("page_image {page_image_id}")));
        }
        Ok(())
    }

    /// Delete every page-image vector of a context (model switch / re-embed).
    pub fn delete_page_image_embeddings_for_context(&self, context_id: i64) -> Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM page_image_embeddings WHERE context_id = ?1", [context_id])?)
    }

    /// Page images of a context without a vector of the context's CURRENT
    /// model (a vector of another model counts as missing). A context without
    /// a model returns all its page images.
    pub fn page_images_needing_embedding(&self, context_id: i64) -> Result<Vec<PageImage>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.context_id, p.document_id, p.page, p.char_start, p.char_end,
                    p.width, p.height, p.created_at, p.description_state, p.description_model
               FROM page_images p
               JOIN contexts c ON c.id = p.context_id
               LEFT JOIN page_image_embeddings e
                      ON e.page_image_id = p.id AND e.embedding_model_id = c.embedding_model_id
              WHERE p.context_id = ?1 AND e.page_image_id IS NULL
              ORDER BY p.document_id, p.page",
        )?;
        let rows = stmt.query_map([context_id], row_to_page_image)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// `(total page images, page images with a vector of embedding_model_id)`
    /// for a context.
    pub fn page_image_counts(&self, context_id: i64, embedding_model_id: i64) -> Result<(i64, i64)> {
        Ok(self.conn.query_row(
            "SELECT (SELECT COUNT(*) FROM page_images WHERE context_id = ?1),
                    (SELECT COUNT(*) FROM page_image_embeddings
                      WHERE context_id = ?1 AND embedding_model_id = ?2)",
            params![context_id, embedding_model_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    /// Every page-image vector of a context made by `embedding_model_id`, with
    /// the page's document, number and text range — the image rank list's scan.
    pub fn scan_page_image_vectors(
        &self,
        context_id: i64,
        embedding_model_id: i64,
    ) -> Result<Vec<StoredPageImageVector>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.document_id, p.page, p.char_start, p.char_end, e.vector
               FROM page_image_embeddings e
               JOIN page_images p ON p.id = e.page_image_id
              WHERE e.context_id = ?1 AND e.embedding_model_id = ?2",
        )?;
        let rows = stmt.query_map(params![context_id, embedding_model_id], |r| {
            let blob: Vec<u8> = r.get(5)?;
            let vector = blob_to_vector(&blob).map_err(|e| blob_err(blob.len(), e))?;
            Ok(StoredPageImageVector {
                page_image_id: r.get(0)?,
                document_id: r.get(1)?,
                page: r.get(2)?,
                char_start: r.get(3)?,
                char_end: r.get(4)?,
                vector,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// All chunks of a document in `chunk_index` order with their position
    /// (byte range, `metadata.page`) and — only if made by `embedding_model_id`
    /// with length `dim` — their vector. No chunk text is loaded.
    pub(crate) fn chunk_spans_with_vectors(
        &self,
        document_id: i64,
        embedding_model_id: i64,
        dim: usize,
    ) -> Result<Vec<ChunkSpanRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.char_start, c.char_end, c.is_omitted,
                    CASE WHEN json_valid(c.metadata)
                         THEN CAST(json_extract(c.metadata, '$.page') AS INTEGER) END,
                    e.vector
               FROM chunks c
               LEFT JOIN embeddings e
                      ON e.chunk_id = c.id AND e.embedding_model_id = ?2 AND e.dim = ?3
              WHERE c.document_id = ?1
              ORDER BY c.chunk_index",
        )?;
        let rows = stmt.query_map(params![document_id, embedding_model_id, dim as i64], |r| {
            let blob: Option<Vec<u8>> = r.get(5)?;
            let vector = match blob {
                Some(b) => Some(blob_to_vector(&b).map_err(|e| blob_err(b.len(), e))?),
                None => None,
            };
            let page: Option<i64> = r.get(4)?;
            let span = ChunkSpan {
                chunk_id: r.get(0)?,
                char_start: r.get(1)?,
                char_end: r.get(2)?,
                page: page.filter(|p| *p > 0),
            };
            Ok((span, r.get(3)?, vector))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
#[path = "page_images_tests.rs"]
mod page_images_tests;
