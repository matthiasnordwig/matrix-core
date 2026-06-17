//! CRUD for `grid_profiles`.

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{Database, Result};

fn row_to_profile(row: &Row<'_>) -> rusqlite::Result<GridProfile> {
    Ok(GridProfile {
        id: row.get("id")?,
        name: row.get("name")?,
        system_prompt: row.get("system_prompt")?,
        data_format: row.get("data_format")?,
        json_schema: row.get("json_schema")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

impl Database {
    pub fn create_grid_profile(&self, p: &NewGridProfile) -> Result<GridProfile> {
        self.conn.execute(
            "INSERT INTO grid_profiles
                (name, system_prompt, data_format, json_schema)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                p.name,
                p.system_prompt,
                p.data_format,
                p.json_schema,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self
            .grid_profile(id)?
            .expect("row just inserted must exist"))
    }

    pub fn grid_profile(&self, id: i64) -> Result<Option<GridProfile>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM grid_profiles WHERE id = ?1",
                [id],
                row_to_profile,
            )
            .optional()?)
    }

    pub fn list_grid_profiles(&self) -> Result<Vec<GridProfile>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM grid_profiles ORDER BY name")?;
        let rows = stmt.query_map([], row_to_profile)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn update_grid_profile(
        &self,
        id: i64,
        p: &NewGridProfile,
    ) -> Result<GridProfile> {
        self.conn.execute(
            "UPDATE grid_profiles SET
                name = ?2, system_prompt = ?3, data_format = ?4, json_schema = ?5, updated_at = unixepoch()
             WHERE id = ?1",
            params![
                id,
                p.name,
                p.system_prompt,
                p.data_format,
                p.json_schema,
            ],
        )?;
        self.grid_profile(id)?
            .ok_or_else(|| super::CoreError::NotFound(format!("grid_profile {id}")))
    }

    pub fn delete_grid_profile(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM grid_profiles WHERE id = ?1", [id])?
            > 0)
    }
}
