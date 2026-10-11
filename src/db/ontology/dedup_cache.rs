//! Cache of prior LLM dedup decisions (`ontology_dedup_cache`), keyed by
//! unordered node-ID pair, so re-running dedup on a context doesn't re-ask
//! the LLM about pairs it has already judged. Reads return both directions of
//! each pair. The cache records decisions only, not merge history (that is
//! `ontology_merge_log`, see `nodes.rs`).
use crate::db::{Database, Result};

impl Database {
    pub fn cache_dedup_decision(&self, context_id: i64, id1: i64, id2: i64, identical: bool) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO ontology_dedup_cache (context_id, id1, id2, identical) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![context_id, id1, id2, identical],
        )?;
        Ok(())
    }

    pub fn get_dedup_cache(&self, context_id: i64) -> Result<std::collections::HashMap<(i64, i64), bool>> {
        let mut stmt = self.conn.prepare(
            "SELECT id1, id2, identical FROM ontology_dedup_cache WHERE context_id = ?1"
        )?;
        let mut map = std::collections::HashMap::new();
        let rows = stmt.query_map([context_id], |row| {
            let id1: i64 = row.get(0)?;
            let id2: i64 = row.get(1)?;
            let identical: bool = row.get(2)?;
            Ok((id1, id2, identical))
        })?;
        for r in rows {
            if let Ok((id1, id2, identical)) = r {
                map.insert((id1, id2), identical);
                map.insert((id2, id1), identical); // Store both directions
            }
        }
        Ok(map)
    }
}
