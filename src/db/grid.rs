//! Grid surface: `grid_sheets`, `grid_rows`, and the async matrix-chat
//! results (`grid_chat_results`). Chat is not history-aware, so a cell upsert
//! overwrites the row's prior result for the same run.

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{Database, Result};

fn row_to_sheet(row: &Row<'_>) -> rusqlite::Result<GridSheet> {
    Ok(GridSheet {
        id: row.get("id")?,
        name: row.get("name")?,
        columns: row.get("columns")?,
        row_count: row.get("row_count")?,
        created_at: row.get("created_at")?,
    })
}

fn row_to_grid_row(row: &Row<'_>) -> rusqlite::Result<GridRow> {
    Ok(GridRow {
        id: row.get("id")?,
        sheet_id: row.get("sheet_id")?,
        source_chunk_id: row.get("source_chunk_id")?,
        row_index: row.get("row_index")?,
        data: row.get("data")?,
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
        updated_at: row.get("updated_at")?,
    })
}

impl Database {
    // --- sheets & rows -----------------------------------------------------

    pub fn create_grid_sheet(&self, s: &NewGridSheet) -> Result<GridSheet> {
        self.conn.execute(
            "INSERT INTO grid_sheets (name, columns) VALUES (?1, ?2)",
            params![s.name, s.columns],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self
            .conn
            .query_row("SELECT * FROM grid_sheets WHERE id = ?1", [id], row_to_sheet)?)
    }

    pub fn create_grid_row(&self, r: &NewGridRow) -> Result<GridRow> {
        self.conn.execute(
            "INSERT INTO grid_rows (sheet_id, source_chunk_id, row_index, data)
             VALUES (?1, ?2, ?3, ?4)",
            params![r.sheet_id, r.source_chunk_id, r.row_index, r.data],
        )?;
        if let Some(sheet_id) = r.sheet_id {
            self.conn.execute(
                "UPDATE grid_sheets SET row_count = row_count + 1 WHERE id = ?1",
                [sheet_id],
            )?;
        }
        let id = self.conn.last_insert_rowid();
        Ok(self
            .conn
            .query_row("SELECT * FROM grid_rows WHERE id = ?1", [id], row_to_grid_row)?)
    }

    pub fn delete_grid_row(&self, id: i64) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM grid_rows WHERE id = ?1", [id])? > 0)
    }

    // --- chat results ------------------------------------------------------

    /// Insert or overwrite the chat cell for `(run_id, row)`.
    pub fn upsert_grid_chat_result(&self, r: &GridChatUpsert) -> Result<GridChatResult> {
        self.conn.execute(
            "INSERT INTO grid_chat_results
                (run_id, row_uid, row_ref_type, row_ref_id, prompt, columns_context,
                 retrieved_refs, response, status, error, prompt_snapshot, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, unixepoch())
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

    pub fn list_grid_runs(&self) -> Result<Vec<GridRun>> {
        let mut stmt = self.conn.prepare(
            "SELECT run_id, MIN(prompt) as prompt, MAX(updated_at) as updated_at 
             FROM grid_chat_results 
             GROUP BY run_id 
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(GridRun {
                run_id: row.get("run_id")?,
                prompt: row.get("prompt")?,
                updated_at: row.get("updated_at")?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
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

    pub fn upsert_grid_run_meta(&self, run_id: &str, system_prompt: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO grid_run_meta (run_id, system_prompt) VALUES (?1, ?2)
             ON CONFLICT(run_id) DO UPDATE SET system_prompt = excluded.system_prompt",
            params![run_id, system_prompt],
        )?;
        Ok(())
    }

    pub fn grid_run_system_prompt(&self, run_id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT system_prompt FROM grid_run_meta WHERE run_id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .optional()?)
    }
}
