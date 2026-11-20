//! The search memory of Tables (schema_v80): one finished row search per entry, keyed by a hash the
//! services crate builds (`commands/grid_search_key.rs`) from the search text, the sources and every
//! search setting; the value is the ordered hits as JSON, no texts. Entries are not bound to a run.
//!
//! What keeps it honest lives in the schema, not here: every write that can change a collection's
//! search results raises `contexts.search_version` (triggers on passages, vectors, page-image and
//! paragraph vectors, source files, the embedding model), the stamp goes into the key, and the
//! collection's entries are deleted at once (`grid_search_cache_contexts`). This file only reads the
//! stamps, looks up and stores entries and keeps the count under the caller's cap, dropping the
//! least recently used first. Tests: `grid_search_cache_tests.rs`.

use std::collections::HashMap;

use rusqlite::{params, OptionalExtension};

use super::models::SearchStamp;
use super::{Database, Result};

impl Database {
    /// The stamps of `context_ids`, in the given order; unknown ids are left out.
    pub fn context_search_stamps(&self, context_ids: &[i64]) -> Result<Vec<SearchStamp>> {
        let mut stmt = self.conn.prepare_cached("SELECT search_version, embedding_model_id FROM contexts WHERE id = ?1")?;
        let mut out = Vec::with_capacity(context_ids.len());
        for &id in context_ids {
            let row = stmt
                .query_row([id], |r| Ok(SearchStamp { context_id: id, search_version: r.get(0)?, embedding_model_id: r.get(1)? }))
                .optional()?;
            out.extend(row);
        }
        Ok(out)
    }

    /// The stored hits of those `keys` that have an entry; each found entry counts as used now.
    pub fn grid_search_cache_get(&self, keys: &[String]) -> Result<HashMap<String, String>> {
        let mut found = HashMap::new();
        if keys.is_empty() {
            return Ok(found);
        }
        self.in_savepoint("grid_search_cache_get", || {
            let mut get = self.conn.prepare_cached("SELECT hits FROM grid_search_cache WHERE key = ?1")?;
            let mut touch = self.conn.prepare_cached(
                "UPDATE grid_search_cache SET last_used = (SELECT COALESCE(MAX(last_used), 0) + 1 FROM grid_search_cache) WHERE key = ?1",
            )?;
            for key in keys {
                if let Some(hits) = get.query_row([key], |r| r.get::<_, String>(0)).optional()? {
                    touch.execute([key])?;
                    found.insert(key.clone(), hits);
                }
            }
            Ok::<_, super::CoreError>(())
        })?;
        Ok(found)
    }

    /// Stores `entries` (key, hits JSON) as searched in `context_ids`, then drops the least recently
    /// used entries beyond `cap`.
    pub fn grid_search_cache_put(&self, entries: &[(String, String)], context_ids: &[i64], cap: usize) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        self.in_savepoint("grid_search_cache_put", || {
            let mut put = self.conn.prepare_cached(
                "INSERT INTO grid_search_cache (key, hits, last_used)
                 VALUES (?1, ?2, (SELECT COALESCE(MAX(last_used), 0) + 1 FROM grid_search_cache))
                 ON CONFLICT(key) DO UPDATE SET hits = excluded.hits, last_used = excluded.last_used",
            )?;
            let mut link = self.conn.prepare_cached(
                "INSERT OR IGNORE INTO grid_search_cache_contexts (context_id, key)
                 SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM contexts WHERE id = ?1)",
            )?;
            for (key, hits) in entries {
                put.execute(params![key, hits])?;
                for &cid in context_ids {
                    link.execute(params![cid, key])?;
                }
            }
            self.conn.execute(
                "DELETE FROM grid_search_cache WHERE key IN (
                     SELECT key FROM grid_search_cache ORDER BY last_used ASC
                     LIMIT MAX(0, (SELECT COUNT(*) FROM grid_search_cache) - ?1))",
                [cap as i64],
            )?;
            Ok::<_, super::CoreError>(())
        })
    }

    /// How many entries the memory holds.
    pub fn grid_search_cache_len(&self) -> Result<usize> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM grid_search_cache", [], |r| r.get::<_, i64>(0))? as usize)
    }
}
