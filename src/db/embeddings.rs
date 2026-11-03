//! Vector persistence: raw f32 BLOBs + pure-Rust brute-force cosine.
//!
//! No `sqlite-vec` / HNSW C-extensions (those break static cross-compilation
//! for `aarch64-apple-ios`). `dot` is plain Rust that the compiler vectorizes
//! for each target (NEON on ARM, SSE on x86).

use rusqlite::{params, OptionalExtension};

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

/// Dot product over the common length. Eight running sums, because the compiler must keep the
/// order of a single float sum and therefore leaves `iter().sum()` scalar.
#[inline]
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    const LANES: usize = 8;
    let n = a.len().min(b.len());
    let (a, b) = (a[..n].chunks_exact(LANES), b[..n].chunks_exact(LANES));
    let tail: f32 = a.remainder().iter().zip(b.remainder()).map(|(x, y)| x * y).sum();
    let mut sums = [0.0f32; LANES];
    for (x, y) in a.zip(b) {
        for i in 0..LANES {
            sums[i] += x[i] * y[i];
        }
    }
    sums.iter().sum::<f32>() + tail
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

impl Database {
    /// Store a chunk's embedding. Fails if `vector.len()` disagrees with `dim`.
    /// `INSERT OR REPLACE` (one row per chunk): re-embedding overwrites in place,
    /// which is what makes both the force-re-embed path and the model-switch
    /// self-correction idempotent.
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
        self.delete_paragraph_vectors_for_context(context_id)?;
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

    /// Count stored vectors for a context without loading them — used for
    /// export size estimates and to decide whether a block of them is held (`vector_hold.rs`).
    pub fn count_embeddings_for_context(&self, context_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM embeddings WHERE context_id = ?1",
            [context_id],
            |row| row.get(0),
        )?)
    }

    /// Brute-force cosine top-k over a context's vectors. All vectors in a
    /// context share one dimension, so `query` must match it.
    ///
    /// `doc_ids = Some(&[…])` restricts the scan to vectors of those documents
    /// (the file-level retrieval scope, AP8); `None` scans the whole context.
    /// An empty slice means "no documents in scope" → empty result (never
    /// interpreted as "all").
    pub fn search_context(
        &self,
        context_id: i64,
        query: &[f32],
        top_k: usize,
        doc_ids: Option<&[i64]>,
    ) -> Result<Vec<ScoredChunk>> {
        if matches!(doc_ids, Some(d) if d.is_empty()) {
            return Ok(Vec::new());
        }
        let table = self.vector_table(context_id)?;
        let gone = self.missing_document_ids(context_id)?; // file gone from disk (`source_files.rs`)
        let query_norm = dot(query, query).sqrt();
        let allowed = super::doc_set(doc_ids);
        let mut scored = Vec::with_capacity(table.len());
        for i in table.visible(&gone, allowed.as_ref()) {
            let (chunk_id, vector) = table.row(i);
            if vector.len() != query.len() {
                return Err(CoreError::DimMismatch { expected: query.len(), got: vector.len() });
            }
            scored.push(ScoredChunk::new(chunk_id, table.cosine(i, query, query_norm)));
        }
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        Ok(scored)
    }
}
