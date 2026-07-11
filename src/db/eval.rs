//! CRUD for the LLM-free retrieval eval (schema_v47, RETRIEVAL_QUALITY_PLAN.md
//! AP0): golden sets + entries, runs, and per-entry run results. Pure DB layer —
//! the run *logic* (embedding, anchor resolution, metric computation) lives in
//! `matrix_services::commands::eval`, not here. Models live in `models.rs`
//! (project convention: no model structs in CRUD files).

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{CoreError, Database, Result};

fn row_to_set(row: &Row<'_>) -> rusqlite::Result<EvalGoldenSet> {
    Ok(EvalGoldenSet {
        id: row.get("id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_entry(row: &Row<'_>) -> rusqlite::Result<EvalGoldenEntry> {
    Ok(EvalGoldenEntry {
        id: row.get("id")?,
        set_id: row.get("set_id")?,
        entry_key: row.get("entry_key")?,
        question: row.get("question")?,
        anchors_any: row.get("anchors_any")?,
        note: row.get("note")?,
    })
}

fn row_to_run(row: &Row<'_>) -> rusqlite::Result<EvalRun> {
    Ok(EvalRun {
        id: row.get("id")?,
        set_id: row.get("set_id")?,
        context_ids: row.get("context_ids")?,
        config: row.get("config")?,
        status: row.get("status")?,
        started_at: row.get("started_at")?,
        finished_at: row.get("finished_at")?,
        metrics: row.get("metrics")?,
    })
}

fn row_to_result(row: &Row<'_>) -> rusqlite::Result<EvalRunResult> {
    Ok(EvalRunResult {
        id: row.get("id")?,
        run_id: row.get("run_id")?,
        entry_id: row.get("entry_id")?,
        entry_key: row.get("entry_key")?,
        question: row.get("question")?,
        resolved_chunks: row.get("resolved_chunks")?,
        first_rank: row.get("first_rank")?,
        hit5: row.get("hit5")?,
        hit10: row.get("hit10")?,
        skipped: row.get("skipped")?,
    })
}

impl Database {
    // --- Golden sets -------------------------------------------------------

    pub fn create_eval_golden_set(&self, s: &NewEvalGoldenSet) -> Result<EvalGoldenSet> {
        self.conn.execute(
            "INSERT INTO eval_golden_sets (title, description) VALUES (?1, ?2)",
            params![s.title, s.description],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self.eval_golden_set(id)?.expect("row just inserted must exist"))
    }

    pub fn eval_golden_set(&self, id: i64) -> Result<Option<EvalGoldenSet>> {
        Ok(self
            .conn
            .query_row("SELECT * FROM eval_golden_sets WHERE id = ?1", [id], row_to_set)
            .optional()?)
    }

    pub fn list_eval_golden_sets(&self) -> Result<Vec<EvalGoldenSet>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM eval_golden_sets ORDER BY title")?;
        let rows = stmt.query_map([], row_to_set)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn update_eval_golden_set(&self, id: i64, title: &str, description: &str) -> Result<EvalGoldenSet> {
        self.conn.execute(
            "UPDATE eval_golden_sets SET title = ?2, description = ?3, updated_at = strftime('%s','now') WHERE id = ?1",
            params![id, title, description],
        )?;
        self.eval_golden_set(id)?
            .ok_or_else(|| CoreError::NotFound(format!("eval_golden_set {id}")))
    }

    pub fn delete_eval_golden_set(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM eval_golden_sets WHERE id = ?1", [id])?
            > 0)
    }

    // --- Golden entries ----------------------------------------------------

    pub fn create_eval_golden_entry(&self, e: &NewEvalGoldenEntry) -> Result<EvalGoldenEntry> {
        self.conn.execute(
            "INSERT INTO eval_golden_entries (set_id, entry_key, question, anchors_any, note)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![e.set_id, e.entry_key, e.question, e.anchors_any, e.note],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self.eval_golden_entry(id)?.expect("row just inserted must exist"))
    }

    pub fn eval_golden_entry(&self, id: i64) -> Result<Option<EvalGoldenEntry>> {
        Ok(self
            .conn
            .query_row("SELECT * FROM eval_golden_entries WHERE id = ?1", [id], row_to_entry)
            .optional()?)
    }

    pub fn list_eval_golden_entries(&self, set_id: i64) -> Result<Vec<EvalGoldenEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM eval_golden_entries WHERE set_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map([set_id], row_to_entry)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn update_eval_golden_entry(
        &self,
        id: i64,
        entry_key: &str,
        question: &str,
        anchors_any: &str,
        note: &str,
    ) -> Result<EvalGoldenEntry> {
        self.conn.execute(
            "UPDATE eval_golden_entries SET entry_key = ?2, question = ?3, anchors_any = ?4, note = ?5 WHERE id = ?1",
            params![id, entry_key, question, anchors_any, note],
        )?;
        self.eval_golden_entry(id)?
            .ok_or_else(|| CoreError::NotFound(format!("eval_golden_entry {id}")))
    }

    pub fn delete_eval_golden_entry(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM eval_golden_entries WHERE id = ?1", [id])?
            > 0)
    }

    // --- Runs --------------------------------------------------------------

    pub fn create_eval_run(&self, r: &NewEvalRun) -> Result<EvalRun> {
        self.conn.execute(
            "INSERT INTO eval_runs (set_id, context_ids, config, status) VALUES (?1, ?2, ?3, 'running')",
            params![r.set_id, r.context_ids, r.config],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self.eval_run(id)?.expect("row just inserted must exist"))
    }

    pub fn eval_run(&self, id: i64) -> Result<Option<EvalRun>> {
        Ok(self
            .conn
            .query_row("SELECT * FROM eval_runs WHERE id = ?1", [id], row_to_run)
            .optional()?)
    }

    /// Runs for a given set (most recent first). `set_id = None` lists all runs.
    pub fn list_eval_runs(&self, set_id: Option<i64>) -> Result<Vec<EvalRun>> {
        match set_id {
            Some(sid) => {
                let mut stmt = self
                    .conn
                    .prepare("SELECT * FROM eval_runs WHERE set_id = ?1 ORDER BY id DESC")?;
                let rows = stmt.query_map([sid], row_to_run)?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            }
            None => {
                let mut stmt = self.conn.prepare("SELECT * FROM eval_runs ORDER BY id DESC")?;
                let rows = stmt.query_map([], row_to_run)?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            }
        }
    }

    /// Finalize a run: set status + metrics JSON + finished_at.
    pub fn finish_eval_run(&self, id: i64, status: &str, metrics: &str) -> Result<EvalRun> {
        self.conn.execute(
            "UPDATE eval_runs SET status = ?2, metrics = ?3, finished_at = strftime('%s','now') WHERE id = ?1",
            params![id, status, metrics],
        )?;
        self.eval_run(id)?
            .ok_or_else(|| CoreError::NotFound(format!("eval_run {id}")))
    }

    pub fn delete_eval_run(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM eval_runs WHERE id = ?1", [id])?
            > 0)
    }

    // --- Run results -------------------------------------------------------

    pub fn insert_eval_run_result(&self, r: &NewEvalRunResult) -> Result<()> {
        self.conn.execute(
            "INSERT INTO eval_run_results
                (run_id, entry_id, entry_key, question, resolved_chunks, first_rank, hit5, hit10, skipped)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                r.run_id, r.entry_id, r.entry_key, r.question, r.resolved_chunks,
                r.first_rank, r.hit5, r.hit10, r.skipped
            ],
        )?;
        Ok(())
    }

    pub fn get_eval_run_results(&self, run_id: i64) -> Result<Vec<EvalRunResult>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM eval_run_results WHERE run_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map([run_id], row_to_result)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}
