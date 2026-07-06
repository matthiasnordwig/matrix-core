//! CRUD for `ontology_communities` (label-propagation community summaries)
//! and `ontology_community_members` (per-lens membership, schema_v37 — a
//! node can be in community X under lens A and Y under lens B, so membership
//! is no longer written to the legacy single-valued
//! `ontology_nodes.community_id`).
//!
//! NOTE on the Uniper2 community-coloring bugfix: this file intentionally
//! does NOT contain a bulk "assign raw label-propagation IDs to all nodes"
//! method anymore (there used to be an `assign_communities` here). See
//! `app/src-tauri/src/ontology/community/mod.rs` for the full explanation — in
//! short, that method wrote non-`ontology_communities` IDs into
//! `ontology_nodes.community_id`, which broke the frontend's community color
//! lookup for nodes outside the top-100-by-size cutoff.
use crate::db::{Database, Result};
use crate::db::models::{OntologyCommunity, OntologyCommunityWithMembers};

const COMMUNITY_COLS: &str =
    "id, context_id, community_label, node_count, summary_text, lens_id, members_key, created_at";

fn row_to_community(row: &rusqlite::Row) -> rusqlite::Result<OntologyCommunity> {
    Ok(OntologyCommunity {
        id: row.get(0)?,
        context_id: row.get(1)?,
        community_label: row.get(2)?,
        node_count: row.get(3)?,
        summary_text: row.get(4)?,
        lens_id: row.get(5)?,
        members_key: row.get(6)?,
        created_at: row.get(7)?,
    })
}

