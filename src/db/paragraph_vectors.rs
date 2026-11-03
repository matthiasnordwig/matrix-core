//! Paragraph vectors of mail passages (`embedding_paragraphs`, schema_v77), next to the passage's own
//! vector in `embeddings`. Written by the embedding job for the mail passages it embeds (services
//! `commands/paragraph_vectors.rs`, paragraphs cut by `chunking::paragraphs::paragraph_units`); read
//! only to choose the reranker's excerpt (services `commands/rerank_excerpt.rs`), never by the vector
//! search. Removed with their chunk (cascade) and with the collection's vectors (model change).

use rusqlite::params;

use super::embeddings::{blob_to_vector, vector_to_blob};
use super::models::{Chunk, ParagraphVector};
use super::{Database, Result};

impl Database {
    /// Replace the paragraph vectors of `chunk` (none = remove them). `parts` = (start, end, vector).
    pub fn set_paragraph_vectors(&self, chunk: &Chunk, model_id: i64, parts: &[(usize, usize, Vec<f32>)]) -> Result<()> {
        self.conn.execute("DELETE FROM embedding_paragraphs WHERE chunk_id = ?1", [chunk.id])?;
        let mut stmt = self.conn.prepare(
            "INSERT INTO embedding_paragraphs (chunk_id, part, context_id, document_id, embedding_model_id, start_byte, end_byte, vector) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for (i, (start, end, v)) in parts.iter().enumerate() {
            stmt.execute(params![chunk.id, i as i64, chunk.context_id, chunk.document_id, model_id, *start as i64, *end as i64, vector_to_blob(v)])?;
        }
        Ok(())
    }

    /// The paragraph vectors of the chunks `ids`, in paragraph order per chunk.
    pub fn paragraph_vectors(&self, ids: &[i64]) -> Result<Vec<ParagraphVector>> {
        let mut out = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT chunk_id, embedding_model_id, start_byte, end_byte, vector FROM embedding_paragraphs WHERE chunk_id = ?1 ORDER BY part",
        )?;
        for id in ids {
            let mut rows = stmt.query([id])?;
            while let Some(r) = rows.next()? {
                let bytes: Vec<u8> = r.get(4)?;
                out.push(ParagraphVector {
                    chunk_id: r.get(0)?,
                    embedding_model_id: r.get(1)?,
                    start: r.get::<_, i64>(2)? as usize,
                    end: r.get::<_, i64>(3)? as usize,
                    vector: blob_to_vector(&bytes)?,
                });
            }
        }
        Ok(out)
    }

    /// Remove the paragraph vectors of `context_id` (re-embedding, model change). Returns the rows removed.
    pub fn delete_paragraph_vectors_for_context(&self, context_id: i64) -> Result<usize> {
        Ok(self.conn.execute("DELETE FROM embedding_paragraphs WHERE context_id = ?1", [context_id])?)
    }
}
