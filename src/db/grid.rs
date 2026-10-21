//! Grid surface: the async matrix-chat results (`grid_chat_results`). Chat is not history-aware, so a cell upsert
//! overwrites the row's prior result for the same run.
//!
//! `grid_run_meta` stores, once per run, the system prompt and the grid
//! profile's `json_schema` (instead of duplicating them in every row's
//! `prompt_snapshot`). `json_schema` semantics: a real schema string = JSON
//! profile, NULL = plain-text profile (never explodes), `''` = legacy run
//! (pre-schema_v41, mode unknown, so history loading falls back to lenient
//! shape-based explosion). Since schema_v66 it also records how the run was
//! started (`GridRunInfo`: model, agentic budget, template, context mode),
//! which `list_grid_runs` joins into every history entry.

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{Database, Result};

/// The schema_v66 start-settings columns of `grid_run_meta` (NULL when the run
/// has no meta row or predates v66).
fn row_to_run_info(row: &Row<'_>) -> rusqlite::Result<GridRunInfo> {
    Ok(GridRunInfo {
        llm_endpoint_id: row.get("llm_endpoint_id")?,
        pool_id: row.get("pool_id")?,
        model_label: row.get("model_label")?,
        tool_call_budget: row.get("tool_call_budget")?,
        template_name: row.get("template_name")?,
        context_mode: row.get("context_mode")?,
    })
}

fn row_to_chat_result(row: &Row<'_>) -> rusqlite::Result<GridChatResult> {
    Ok(GridChatResult {
        id: row.get("id")?,
        run_id: row.get("run_id")?,
        row_uid: row.get("row_uid")?,
        row_ref_type: row.get("row_ref_type")?,
        row_ref_id: row.get("row_ref_id")?,
        prompt: row.get("prompt")?,
        columns_context: row.get("columns_context")?,
        retrieved_refs: row.get("retrieved_refs")?,
        response: row.get("response")?,
        status: row.get("status")?,
        error: row.get("error")?,
        prompt_snapshot: row.get("prompt_snapshot")?,
        row_source_text: row.get("row_source_text")?,
        updated_at: row.get("updated_at")?,
    })
}

impl Database {
    // --- chat results ------------------------------------------------------