impl Database {
    /// `lens_id` `None` = raw/unfiltered view; `members_key` `None` only for
    /// legacy-style rows that should never cache-hit (e.g. imports whose
    /// member set is unknown).
    pub fn create_ontology_community(
        &self,
        context_id: i64,
        community_label: &str,
        node_count: i64,
        summary_text: &str,
        lens_id: Option<i64>,
        members_key: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO ontology_communities (context_id, community_label, node_count, summary_text, lens_id, members_key) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![context_id, community_label, node_count, summary_text, lens_id, members_key]
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Replaces the member set of one community (insert-only in practice —
    /// communities are always freshly created per (re)compute, see
    /// `delete_communities_for_lens`).
    pub fn set_community_members(&self, community_id: i64, node_ids: &[i64]) -> Result<()> {
        self.conn.execute(
            "DELETE FROM ontology_community_members WHERE community_id = ?1",
            [community_id],
        )?;
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR IGNORE INTO ontology_community_members (community_id, node_id) VALUES (?1, ?2)",
        )?;
        for node_id in node_ids {
            stmt.execute(rusqlite::params![community_id, node_id])?;
        }
        Ok(())
    }

    pub fn update_community_vector(&self, community_id: i64, vector_blob: &[u8]) -> Result<()> {
        self.conn.execute("UPDATE ontology_communities SET vector_blob = ?1 WHERE id = ?2", rusqlite::params![vector_blob, community_id])?;
        Ok(())
    }

    /// All communities of a context regardless of lens — used by
    /// context-transfer export and legacy call sites; per-lens readers use
    /// `list_communities_for_lens`.
    pub fn list_ontology_communities(&self, context_id: i64) -> Result<Vec<OntologyCommunity>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COMMUNITY_COLS} FROM ontology_communities WHERE context_id = ?1"
        ))?;
        let rows = stmt.query_map([context_id], row_to_community)?
            .filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Communities of one lens (`None` = raw view / legacy rows), including
    /// their member node ids. SQLite's `IS` operator handles the NULL
    /// comparison for the raw view.
    pub fn list_communities_for_lens(
        &self,
        context_id: i64,
        lens_id: Option<i64>,
    ) -> Result<Vec<OntologyCommunityWithMembers>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COMMUNITY_COLS} FROM ontology_communities WHERE context_id = ?1 AND lens_id IS ?2"
        ))?;
        let communities: Vec<OntologyCommunity> = stmt
            .query_map(rusqlite::params![context_id, lens_id], row_to_community)?
            .filter_map(|r| r.ok())
            .collect();

        let mut m_stmt = self.conn.prepare_cached(
            "SELECT node_id FROM ontology_community_members WHERE community_id = ?1 ORDER BY node_id",
        )?;
        let mut out = Vec::with_capacity(communities.len());
        for c in communities {
            let member_ids: Vec<i64> = m_stmt
                .query_map([c.id], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            out.push(OntologyCommunityWithMembers {
                id: c.id,
                context_id: c.context_id,
                community_label: c.community_label,
                node_count: c.node_count,
                summary_text: c.summary_text,
                lens_id: c.lens_id,
                members_key: c.members_key,
                created_at: c.created_at,
                member_ids,
            });
        }
        Ok(out)
    }

    /// Summary-cache lookup: returns `(summary_text, vector_blob)` of an
    /// existing community with the same member set **under the same lens**
    /// (cache key is `(lens_id, members_key)`, not `members_key` alone — the
    /// same node set can deserve different summaries under different lenses;
    /// architecture-review 2026-07-06). Rows with an empty summary (created
    /// by a cache-miss recompute) never count as hits.
    pub fn find_community_by_members_key(
        &self,
        context_id: i64,
        lens_id: Option<i64>,
        members_key: &str,
    ) -> Result<Option<(String, Option<Vec<u8>>)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT summary_text, vector_blob FROM ontology_communities
             WHERE context_id = ?1 AND lens_id IS ?2 AND members_key = ?3 AND summary_text != ''
             LIMIT 1",
        )?;
        let mut rows = stmt.query(rusqlite::params![context_id, lens_id, members_key])?;
        if let Some(row) = rows.next()? {
            Ok(Some((row.get(0)?, row.get(1)?)))
        } else {
            Ok(None)
        }
    }

    /// Deletes only one lens's communities (members cascade), leaving other
    /// lenses' rows in place as the lazy summary cache — switching back to a
    /// previous lens is exactly the case the cache exists for.
    pub fn delete_communities_for_lens(&self, context_id: i64, lens_id: Option<i64>) -> Result<()> {
        self.conn.execute(
            "DELETE FROM ontology_communities WHERE context_id = ?1 AND lens_id IS ?2",
            rusqlite::params![context_id, lens_id],
        )?;
        Ok(())
    }

    /// Full wipe across all lenses (context re-extraction/lifecycle reset).
    /// Also clears the legacy `ontology_nodes.community_id` for pre-v37 rows.
    pub fn delete_communities_for_context(&self, context_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM ontology_communities WHERE context_id = ?1", [context_id])?;
        self.conn.execute("UPDATE ontology_nodes SET community_id = NULL WHERE context_id = ?1", [context_id])?;
        Ok(())
    }

    /// Communities still lacking an embedding vector — after a pipeline run,
    /// cache-hit communities already carry their copied vector and other
    /// lenses' rows must not be re-embedded, so `embed_communities` embeds
    /// only these.
    pub fn list_communities_missing_vector(&self, context_id: i64) -> Result<Vec<OntologyCommunity>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COMMUNITY_COLS} FROM ontology_communities
             WHERE context_id = ?1 AND vector_blob IS NULL AND summary_text != ''"
        ))?;
        let rows = stmt.query_map([context_id], row_to_community)?
            .filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// All membership pairs of a context as `(community_id, node_id)` — used
    /// by context-transfer export.
    pub fn list_community_members_for_context(&self, context_id: i64) -> Result<Vec<(i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.community_id, m.node_id FROM ontology_community_members m
             JOIN ontology_communities co ON co.id = m.community_id
             WHERE co.context_id = ?1",
        )?;
        let rows = stmt.query_map([context_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok()).collect();
        Ok(rows)
    }
}
