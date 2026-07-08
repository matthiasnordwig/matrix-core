//! CRUD for `ontology_edge_reviews` — the non-blocking review list for edges
//! flagged by the stage-2 negation/polarity verification (see
//! `app/src-tauri/src/ontology/extract/verify.rs`) as "unclear" or whose
//! verification call failed outright. Unlike `ontology_quarantine` (s.
//! `quarantine.rs`), rows here never block the pipeline.
use crate::db::{Database, Result};
use crate::db::models::OntologyEdgeReview;

impl Database {
    pub fn insert_ontology_edge_review(
        &self,
        context_id: i64,
        edge_id: i64,
        chunk_id: Option<i64>,
        relation_type: &str,
        evidence: Option<&str>,
        reason: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_edge_reviews (context_id, edge_id, chunk_id, relation_type, evidence, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![context_id, edge_id, chunk_id, relation_type, evidence, reason],
        )?;
        Ok(())
    }

    /// Joins the reviewed edge's source/target node labels (never orphaned:
    /// `ontology_edge_reviews.edge_id` cascades on edge deletion) so the UI
    /// viewer can render the triplet without a second round-trip.
    pub fn list_ontology_edge_reviews(&self, context_id: i64) -> Result<Vec<OntologyEdgeReview>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.context_id, r.edge_id, r.chunk_id, r.relation_type, r.evidence, r.reason, r.created_at, r.attempts, s.label, t.label
             FROM ontology_edge_reviews r
             JOIN ontology_edges e ON e.id = r.edge_id
             JOIN ontology_nodes s ON s.id = e.source_id
             JOIN ontology_nodes t ON t.id = e.target_id
             WHERE r.context_id = ?1"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            Ok(OntologyEdgeReview {
                id: row.get(0)?,
                context_id: row.get(1)?,
                edge_id: row.get(2)?,
                chunk_id: row.get(3)?,
                relation_type: row.get(4)?,
                evidence: row.get(5)?,
                reason: row.get(6)?,
                created_at: row.get(7)?,
                attempts: row.get(8)?,
                source_label: row.get(9)?,
                target_label: row.get(10)?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn delete_ontology_edge_review(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM ontology_edge_reviews WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Single-row fetch (same join as `list_ontology_edge_reviews`) — used by
    /// the "re-check" action, which needs one review row's `edge_id`/`reason`
    /// to re-run the originating lint rule.
    pub fn get_ontology_edge_review(&self, id: i64) -> Result<Option<OntologyEdgeReview>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.context_id, r.edge_id, r.chunk_id, r.relation_type, r.evidence, r.reason, r.created_at, r.attempts, s.label, t.label
             FROM ontology_edge_reviews r
             JOIN ontology_edges e ON e.id = r.edge_id
             JOIN ontology_nodes s ON s.id = e.source_id
             JOIN ontology_nodes t ON t.id = e.target_id
             WHERE r.id = ?1"
        )?;
        let mut rows = stmt.query_map([id], |row| {
            Ok(OntologyEdgeReview {
                id: row.get(0)?,
                context_id: row.get(1)?,
                edge_id: row.get(2)?,
                chunk_id: row.get(3)?,
                relation_type: row.get(4)?,
                evidence: row.get(5)?,
                reason: row.get(6)?,
                created_at: row.get(7)?,
                attempts: row.get(8)?,
                source_label: row.get(9)?,
                target_label: row.get(10)?,
            })
        })?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    /// Bulk "keep edge": drop review rows by id in one statement instead of N
    /// round-trips from the frontend (triage of large review batches — see
    /// BACKLOG.md's edge-review-triage item). Returns the number of rows
    /// actually deleted. `rusqlite` has no native slice-bind for `IN (...)`,
    /// so the placeholder list is built manually (ids are i64s from our own
    /// DB, never user text — no injection surface).
    pub fn bulk_delete_ontology_edge_reviews(&self, ids: &[i64]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("DELETE FROM ontology_edge_reviews WHERE id IN ({placeholders})");
        let params = rusqlite::params_from_iter(ids.iter());
        let n = self.conn.execute(&sql, params)?;
        Ok(n)
    }

    /// Bulk "delete edge": removes the flagged edges themselves; their review
    /// rows cascade away with them (same FK as the single-edge
    /// `discard_ontology_edge_review` command). Returns the number of edges
    /// actually deleted.
    pub fn bulk_delete_ontology_edges(&self, edge_ids: &[i64]) -> Result<usize> {
        if edge_ids.is_empty() {
            return Ok(0);
        }
        let placeholders = edge_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("DELETE FROM ontology_edges WHERE id IN ({placeholders})");
        let params = rusqlite::params_from_iter(edge_ids.iter());
        let n = self.conn.execute(&sql, params)?;
        Ok(n)
    }

    /// Records (or re-records) a failed polarity verification for an edge and
    /// returns the resulting `attempts` count. Unlike `insert_ontology_edge_review`,
    /// this **dedups per edge**: if a `verification call failed` row already
    /// exists for `edge_id`, it bumps `attempts` and refreshes the reason/error
    /// instead of inserting a duplicate — so re-running the pipeline (or a manual
    /// re-verify) accumulates attempts on one row rather than piling up copies.
    /// Only the `verification call failed:%` class is matched; an `LLM verdict`
    /// row for the same edge is left untouched.
    pub fn upsert_verification_failure(
        &self,
        context_id: i64,
        edge_id: i64,
        chunk_id: Option<i64>,
        relation_type: &str,
        evidence: Option<&str>,
        err: &str,
    ) -> Result<i64> {
        let reason = format!("verification call failed: {err}");
        let existing: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT id, attempts FROM ontology_edge_reviews
                 WHERE edge_id = ?1 AND reason LIKE 'verification call failed:%'
                 LIMIT 1",
                [edge_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();
        match existing {
            Some((row_id, attempts)) => {
                let new_attempts = attempts + 1;
                self.conn.execute(
                    "UPDATE ontology_edge_reviews SET attempts = ?1, reason = ?2, chunk_id = ?3, evidence = ?4 WHERE id = ?5",
                    rusqlite::params![new_attempts, reason, chunk_id, evidence, row_id],
                )?;
                Ok(new_attempts)
            }
            None => {
                self.conn.execute(
                    "INSERT INTO ontology_edge_reviews (context_id, edge_id, chunk_id, relation_type, evidence, reason, attempts)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
                    rusqlite::params![context_id, edge_id, chunk_id, relation_type, evidence, reason],
                )?;
                Ok(1)
            }
        }
    }

    /// Updates a review row's `reason` + `attempts` in place. Used by the
    /// re-verify command when an edge still can't be resolved: bump `attempts`
    /// and refresh the error, or convert a `verification call failed` row into a
    /// genuine `LLM verdict: unclear` once the model finally returns a verdict.
    /// Resolved edges are removed via `delete_ontology_edge_review` instead.
    pub fn update_edge_review(&self, id: i64, reason: &str, attempts: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE ontology_edge_reviews SET reason = ?1, attempts = ?2 WHERE id = ?3",
            rusqlite::params![reason, attempts, id],
        )?;
        Ok(())
    }
}
