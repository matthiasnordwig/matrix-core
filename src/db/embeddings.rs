//! Vector persistence: raw f32 BLOBs + pure-Rust brute-force cosine.
//!
//! No `sqlite-vec` / HNSW C-extensions (those break static cross-compilation
//! for `aarch64-apple-ios`). The dot-product loops are plain iterators that
//! LLVM autovectorizes into ARM64 NEON on Apple Silicon — fast enough for
//! brute-force over thousands of small (128-dim Matryoshka) vectors.

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{CoreError, Database, Result};

// --- f32 BLOB codec --------------------------------------------------------

/// Encode a vector as little-endian f32 bytes.
pub fn vector_to_blob(v: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for x in v {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    bytes
}

/// Decode a little-endian f32 BLOB back into a vector.
pub fn blob_to_vector(bytes: &[u8]) -> Result<Vec<f32>> {
    if !bytes.len().is_multiple_of(4) {
        return Err(CoreError::InvalidBlob(bytes.len()));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

// --- vector math -----------------------------------------------------------

#[inline]
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// L2-normalize in place. No-op for a zero vector. Used by the embedder
/// before storing vectors so cosine reduces to a dot product.
pub fn l2_normalize(v: &mut [f32]) {
    let norm = dot(v, v).sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Cosine similarity. Equals the dot product when both inputs are normalized.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let na = dot(a, a).sqrt();
    let nb = dot(b, b).sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot(a, b) / (na * nb)
    }
}

fn row_to_stored_vector(row: &Row<'_>) -> rusqlite::Result<StoredVector> {
    let blob: Vec<u8> = row.get("vector")?;
    // blob_to_vector only fails on a corrupt (non-multiple-of-4) BLOB; surface
    // that through rusqlite's error channel so query_map stays ergonomic.
    let vector = blob_to_vector(&blob).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(blob.len(), rusqlite::types::Type::Blob, Box::new(
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
        ))
    })?;
    Ok(StoredVector {
        chunk_id: row.get("chunk_id")?,
        document_id: row.get("document_id")?,
        dim: row.get("dim")?,
        vector,
    })
}

impl Database {
    /// Store a chunk's embedding. Fails if `vector.len()` disagrees with `dim`.
    pub fn insert_embedding(&self, e: &NewEmbedding) -> Result<()> {
        if e.vector.len() as i64 != e.dim {
            return Err(CoreError::DimMismatch {
                expected: e.dim as usize,
                got: e.vector.len(),
            });
        }
        let blob = vector_to_blob(&e.vector);
        self.conn.execute(
            "INSERT OR REPLACE INTO embeddings
                (chunk_id, context_id, document_id, embedding_model_id, dim, vector)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![e.chunk_id, e.context_id, e.document_id, e.embedding_model_id, e.dim, blob],
        )?;
        Ok(())
    }

    /// Delete every stored vector for a context. Used when its embedding model
    /// changes: the old vectors live in a different embedding space and would
    /// otherwise be compared (meaninglessly) against new-model query vectors.
    /// Returns the number of rows removed.
    pub fn delete_embeddings_for_context(&self, context_id: i64) -> Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM embeddings WHERE context_id = ?1", [context_id])?)
    }

    /// Read back a single chunk's vector (used by tests / re-embedding checks).
    pub fn embedding_vector(&self, chunk_id: i64) -> Result<Option<Vec<f32>>> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT vector FROM embeddings WHERE chunk_id = ?1",
                [chunk_id],
                |row| row.get(0),
            )
            .optional()?;
        match blob {
            Some(b) => Ok(Some(blob_to_vector(&b)?)),
            None => Ok(None),
        }
    }

    /// Load every vector for a context (one embedding space — the unit a
    /// brute-force scan operates over).
    pub fn scan_context_vectors(&self, context_id: i64) -> Result<Vec<StoredVector>> {
        let mut stmt = self.conn.prepare(
            "SELECT chunk_id, document_id, dim, vector FROM embeddings WHERE context_id = ?1",
        )?;
        let rows = stmt.query_map([context_id], row_to_stored_vector)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Load vectors for a specific set of documents within a context (the
    /// file-level retrieval scope the left pane requests).
    pub fn scan_document_vectors(&self, document_id: i64) -> Result<Vec<StoredVector>> {
        let mut stmt = self.conn.prepare(
            "SELECT chunk_id, document_id, dim, vector FROM embeddings WHERE document_id = ?1",
        )?;
        let rows = stmt.query_map([document_id], row_to_stored_vector)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Brute-force cosine top-k over a context's vectors. All vectors in a
    /// context share one dimension, so `query` must match it.
    pub fn search_context(
        &self,
        context_id: i64,
        query: &[f32],
        top_k: usize,
    ) -> Result<Vec<ScoredChunk>> {
        let stored = self.scan_context_vectors(context_id)?;
        let mut scored = Vec::with_capacity(stored.len());
        for sv in &stored {
            if sv.vector.len() != query.len() {
                return Err(CoreError::DimMismatch {
                    expected: query.len(),
                    got: sv.vector.len(),
                });
            }
            scored.push(ScoredChunk {
                chunk_id: sv.chunk_id,
                score: cosine(query, &sv.vector),
            });
        }
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        Ok(scored)
    }
}
