//! CRUD for `ref_abbreviations` (schema_v56, TOOL_TIER_PLAN.md Teil B).
//!
//! `long_names` is a JSON array of strings in the DB (`TEXT`), mapped to/from
//! `Vec<String>` here — same pattern as `reasoning_lists.rs`'
//! `allowed_efforts`. `kuerzel` is trimmed + lowercased on every write so the
//! stored value always matches `RefLexicon`'s lowercase matching key (the
//! `COLLATE NOCASE UNIQUE` constraint on the column additionally guards
//! against a race inserting a differently-cased duplicate).

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{Database, Result};

fn row_to_ref_abbreviation(row: &Row<'_>) -> rusqlite::Result<RefAbbreviation> {
    let long_names_json: String = row.get("long_names")?;
    let long_names = serde_json::from_str::<Vec<String>>(&long_names_json).unwrap_or_default();
    Ok(RefAbbreviation {
        id: row.get("id")?,
        kuerzel: row.get("kuerzel")?,
        long_names,
        enabled: row.get("enabled")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Trim + lowercase a Kürzel for storage (matches `RefLexicon`'s matching key).
fn normalize_kuerzel(kuerzel: &str) -> String {
    kuerzel.trim().to_lowercase()
}

impl Database {
    pub fn list_ref_abbreviations(&self) -> Result<Vec<RefAbbreviation>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM ref_abbreviations ORDER BY kuerzel")?;
        let rows = stmt.query_map([], row_to_ref_abbreviation)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn create_ref_abbreviation(&self, a: &NewRefAbbreviation) -> Result<RefAbbreviation> {
        let kuerzel = normalize_kuerzel(&a.kuerzel);
        let long_names_json = serde_json::to_string(&a.long_names).unwrap_or_else(|_| "[]".into());
        self.conn.execute(
            "INSERT INTO ref_abbreviations (kuerzel, long_names, enabled)
             VALUES (?1, ?2, ?3)",
            params![kuerzel, long_names_json, a.enabled],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self
            .ref_abbreviation(id)?
            .expect("row just inserted must exist"))
    }

    pub fn ref_abbreviation(&self, id: i64) -> Result<Option<RefAbbreviation>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM ref_abbreviations WHERE id = ?1",
                [id],
                row_to_ref_abbreviation,
            )
            .optional()?)
    }

    pub fn update_ref_abbreviation(
        &self,
        id: i64,
        a: &NewRefAbbreviation,
    ) -> Result<RefAbbreviation> {
        let kuerzel = normalize_kuerzel(&a.kuerzel);
        let long_names_json = serde_json::to_string(&a.long_names).unwrap_or_else(|_| "[]".into());
        self.conn.execute(
            "UPDATE ref_abbreviations SET
                kuerzel = ?2, long_names = ?3, enabled = ?4, updated_at = unixepoch()
             WHERE id = ?1",
            params![id, kuerzel, long_names_json, a.enabled],
        )?;
        self.ref_abbreviation(id)?
            .ok_or_else(|| super::CoreError::NotFound(format!("ref_abbreviation {id}")))
    }

    pub fn delete_ref_abbreviation(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM ref_abbreviations WHERE id = ?1", [id])?
            > 0)
    }
}

#[cfg(test)]
#[path = "ref_abbreviations_tests.rs"]
mod ref_abbreviations_tests;
