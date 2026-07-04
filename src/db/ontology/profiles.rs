//! CRUD for `ontology_profiles` (the entity/relation-type schema used to
//! drive GraphRAG extraction). Split out of the former monolithic
//! `db/ontology.rs` — see HANDBUCH.md.
use crate::db::{Database, Result};
use crate::db::models::{OntologyProfile, NewOntologyProfile};
use rusqlite::OptionalExtension;

impl Database {
    pub fn list_ontology_profiles(&self) -> Result<Vec<OntologyProfile>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, entity_types_json, relation_types_json, extract_prompt, dedup_prompt, community_prompt, created_at, updated_at
             FROM ontology_profiles ORDER BY name"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(OntologyProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                entity_types_json: row.get(2)?,
                relation_types_json: row.get(3)?,
                extract_prompt: row.get(4)?,
                dedup_prompt: row.get(5)?,
                community_prompt: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn ontology_profile(&self, id: i64) -> Result<Option<OntologyProfile>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, entity_types_json, relation_types_json, extract_prompt, dedup_prompt, community_prompt, created_at, updated_at
             FROM ontology_profiles WHERE id = ?1"
        )?;
        let profile = stmt.query_row([id], |row| {
            Ok(OntologyProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                entity_types_json: row.get(2)?,
                relation_types_json: row.get(3)?,
                extract_prompt: row.get(4)?,
                dedup_prompt: row.get(5)?,
                community_prompt: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        }).optional()?;
        Ok(profile)
    }

    pub fn create_ontology_profile(&self, new: &NewOntologyProfile) -> Result<OntologyProfile> {
        self.conn.execute(
            "INSERT INTO ontology_profiles (name, entity_types_json, relation_types_json, extract_prompt, dedup_prompt, community_prompt)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![new.name, new.entity_types_json, new.relation_types_json, new.extract_prompt, new.dedup_prompt, new.community_prompt],
        )?;
        let id = self.conn.last_insert_rowid();
        self.ontology_profile(id).map(|p| p.unwrap())
    }

    pub fn update_ontology_profile(&self, id: i64, new: &NewOntologyProfile) -> Result<OntologyProfile> {
        self.conn.execute(
            "UPDATE ontology_profiles
             SET name = ?1, entity_types_json = ?2, relation_types_json = ?3, extract_prompt = ?4, dedup_prompt = ?5, community_prompt = ?6, updated_at = strftime('%s', 'now')
             WHERE id = ?7",
            rusqlite::params![new.name, new.entity_types_json, new.relation_types_json, new.extract_prompt, new.dedup_prompt, new.community_prompt, id],
        )?;
        self.ontology_profile(id).map(|p| p.unwrap())
    }

    pub fn delete_ontology_profile(&self, id: i64) -> Result<bool> {
        let rows = self.conn.execute("DELETE FROM ontology_profiles WHERE id = ?1", [id])?;
        Ok(rows > 0)
    }
}
