use rusqlite::{OptionalExtension, Row};
use super::models::{NewStructuralProfile, StructuralPattern, StructuralProfile};
use super::{Database, Result};

fn row_to_structural_pattern(row: &Row<'_>) -> rusqlite::Result<StructuralPattern> {
    Ok(StructuralPattern {
        id: row.get("id")?,
        profile_id: row.get("profile_id")?,
        group_name: row.get("group_name")?,
        role: row.get("role")?,
        regex: row.get("regex")?,
        flags: row.get("flags")?,
        priority: row.get("priority")?,
        label: row.get("label")?,
        sort_order: row.get("sort_order")?,
    })
}

impl Database {
    pub fn create_structural_profile(&self, p: &NewStructuralProfile) -> Result<StructuralProfile> {
        self.conn.execute(
            "INSERT INTO structural_profiles
                (name, min_chunk_chars, max_chunk_chars, definition_triggers, heading_triggers, ignore_patterns, target_chunk_size)
             VALUES (?1, ?2, ?3, '', '', '', 1500)",
            rusqlite::params![
                p.name,
                p.min_chunk_chars,
                p.max_chunk_chars,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        
        for pat in &p.patterns {
            self.conn.execute(
                "INSERT INTO structural_patterns (profile_id, group_name, role, regex, flags, priority, label, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![id, pat.group_name, pat.role, pat.regex, pat.flags, pat.priority, pat.label, pat.sort_order]
            )?;
        }

        Ok(self.structural_profile(id)?.expect("row just inserted must exist"))
    }

    pub fn structural_profile(&self, id: i64) -> Result<Option<StructuralProfile>> {
        let profile = self.conn.query_row("SELECT * FROM structural_profiles WHERE id = ?1", [id], |row| {
            Ok(StructuralProfile {
                id: row.get("id")?,
                name: row.get("name")?,
                min_chunk_chars: row.get("min_chunk_chars")?,
                max_chunk_chars: row.get("max_chunk_chars")?,
                patterns: Vec::new(),
                created_at: row.get("created_at")?,
                updated_at: row.get("updated_at")?,
            })
        }).optional()?;
        
        if let Some(mut p) = profile {
            let mut stmt = self.conn.prepare("SELECT * FROM structural_patterns WHERE profile_id = ?1 ORDER BY sort_order, priority DESC")?;
            p.patterns = stmt.query_map([id], row_to_structural_pattern)?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(Some(p))
        } else {
            Ok(None)
        }
    }

    pub fn list_structural_profiles(&self) -> Result<Vec<StructuralProfile>> {
        let mut stmt = self.conn.prepare("SELECT * FROM structural_profiles ORDER BY name")?;
        let mut profiles: Vec<StructuralProfile> = stmt.query_map([], |row| {
            Ok(StructuralProfile {
                id: row.get("id")?,
                name: row.get("name")?,
                min_chunk_chars: row.get("min_chunk_chars")?,
                max_chunk_chars: row.get("max_chunk_chars")?,
                patterns: Vec::new(),
                created_at: row.get("created_at")?,
                updated_at: row.get("updated_at")?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        
        let mut stmt = self.conn.prepare("SELECT * FROM structural_patterns ORDER BY sort_order, priority DESC")?;
        let patterns: Vec<StructuralPattern> = stmt.query_map([], row_to_structural_pattern)?.collect::<rusqlite::Result<Vec<_>>>()?;
        
        for p in &mut profiles {
            p.patterns = patterns.iter().filter(|pat| pat.profile_id == p.id).cloned().collect();
        }
        
        Ok(profiles)
    }

    pub fn update_structural_profile(&self, id: i64, p: &NewStructuralProfile) -> Result<StructuralProfile> {
        self.conn.execute(
            "UPDATE structural_profiles SET
                name = ?2, min_chunk_chars = ?3, max_chunk_chars = ?4, updated_at = unixepoch()
             WHERE id = ?1",
            rusqlite::params![
                id,
                p.name,
                p.min_chunk_chars,
                p.max_chunk_chars,
            ],
        )?;
        
        self.conn.execute("DELETE FROM structural_patterns WHERE profile_id = ?1", [id])?;
        for pat in &p.patterns {
            self.conn.execute(
                "INSERT INTO structural_patterns (profile_id, group_name, role, regex, flags, priority, label, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![id, pat.group_name, pat.role, pat.regex, pat.flags, pat.priority, pat.label, pat.sort_order]
            )?;
        }

        self.structural_profile(id)?
            .ok_or_else(|| super::CoreError::NotFound(format!("structural_profile {id}")))
    }

    pub fn delete_structural_profile(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM structural_profiles WHERE id = ?1", [id])?
            > 0)
    }
}
