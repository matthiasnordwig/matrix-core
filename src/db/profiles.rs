//! CRUD for `chunking_profiles`.

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{Database, Result};

fn row_to_profile(row: &Row<'_>) -> rusqlite::Result<ChunkingProfile> {
    Ok(ChunkingProfile {
        id: row.get("id")?,
        name: row.get("name")?,
        prompt: row.get("prompt")?,
        overlap_ratio: row.get("overlap_ratio")?,
        max_signature_len: row.get("max_signature_len")?,
        metadata_fields: row.get("metadata_fields")?,
        match_strategy: row.get("match_strategy")?,
        fuzzy_threshold: row.get("fuzzy_threshold")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

impl Database {
    pub fn create_chunking_profile(&self, p: &NewChunkingProfile) -> Result<ChunkingProfile> {
        self.conn.execute(
            "INSERT INTO chunking_profiles
                (name, prompt, overlap_ratio, max_signature_len,
                 metadata_fields, match_strategy, fuzzy_threshold)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                p.name,
                p.prompt,
                p.overlap_ratio,
                p.max_signature_len,
                p.metadata_fields,
                p.match_strategy,
                p.fuzzy_threshold,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self
            .chunking_profile(id)?
            .expect("row just inserted must exist"))
    }

    pub fn chunking_profile(&self, id: i64) -> Result<Option<ChunkingProfile>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM chunking_profiles WHERE id = ?1",
                [id],
                row_to_profile,
            )
            .optional()?)
    }

    pub fn list_chunking_profiles(&self) -> Result<Vec<ChunkingProfile>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM chunking_profiles ORDER BY name")?;
        let rows = stmt.query_map([], row_to_profile)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn update_chunking_profile(
        &self,
        id: i64,
        p: &NewChunkingProfile,
    ) -> Result<ChunkingProfile> {
        self.conn.execute(
            "UPDATE chunking_profiles SET
                name = ?2, prompt = ?3, overlap_ratio = ?4,
                max_signature_len = ?5, metadata_fields = ?6,
                match_strategy = ?7, fuzzy_threshold = ?8, updated_at = unixepoch()
             WHERE id = ?1",
            params![
                id,
                p.name,
                p.prompt,
                p.overlap_ratio,
                p.max_signature_len,
                p.metadata_fields,
                p.match_strategy,
                p.fuzzy_threshold,
            ],
        )?;
        self.chunking_profile(id)?
            .ok_or_else(|| super::CoreError::NotFound(format!("chunking_profile {id}")))
    }

    pub fn delete_chunking_profile(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM chunking_profiles WHERE id = ?1", [id])?
            > 0)
    }
}