    /// Insert or overwrite the chat cell for `(run_id, row)`.
    pub fn upsert_grid_chat_result(&self, r: &GridChatUpsert) -> Result<GridChatResult> {
        self.conn.execute(
            "INSERT INTO grid_chat_results
                (run_id, row_uid, row_ref_type, row_ref_id, prompt, columns_context,
                 retrieved_refs, response, status, error, prompt_snapshot, row_source_text, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, unixepoch())
             ON CONFLICT(run_id, row_uid) DO UPDATE SET
                row_ref_type = excluded.row_ref_type,
                row_ref_id = excluded.row_ref_id,
                prompt = excluded.prompt,
                columns_context = excluded.columns_context,
                retrieved_refs = excluded.retrieved_refs,
                response = excluded.response,
                status = excluded.status,
                error = excluded.error,
                prompt_snapshot = excluded.prompt_snapshot,
                row_source_text = excluded.row_source_text,
                updated_at = unixepoch()",
            params![
                r.run_id,
                r.row_uid,
                r.row_ref_type,
                r.row_ref_id,
                r.prompt,
                r.columns_context,
                r.retrieved_refs,
                r.response,
                r.status,
                r.error,
                r.prompt_snapshot,
                r.row_source_text,
            ],
        )?;
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM grid_chat_results
                 WHERE run_id = ?1 AND row_uid = ?2",
                params![r.run_id, r.row_uid],
                row_to_chat_result,
            )?)
    }

    /// All chat cells for a run, ordered by row.
    pub fn list_grid_chat_results(&self, run_id: &str) -> Result<Vec<GridChatResult>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM grid_chat_results WHERE run_id = ?1 ORDER BY row_ref_id",
        )?;
        let rows = stmt.query_map([run_id], row_to_chat_result)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn count_grid_chat_results(&self, run_id: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM grid_chat_results WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )?)
    }

    /// History entries, newest first, each with the start settings from
    /// `grid_run_meta` (all `None` for runs without meta or from before v66).
    pub fn list_grid_runs(&self) -> Result<Vec<GridRun>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.run_id, r.prompt, r.updated_at, m.llm_endpoint_id, m.pool_id, m.model_label,
                    m.tool_call_budget, m.template_name, m.context_mode
             FROM (SELECT run_id, MIN(prompt) AS prompt, MAX(updated_at) AS updated_at
                   FROM grid_chat_results GROUP BY run_id) r
             LEFT JOIN grid_run_meta m ON m.run_id = r.run_id
             ORDER BY r.updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(GridRun {
                run_id: row.get("run_id")?,
                prompt: row.get("prompt")?,
                updated_at: row.get("updated_at")?,
                info: row_to_run_info(row)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Deletes `run_id`'s `grid_chat_results` rows whose `row_uid` is NOT in
    /// `keep_uids`. Used when overwriting a history-loaded run (`boundRunId` in
    /// GridTab.tsx): a re-run only re-sends the rows still present in the grid,
    /// so any row the user deleted locally before re-running would otherwise
    /// survive in the DB under the same `run_id` and reappear as an orphan on
    /// the next history load (see ISSUES.md "Grid-Run-Overwrite..."). Returns
    /// the number of deleted rows. Empty `keep_uids` prunes the whole run's
    /// results (but not `grid_run_meta` — mirrors `delete_grid_chat_run` only
    /// dropping meta when the run itself is deleted, not on a partial prune).
    pub fn prune_grid_run(&self, run_id: &str, keep_uids: &[String]) -> Result<usize> {
        if keep_uids.is_empty() {
            return Ok(self.conn.execute(
                "DELETE FROM grid_chat_results WHERE run_id = ?1",
                [run_id],
            )?);
        }
        let placeholders = std::iter::repeat("?")
            .take(keep_uids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "DELETE FROM grid_chat_results WHERE run_id = ? AND row_uid NOT IN ({placeholders})"
        );
        let mut params_vec: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + keep_uids.len());
        params_vec.push(&run_id);
        for uid in keep_uids {
            params_vec.push(uid);
        }
        Ok(self.conn.execute(&sql, params_vec.as_slice())?)
    }

    pub fn delete_grid_chat_run(&self, run_id: &str) -> Result<bool> {
        // Also drop the run's metadata row (system prompt) so deleting a run
        // leaves no orphan in `grid_run_meta`.
        self.conn.execute("DELETE FROM grid_run_meta WHERE run_id = ?1", [run_id])?;
        Ok(self.conn.execute(
            "DELETE FROM grid_chat_results WHERE run_id = ?1",
            [run_id],
        )? > 0)
    }

    // --- run metadata (AP7: system prompt stored once per run instead of
    // duplicated inside every row's `prompt_snapshot`) -----------------------

    pub fn upsert_grid_run_meta(&self, run_id: &str, system_prompt: &str, json_schema: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO grid_run_meta (run_id, system_prompt, json_schema) VALUES (?1, ?2, ?3)
             ON CONFLICT(run_id) DO UPDATE SET
                system_prompt = excluded.system_prompt,
                json_schema = excluded.json_schema",
            params![run_id, system_prompt, json_schema],
        )?;
        Ok(())
    }

    pub fn grid_run_system_prompt(&self, run_id: &str) -> Result<Option<String>> {
        Ok(self.grid_run_meta(run_id)?.map(|m| m.system_prompt))
    }

    /// The run's stored system prompt + grid-profile JSON schema (the
    /// `{mode,fields}` string, `None` for plain-text profiles). Used by history
    /// loading to gate row explosion on the run's own mode. `None` for runs
    /// created before `grid_run_meta` existed.
    pub fn grid_run_meta(&self, run_id: &str) -> Result<Option<GridRunMeta>> {
        Ok(self
            .conn
            .query_row(
                "SELECT system_prompt, json_schema, llm_endpoint_id, pool_id, model_label,
                        tool_call_budget, template_name, context_mode
                 FROM grid_run_meta WHERE run_id = ?1",
                [run_id],
                |row| {
                    Ok(GridRunMeta {
                        system_prompt: row.get("system_prompt")?,
                        json_schema: row.get("json_schema")?,
                        info: row_to_run_info(row)?,
                    })
                },
            )
            .optional()?)
    }

    /// Records how the run was started (model, agentic budget, template,
    /// context mode). Call after `upsert_grid_run_meta`; a re-run under the
    /// same `run_id` overwrites the previous values.
    pub fn set_grid_run_info(&self, run_id: &str, info: &GridRunInfo) -> Result<()> {
        self.conn.execute(
            "UPDATE grid_run_meta SET llm_endpoint_id = ?2, pool_id = ?3, model_label = ?4,
                    tool_call_budget = ?5, template_name = ?6, context_mode = ?7
             WHERE run_id = ?1",
            params![
                run_id,
                info.llm_endpoint_id,
                info.pool_id,
                info.model_label,
                info.tool_call_budget,
                info.template_name,
                info.context_mode
            ],
        )?;
        Ok(())
    }
}
