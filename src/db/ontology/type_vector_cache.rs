//! CRUD for `ontology_type_vector_cache` — a persistent raw-type -> vector
//! cache shared by `materialize_lens` and `schema_suggest.rs` (see
//! `schema_v35.sql`, BACKLOG.md "Rohtyp-Embeddings ... batchen + cachen").
//! Keyed per `embedding_model_id` since vectors are model-specific.
use crate::db::{Database, Result};
use crate::db::embeddings::{blob_to_vector, vector_to_blob};
use std::collections::HashMap;

impl Database {
    /// Looks up cached vectors for the given `raw_types` under one embedding
    /// model, in a single query. Types with no cache entry are simply absent
    /// from the returned map (caller embeds those and calls
    /// `upsert_type_vector` to fill the gap).
    pub fn get_type_vectors(&self, embedding_model_id: i64, raw_types: &[String]) -> Result<HashMap<String, Vec<f32>>> {
        if raw_types.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = raw_types.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT raw_type, vector FROM ontology_type_vector_cache
             WHERE embedding_model_id = ? AND raw_type IN ({placeholders})"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(raw_types.len() + 1);
        params.push(&embedding_model_id);
        for t in raw_types {
            params.push(t);
        }
        let rows = stmt.query_map(params.as_slice(), |row| {
            let raw_type: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((raw_type, blob))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;

        let mut out = HashMap::with_capacity(rows.len());
        for (raw_type, blob) in rows {
            if let Ok(vec) = blob_to_vector(&blob) {
                out.insert(raw_type, vec);
            }
        }
        Ok(out)
    }

    /// Upserts one raw-type vector for the given embedding model.
    pub fn upsert_type_vector(&self, embedding_model_id: i64, raw_type: &str, vector: &[f32]) -> Result<()> {
        let blob = vector_to_blob(vector);
        self.conn.execute(
            "INSERT OR REPLACE INTO ontology_type_vector_cache (embedding_model_id, raw_type, vector)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![embedding_model_id, raw_type, blob],
        )?;
        Ok(())
    }
}
