//! A collection's vectors as one block of memory instead of one allocation per passage, read from
//! SQLite once and kept for the next search where that pays.
//!
//! Search stays exact: every passage is compared. Held is one collection, the one searched last, and
//! only from `MIN_HELD_VECTORS` on; below that reading is cheap and the memory is better free.
//!
//! The held block is valid while nothing wrote to `embeddings`: the update hook counts writes of this
//! connection, cascades included, and `PRAGMA data_version` sees commits of other connections and
//! processes on the same file.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

use super::embeddings::dot;
use super::{CoreError, Database, Result};

const MIN_HELD_VECTORS: i64 = if cfg!(test) { 3 } else { 50_000 };

type Stamp = (i64, u64);

/// All vectors of one collection, in table order. Lengths may differ (a corrupt or half-migrated
/// collection); `search_context` reports that as before, per row, after its filters.
pub(crate) struct VectorTable {
    context_id: i64,
    stamp: Stamp,
    chunks: Vec<i64>,
    documents: Vec<i64>,
    starts: Vec<usize>,
    values: Vec<f32>,
    norms: Vec<f32>,
}

impl VectorTable {
    pub(crate) fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Rows without the documents in `gone` and, if given, only those of `allowed`, in table order.
    /// `allowed` is a set: a filtered search can allow hundreds of thousands of documents.
    pub(crate) fn visible<'a>(&'a self, gone: &'a HashSet<i64>, allowed: Option<&'a HashSet<i64>>) -> impl Iterator<Item = usize> + 'a {
        (0..self.len()).filter(move |&i| {
            let document = self.documents[i];
            !gone.contains(&document) && allowed.is_none_or(|a| a.contains(&document))
        })
    }

    /// Row `i` as (chunk, vector).
    pub(crate) fn row(&self, i: usize) -> (i64, &[f32]) {
        (self.chunks[i], &self.values[self.starts[i]..self.starts[i + 1]])
    }

    /// Cosine of row `i` with `query`, whose norm is `query_norm`. Same value as `cosine`, without
    /// computing the stored vector's norm again.
    pub(crate) fn cosine(&self, i: usize, query: &[f32], query_norm: f32) -> f32 {
        let norm = self.norms[i];
        if query_norm == 0.0 || norm == 0.0 {
            0.0
        } else {
            dot(query, self.row(i).1) / (query_norm * norm)
        }
    }
}

/// `Database`'s part: the write counter and the held block.
pub(super) struct VectorHold {
    writes: Arc<AtomicU64>,
    pub(super) held: RefCell<Option<Arc<VectorTable>>>,
}

impl VectorHold {
    pub(super) fn watching(conn: &Connection) -> Self {
        let writes = Arc::new(AtomicU64::new(0));
        let counter = writes.clone();
        conn.update_hook(Some(move |_action, _db: &str, table: &str, _rowid: i64| {
            if table == "embeddings" {
                counter.fetch_add(1, Ordering::Relaxed);
            }
        }));
        Self { writes, held: RefCell::new(None) }
    }
}

impl Database {
    fn vector_stamp(&self) -> Result<Stamp> {
        let foreign: i64 = self.conn.pragma_query_value(None, "data_version", |r| r.get(0))?;
        Ok((foreign, self.vectors.writes.load(Ordering::Relaxed)))
    }

    /// The vectors of `context_id`: the held block if it is still valid, otherwise read now.
    pub(crate) fn vector_table(&self, context_id: i64) -> Result<Arc<VectorTable>> {
        // Before reading: a commit in between makes the block look older than it is, never newer.
        let stamp = self.vector_stamp()?;
        let held = self.vectors.held.borrow().clone();
        if let Some(t) = held.filter(|t| t.context_id == context_id && t.stamp == stamp) {
            return Ok(t);
        }
        let hold = self.count_embeddings_for_context(context_id)? >= MIN_HELD_VECTORS;
        if hold {
            self.vectors.held.borrow_mut().take(); // free first, or both blocks exist at once
        }
        let table = Arc::new(self.read_vector_table(context_id, stamp)?);
        // Inside a transaction the block could show rows that are later rolled back.
        if hold && self.conn.is_autocommit() {
            *self.vectors.held.borrow_mut() = Some(table.clone());
        }
        Ok(table)
    }

    fn read_vector_table(&self, context_id: i64, stamp: Stamp) -> Result<VectorTable> {
        let mut table = VectorTable {
            context_id,
            stamp,
            chunks: Vec::new(),
            documents: Vec::new(),
            starts: vec![0],
            values: Vec::new(),
            norms: Vec::new(),
        };
        let mut stmt = self.conn.prepare("SELECT chunk_id, document_id, vector FROM embeddings WHERE context_id = ?1")?;
        let mut rows = stmt.query([context_id])?;
        while let Some(row) = rows.next()? {
            let bytes = row.get_ref(2)?.as_blob().map_err(rusqlite::Error::from)?;
            if !bytes.len().is_multiple_of(4) {
                return Err(CoreError::InvalidBlob(bytes.len()));
            }
            let start = table.values.len();
            table.values.extend(bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])));
            let vector = &table.values[start..];
            table.norms.push(dot(vector, vector).sqrt());
            table.chunks.push(row.get(0)?);
            table.documents.push(row.get(1)?);
            table.starts.push(table.values.len());
        }
        Ok(table)
    }
}
