//! CRUD for the Lens system (`ontology_lenses`/`ontology_lens_node_types`/
//! `ontology_lens_edge_verdicts`) — see BACKLOG.md "Schema-Labeling ohne
//! destruktives Sanitize" and `app/src-tauri/src/ontology/extract/sanitize.rs`
//! (`materialize_lens`, the writer of these tables).
use crate::db::{Database, Result};
use crate::db::models::OntologyLens;

fn row_to_lens(row: &rusqlite::Row<'_>) -> rusqlite::Result<OntologyLens> {
    Ok(OntologyLens {
        id: row.get("id")?,
        context_id: row.get("context_id")?,
        name: row.get("name")?,
        ontology_profile_id: row.get("ontology_profile_id")?,
        is_extraction_lens: row.get("is_extraction_lens")?,
        created_at: row.get("created_at")?,
    })
}

impl Database {
    /// Creates a lens for (context_id, profile_id) if none exists yet,
    /// otherwise refreshes its `name`/`is_extraction_lens` in place — a lens
    /// is identified by *which profile it materializes*, not by *when* it was
    /// created, so re-running materialization (e.g. after a delta-extraction)
    /// updates the same row instead of orphaning a new one each time.
    /// `is_extraction_lens` only ever turns true → stays true (a lens first
    /// added standalone can later legitimately be used by a real extraction
    /// run, e.g. a delta-extraction with that profile).
    pub fn get_or_create_lens(&self, context_id: i64, name: &str, profile_id: i64, is_extraction_lens: bool) -> Result<OntologyLens> {
        self.conn.execute(
            "INSERT INTO ontology_lenses (context_id, name, ontology_profile_id, is_extraction_lens)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(context_id, ontology_profile_id) DO UPDATE SET
                name = excluded.name,
                is_extraction_lens = is_extraction_lens OR excluded.is_extraction_lens",
            rusqlite::params![context_id, name, profile_id, is_extraction_lens],
        )?;
        let lens = self.conn.query_row(
            "SELECT id, context_id, name, ontology_profile_id, is_extraction_lens, created_at
             FROM ontology_lenses WHERE context_id = ?1 AND ontology_profile_id = ?2",
            rusqlite::params![context_id, profile_id],
            row_to_lens,
        )?;
        Ok(lens)
    }

    pub fn list_lenses_for_context(&self, context_id: i64) -> Result<Vec<OntologyLens>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, context_id, name, ontology_profile_id, is_extraction_lens, created_at
             FROM ontology_lenses WHERE context_id = ?1 ORDER BY created_at"
        )?;
        let rows = stmt.query_map([context_id], row_to_lens)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn upsert_lens_node_type(&self, lens_id: i64, node_id: i64, resolved_type: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_lens_node_types (lens_id, node_id, resolved_type) VALUES (?1, ?2, ?3)
             ON CONFLICT(lens_id, node_id) DO UPDATE SET resolved_type = excluded.resolved_type",
            rusqlite::params![lens_id, node_id, resolved_type],
        )?;
        Ok(())
    }

    /// `resolved_relation_type` may be `None` when only the verdict itself
    /// changes (e.g. `verify_edge_polarity` marking an already-materialized
    /// edge `deleted`) — in that case the previously-resolved type, if any,
    /// is preserved rather than clobbered.
    pub fn upsert_lens_edge_verdict(&self, lens_id: i64, edge_id: i64, verdict: &str, resolved_relation_type: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_lens_edge_verdicts (lens_id, edge_id, verdict, resolved_relation_type) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(lens_id, edge_id) DO UPDATE SET
                verdict = excluded.verdict,
                resolved_relation_type = COALESCE(excluded.resolved_relation_type, ontology_lens_edge_verdicts.resolved_relation_type)",
            rusqlite::params![lens_id, edge_id, verdict, resolved_relation_type],
        )?;
        Ok(())
    }

    pub fn set_context_active_lens(&self, context_id: i64, lens_id: Option<i64>) -> Result<()> {
        self.conn.execute(
            "UPDATE contexts SET active_lens_id = ?1, updated_at = unixepoch() WHERE id = ?2",
            rusqlite::params![lens_id, context_id],
        )?;
        Ok(())
    }

    /// Deleting a lens is always safe: raw types live on the nodes/edges
    /// themselves (`raw_entity_type`/`raw_relation_type`), not here, so a
    /// deleted lens can be recreated any time via `get_or_create_lens`
    /// without new LLM calls. Mapping/verdict rows cascade; if this was the
    /// context's active lens, `contexts.active_lens_id` falls back to NULL
    /// (raw/unfiltered) via the column's `ON DELETE SET NULL`.
    pub fn delete_lens(&self, lens_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM ontology_lenses WHERE id = ?1", [lens_id])?;
        Ok(())
    }
}
