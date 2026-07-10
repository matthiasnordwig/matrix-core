//! CRUD for `reasoning_effort_lists`.
//!
//! `allowed_efforts` is a JSON array of level strings in the DB (`TEXT`), mapped
//! to/from `Vec<String>` here so callers work with a typed list. A malformed or
//! NULL JSON blob degrades to an empty list rather than erroring — a list with no
//! allowed levels simply offers nothing, which the extraction clamp treats as
//! "no valid level" (falls back to the provider default).

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{Database, Result};

fn row_to_list(row: &Row<'_>) -> rusqlite::Result<ReasoningEffortList> {
    let efforts_json: String = row.get("allowed_efforts")?;
    let allowed_efforts = serde_json::from_str::<Vec<String>>(&efforts_json).unwrap_or_default();
    Ok(ReasoningEffortList {
        id: row.get("id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        allowed_efforts,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

impl Database {
    pub fn create_reasoning_effort_list(
        &self,
        l: &NewReasoningEffortList,
    ) -> Result<ReasoningEffortList> {
        let efforts_json = serde_json::to_string(&l.allowed_efforts).unwrap_or_else(|_| "[]".into());
        self.conn.execute(
            "INSERT INTO reasoning_effort_lists (title, description, allowed_efforts)
             VALUES (?1, ?2, ?3)",
            params![l.title, l.description, efforts_json],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self
            .reasoning_effort_list(id)?
            .expect("row just inserted must exist"))
    }

    pub fn reasoning_effort_list(&self, id: i64) -> Result<Option<ReasoningEffortList>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM reasoning_effort_lists WHERE id = ?1",
                [id],
                row_to_list,
            )
            .optional()?)
    }

    pub fn list_reasoning_effort_lists(&self) -> Result<Vec<ReasoningEffortList>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM reasoning_effort_lists ORDER BY title")?;
        let rows = stmt.query_map([], row_to_list)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn update_reasoning_effort_list(
        &self,
        id: i64,
        l: &NewReasoningEffortList,
    ) -> Result<ReasoningEffortList> {
        let efforts_json = serde_json::to_string(&l.allowed_efforts).unwrap_or_else(|_| "[]".into());
        self.conn.execute(
            "UPDATE reasoning_effort_lists SET
                title = ?2, description = ?3, allowed_efforts = ?4, updated_at = unixepoch()
             WHERE id = ?1",
            params![id, l.title, l.description, efforts_json],
        )?;
        self.reasoning_effort_list(id)?
            .ok_or_else(|| super::CoreError::NotFound(format!("reasoning_effort_list {id}")))
    }

    pub fn delete_reasoning_effort_list(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM reasoning_effort_lists WHERE id = ?1", [id])?
            > 0)
    }
}
