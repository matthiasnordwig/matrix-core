//! CRUD for `source_roots` and `source_files` (schema_v71): the files and folders
//! the stand-alone MCP program read into a collection, and each file's size,
//! modification time and hash when read. The comparison with the disk and the
//! update itself live in services (`commands/pipeline/resync*.rs`); here only rows.
//!
//! Nothing here deletes a document: a gone file only gets `missing_since`
//! ([`Database::mark_sources_missing`]), and every search leaves such documents out
//! ([`FILE_PRESENT_SQL`], [`Database::missing_document_ids`]); reading a chunk by id
//! still works. Documents of an update that did not
//! finish carry `pending` and are invisible to [`Database::source_files`]; the
//! update lists them with [`Database::pending_source_documents`] to remove them; a
//! document without any row is never a leftover (an older program writes none).
//! Every function is scoped to one context.
//!
//! **Lease** (`source_leases`): `add_files` and the update of a collection take
//! [`Database::try_source_lease`] for their context and renew it while they work;
//! another program on the same data folder (a second Claude Code window) then
//! leaves that collection alone until the lease is released or has expired.

use rusqlite::params;

use super::models::{SourceFile, SourceRoot};
use super::{Database, Result};

fn row_to_source_file(r: &rusqlite::Row<'_>) -> rusqlite::Result<SourceFile> {
    Ok(SourceFile {
        document_id: r.get(0)?,
        context_id: r.get(1)?,
        path: r.get(2)?,
        byte_size: r.get(3)?,
        mtime_ns: r.get(4)?,
        sha256: r.get(5)?,
        missing_since: r.get(6)?,
    })
}

/// SQL condition on a chunk row `c`: its document's file is not missing. The search
/// paths (`fts.rs`, `chunk_refs.rs::chunks_with_ref`) add it, so a gone file is left
/// out by exclusion — no list of the present documents, no bound variables.
pub(crate) const FILE_PRESENT_SQL: &str = "NOT EXISTS (SELECT 1 FROM source_files s \
     WHERE s.document_id = c.document_id AND s.pending = 0 AND s.missing_since IS NOT NULL)";

const FILE_COLS: &str = "document_id, context_id, path, byte_size, mtime_ns, sha256, missing_since";

impl Database {
    /// Remember a file or folder `add_files` read for `context_id`. Again for the
    /// same path: `recursive` only widens (a recursive add stays recursive).
    pub fn record_source_root(&self, context_id: i64, path: &str, is_dir: bool, recursive: bool) -> Result<()> {
        self.conn.execute(
            "INSERT INTO source_roots (context_id, path, is_dir, recursive) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (context_id, path) DO UPDATE SET
                 recursive = MAX(recursive, excluded.recursive), is_dir = excluded.is_dir",
            params![context_id, path, is_dir, recursive],
        )?;
        Ok(())
    }

