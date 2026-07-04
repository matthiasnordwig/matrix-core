//! Rolling per-phase performance metrics for the ontology pipeline (extract/
//! dedup/embed/community ms-per-chunk, keeps only the last 3 runs per
//! phase+model). Split out of the former monolithic `db/ontology.rs` — see
//! HANDBUCH.md.
use crate::db::{Database, Result};

impl Database {
    pub fn insert_phase_metric(&self, phase: &str, model_name: &str, ms_per_chunk: f64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_phase_metrics (phase_name, model_name, ms_per_chunk) VALUES (?1, ?2, ?3)",
            rusqlite::params![phase, model_name, ms_per_chunk],
        )?;
        // Keep only the last 3 runs per phase + model. `created_at` has only
        // second resolution (unixepoch()), so several fast local runs can tie
        // on it — `id DESC` (monotonic rowid) breaks the tie by actual insert
        // order instead of leaving it to SQLite's unspecified tie behavior,
        // which previously could keep the oldest 3 rows instead of the newest.
        self.conn.execute(
            "DELETE FROM ontology_phase_metrics WHERE id NOT IN (
                SELECT id FROM ontology_phase_metrics
                WHERE phase_name = ?1 AND model_name = ?2
                ORDER BY created_at DESC, id DESC LIMIT 3
            ) AND phase_name = ?1 AND model_name = ?2",
            rusqlite::params![phase, model_name],
        )?;
        Ok(())
    }

    pub fn get_phase_averages(&self, model_name: &str) -> Result<std::collections::HashMap<String, f64>> {
        let mut stmt = self.conn.prepare(
            "SELECT phase_name, AVG(ms_per_chunk) FROM ontology_phase_metrics WHERE model_name = ?1 GROUP BY phase_name"
        )?;
        let mut map = std::collections::HashMap::new();
        let rows = stmt.query_map([model_name], |row| {
            let phase: String = row.get(0)?;
            let avg: f64 = row.get(1)?;
            Ok((phase, avg))
        })?;
        for r in rows {
            if let Ok((phase, avg)) = r {
                map.insert(phase, avg);
            }
        }
        Ok(map)
    }
}