    /// The files and folders of `context_id`, by path.
    pub fn source_roots(&self, context_id: i64) -> Result<Vec<SourceRoot>> {
        let mut stmt = self.conn.prepare(
            "SELECT context_id, path, is_dir, recursive, checked_at FROM source_roots
             WHERE context_id = ?1 ORDER BY path",
        )?;
        let rows = stmt.query_map([context_id], |r| {
            Ok(SourceRoot {
                context_id: r.get(0)?,
                path: r.get(1)?,
                is_dir: r.get(2)?,
                recursive: r.get(3)?,
                checked_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Contexts with at least one recorded file or folder, by id.
    pub fn contexts_with_source_roots(&self) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT context_id FROM source_roots ORDER BY context_id")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Note the time of a comparison with the disk on every root of `context_id`.
    pub fn set_source_roots_checked(&self, context_id: i64, at: i64) -> Result<()> {
        self.conn.execute("UPDATE source_roots SET checked_at = ?2 WHERE context_id = ?1", params![context_id, at])?;
        Ok(())
    }

    /// Remember the file `document_id` was read from (`missing_since` is ignored:
    /// a file just read is present). `pending`: see the module comment.
    pub fn record_source_file(&self, f: &SourceFile, pending: bool) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO source_files
                 (document_id, context_id, path, byte_size, mtime_ns, sha256, missing_since, pending)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7)",
            params![f.document_id, f.context_id, f.path, f.byte_size, f.mtime_ns, f.sha256, pending],
        )?;
        Ok(())
    }

    /// The finished source rows of `context_id` (not `pending`), by path and document.
    pub fn source_files(&self, context_id: i64) -> Result<Vec<SourceFile>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {FILE_COLS} FROM source_files WHERE context_id = ?1 AND pending = 0 ORDER BY path, document_id"
        ))?;
        let rows = stmt.query_map([context_id], row_to_source_file)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The file at `path` is there with this size and time (content unchanged, or back
    /// after it was missing): store the stat, clear `missing_since`. Returns rows changed.
    pub fn set_source_present(&self, context_id: i64, path: &str, byte_size: i64, mtime_ns: i64) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE source_files SET byte_size = ?3, mtime_ns = ?4, missing_since = NULL
             WHERE context_id = ?1 AND path = ?2 AND pending = 0",
            params![context_id, path, byte_size, mtime_ns],
        )?)
    }

    /// The files at `paths` were gone at the check at `at`: mark them (an earlier mark
    /// is kept). Nothing is deleted. Returns rows newly marked.
    pub fn mark_sources_missing(&self, context_id: i64, paths: &[String], at: i64) -> Result<usize> {
        let mut n = 0;
        for p in paths {
            n += self.conn.execute(
                "UPDATE source_files SET missing_since = ?3
                 WHERE context_id = ?1 AND path = ?2 AND pending = 0 AND missing_since IS NULL",
                params![context_id, p, at],
            )?;
        }
        Ok(n)
    }

    /// A missing file turned up at `to` with the same content (moved or renamed): its
    /// rows follow it. Only rows marked missing move. Returns rows moved.
    pub fn move_missing_source(&self, context_id: i64, from: &str, to: &str, byte_size: i64, mtime_ns: i64) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE source_files SET path = ?3, byte_size = ?4, mtime_ns = ?5, missing_since = NULL
             WHERE context_id = ?1 AND path = ?2 AND pending = 0 AND missing_since IS NOT NULL",
            params![context_id, from, to, byte_size, mtime_ns],
        )?)
    }

    /// An update finished: its documents count from now on.
    pub fn confirm_pending_sources(&self, context_id: i64, document_ids: &[i64]) -> Result<()> {
        for id in document_ids {
            self.conn.execute(
                "UPDATE source_files SET pending = 0 WHERE context_id = ?1 AND document_id = ?2",
                params![context_id, id],
            )?;
        }
        Ok(())
    }

    /// Documents of `context_id` left behind by an update that did not finish.
    pub fn pending_source_documents(&self, context_id: i64) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT document_id FROM source_files WHERE context_id = ?1 AND pending = 1 ORDER BY document_id",
        )?;
        let rows = stmt.query_map([context_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Documents of the given contexts whose file was missing at the last check:
    /// `(document_id, path, missing_since)`, by document.
    pub fn missing_source_documents(&self, context_ids: &[i64]) -> Result<Vec<(i64, String, i64)>> {
        let mut out = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT document_id, path, missing_since FROM source_files
             WHERE context_id = ?1 AND pending = 0 AND missing_since IS NOT NULL ORDER BY document_id",
        )?;
        for &cid in context_ids {
            let rows = stmt.query_map([cid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            out.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        Ok(out)
    }

    /// Documents of `context_id` whose file was missing at the last check — what the
    /// vector scans of retrieval skip (`db/embeddings.rs`, `embedding/retrieval.rs`,
    /// `embedding/image_list.rs`). Usually empty: one indexed query.
    pub fn missing_document_ids(&self, context_id: i64) -> Result<std::collections::HashSet<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT document_id FROM source_files WHERE context_id = ?1 AND pending = 0 AND missing_since IS NOT NULL",
        )?;
        let rows = stmt.query_map([context_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Documents of `context_id` without any source row — added by an older program
    /// (before 0.2.3), which stored no path: `(id, file name, page count, ingested_at)`.
    /// The file name is `zip_entry` when the title differs from it, else `name`.
    /// `add_files` takes such a document over instead of adding its file a second time.
    pub fn documents_without_source(&self, context_id: i64) -> Result<Vec<(i64, String, Option<i64>, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT d.id, COALESCE(d.zip_entry, d.name), d.page_count, d.ingested_at FROM documents d
             WHERE d.context_id = ?1 AND NOT EXISTS (SELECT 1 FROM source_files s WHERE s.document_id = d.id)
             ORDER BY d.id",
        )?;
        let rows = stmt.query_map([context_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Take or renew the lease on `context_id` for `holder` until `now + ttl_s`.
    /// `false` = another holder has it and it has not expired.
    pub fn try_source_lease(&self, context_id: i64, holder: &str, now: i64, ttl_s: i64) -> Result<bool> {
        let n = self.conn.execute(
            "INSERT INTO source_leases (context_id, holder, expires_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (context_id) DO UPDATE SET holder = excluded.holder, expires_at = excluded.expires_at
             WHERE source_leases.holder = excluded.holder OR source_leases.expires_at < ?4",
            params![context_id, holder, now + ttl_s, now],
        )?;
        Ok(n > 0)
    }

    /// Give the lease back (only the holder can).
    pub fn release_source_lease(&self, context_id: i64, holder: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM source_leases WHERE context_id = ?1 AND holder = ?2",
            params![context_id, holder],
        )?;
        Ok(())
    }

    /// Who holds an unexpired lease on `context_id` at `now`.
    pub fn source_lease_holder(&self, context_id: i64, now: i64) -> Result<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row(
                "SELECT holder FROM source_leases WHERE context_id = ?1 AND expires_at >= ?2",
                params![context_id, now],
                |r| r.get(0),
            )
            .optional()?)
    }
}
