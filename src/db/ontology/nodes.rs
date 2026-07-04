//! CRUD + embedding/merge logic for `ontology_nodes`. Split out of the former
//! monolithic `db/ontology.rs` — see HANDBUCH.md. Manual-curation node
//! methods (insert/update/delete/context lookup/semantic search) were
//! originally grouped under an "Admin / Manual Curation" comment at the
//! bottom of that file; they're grouped here by entity instead.
use crate::db::{Database, Result};
use crate::db::models::{OntologyNode, NewOntologyNode};

impl Database {
    pub fn list_ontology_nodes(&self, context_id: i64) -> Result<Vec<OntologyNode>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, context_id, label, entity_type, description, community_id, created_at
             FROM ontology_nodes WHERE context_id = ?1"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            Ok(OntologyNode {
                id: row.get(0)?,
                context_id: row.get(1)?,
                label: row.get(2)?,
                entity_type: row.get(3)?,
                description: row.get(4)?,
                community_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn create_ontology_node(&self, new: &NewOntologyNode) -> Result<OntologyNode> {
        self.conn.execute(
            "INSERT INTO ontology_nodes (context_id, label, entity_type, description)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![new.context_id, new.label, new.entity_type, new.description],
        )?;
        let id = self.conn.last_insert_rowid();
        let mut stmt = self.conn.prepare(
            "SELECT id, context_id, label, entity_type, description, community_id, created_at
             FROM ontology_nodes WHERE id = ?1"
        )?;
        let node = stmt.query_row([id], |row| {
            Ok(OntologyNode {
                id: row.get(0)?,
                context_id: row.get(1)?,
                label: row.get(2)?,
                entity_type: row.get(3)?,
                description: row.get(4)?,
                community_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        Ok(node)
    }

    pub fn insert_ontology_node_fast(&self, new: &NewOntologyNode) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO ontology_nodes (context_id, label, entity_type, description) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![new.context_id, new.label, new.entity_type, new.description],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_ontology_node_id_by_label_fast(&self, context_id: i64, label: &str) -> Result<Option<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM ontology_nodes WHERE context_id = ?1 AND LOWER(label) = LOWER(?2) LIMIT 1"
        )?;
        let mut rows = stmt.query(rusqlite::params![context_id, label])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn update_ontology_node_vector(&self, node_id: i64, vector_blob: &[u8]) -> Result<()> {
        self.conn.execute(
            "UPDATE ontology_nodes SET vector_blob = ?1 WHERE id = ?2",
            rusqlite::params![vector_blob, node_id],
        )?;
        Ok(())
    }

    pub fn update_ontology_node_type(&self, node_id: i64, new_type: &str) -> Result<()> {
        self.conn.execute("UPDATE ontology_nodes SET entity_type = ?1 WHERE id = ?2", rusqlite::params![new_type, node_id])?;
        Ok(())
    }

    pub fn update_ontology_node_community(&self, node_id: i64, community_id: Option<i64>) -> Result<()> {
        self.conn.execute("UPDATE ontology_nodes SET community_id = ?1 WHERE id = ?2", rusqlite::params![community_id, node_id])?;
        Ok(())
    }

    pub fn get_chunks_with_ontology(&self, context_id: i64) -> Result<std::collections::HashSet<i64>> {
        let mut stmt = self.conn.prepare("SELECT chunk_id FROM ontology_extracted_chunks WHERE context_id = ?1")?;
        let iter = stmt.query_map([context_id], |row| row.get::<_, i64>(0))?;
        let mut set = std::collections::HashSet::new();
        for item in iter {
            if let Ok(id) = item {
                set.insert(id);
            }
        }
        Ok(set)
    }

    pub fn insert_extracted_chunk(&self, context_id: i64, chunk_id: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO ontology_extracted_chunks (context_id, chunk_id) VALUES (?1, ?2)",
            [context_id, chunk_id]
        )?;
        Ok(())
    }

    pub fn get_ontology_nodes(&self, context_id: i64) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, entity_type FROM ontology_nodes WHERE context_id = ?1")?;
        let iter = stmt.query_map([context_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut list = Vec::new();
        for item in iter { if let Ok(i) = item { list.push(i); } }
        Ok(list)
    }

    pub fn get_ontology_nodes_with_descriptions(&self, context_id: i64) -> Result<std::collections::HashMap<i64, (String, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, label, description FROM ontology_nodes WHERE context_id = ?1")?;
        let iter = stmt.query_map([context_id], |row| {
            let id: i64 = row.get(0)?;
            let label: String = row.get(1)?;
            let desc: String = row.get(2)?;
            Ok((id, (label, desc)))
        })?;

        let mut map = std::collections::HashMap::new();
        for item in iter {
            if let Ok((id, val)) = item { map.insert(id, val); }
        }
        Ok(map)
    }

    pub fn get_ontology_nodes_missing_embeddings(&self, context_id: i64) -> Result<Vec<(i64, String, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, label, description FROM ontology_nodes WHERE context_id = ?1 AND vector_blob IS NULL")?;
        let iter = stmt.query_map([context_id], |row| {
            let id: i64 = row.get(0)?;
            let label: String = row.get(1)?;
            let desc: String = row.get(2)?;
            Ok((id, label, desc))
        })?;

        let mut list = Vec::new();
        for item in iter {
            if let Ok(i) = item { list.push(i); }
        }
        Ok(list)
    }

    pub fn get_ontology_nodes_with_embeddings(&self, context_id: i64) -> Result<Vec<(i64, String, String, String, Vec<f32>)>> {
        let mut stmt = self.conn.prepare("SELECT id, entity_type, label, description, vector_blob FROM ontology_nodes WHERE context_id = ?1 AND vector_blob IS NOT NULL")?;
        let iter = stmt.query_map([context_id], |row| {
            let blob: Vec<u8> = row.get(4)?;
            let vec = crate::db::embeddings::blob_to_vector(&blob).unwrap_or_default();
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                vec
            ))
        })?;

        let mut list = Vec::new();
        for item in iter {
            if let Ok(i) = item { list.push(i); }
        }
        Ok(list)
    }

    pub fn merge_ontology_nodes(&self, merges: &[(i64, i64)]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for &(keep_id, drop_id) in merges {
            tx.execute("
                INSERT OR IGNORE INTO ontology_edge_chunks (edge_id, chunk_id)
                SELECT keep_edge.id, drop_chunk.chunk_id
                FROM ontology_edges drop_edge
                JOIN ontology_edges keep_edge
                  ON drop_edge.target_id = keep_edge.target_id
                 AND LOWER(drop_edge.relation_type) = LOWER(keep_edge.relation_type)
                 AND keep_edge.source_id = ?1
                JOIN ontology_edge_chunks drop_chunk ON drop_chunk.edge_id = drop_edge.id
                WHERE drop_edge.source_id = ?2
            ", rusqlite::params![keep_id, drop_id])?;

            tx.execute("
                DELETE FROM ontology_edges
                WHERE id IN (
                    SELECT drop_edge.id
                    FROM ontology_edges drop_edge
                    JOIN ontology_edges keep_edge
                      ON drop_edge.target_id = keep_edge.target_id
                     AND LOWER(drop_edge.relation_type) = LOWER(keep_edge.relation_type)
                     AND keep_edge.source_id = ?1
                    WHERE drop_edge.source_id = ?2
                )
            ", rusqlite::params![keep_id, drop_id])?;

            tx.execute("UPDATE OR IGNORE ontology_edges SET source_id = ?1 WHERE source_id = ?2", rusqlite::params![keep_id, drop_id])?;

            tx.execute("
                INSERT OR IGNORE INTO ontology_edge_chunks (edge_id, chunk_id)
                SELECT keep_edge.id, drop_chunk.chunk_id
                FROM ontology_edges drop_edge
                JOIN ontology_edges keep_edge
                  ON drop_edge.source_id = keep_edge.source_id
                 AND LOWER(drop_edge.relation_type) = LOWER(keep_edge.relation_type)
                 AND keep_edge.target_id = ?1
                JOIN ontology_edge_chunks drop_chunk ON drop_chunk.edge_id = drop_edge.id
                WHERE drop_edge.target_id = ?2
            ", rusqlite::params![keep_id, drop_id])?;

            tx.execute("
                DELETE FROM ontology_edges
                WHERE id IN (
                    SELECT drop_edge.id
                    FROM ontology_edges drop_edge
                    JOIN ontology_edges keep_edge
                      ON drop_edge.source_id = keep_edge.source_id
                     AND LOWER(drop_edge.relation_type) = LOWER(keep_edge.relation_type)
                     AND keep_edge.target_id = ?1
                    WHERE drop_edge.target_id = ?2
                )
            ", rusqlite::params![keep_id, drop_id])?;

            tx.execute("UPDATE OR IGNORE ontology_edges SET target_id = ?1 WHERE target_id = ?2", rusqlite::params![keep_id, drop_id])?;

            tx.execute("DELETE FROM ontology_nodes WHERE id = ?1", rusqlite::params![drop_id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn insert_ontology_node(&self, context_id: i64, label: &str, entity_type: &str, description: &str, vector_blob: &[u8]) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_nodes (context_id, label, entity_type, description, vector_blob) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![context_id, label, entity_type, description, vector_blob],
        )?;
        Ok(())
    }

    pub fn update_ontology_node(&self, id: i64, label: &str, entity_type: &str, description: &str, vector_blob: &[u8]) -> Result<()> {
        self.conn.execute(
            "UPDATE ontology_nodes SET label = ?1, entity_type = ?2, description = ?3, vector_blob = ?4 WHERE id = ?5",
            rusqlite::params![label, entity_type, description, vector_blob, id],
        )?;
        Ok(())
    }

    pub fn delete_ontology_node(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM ontology_nodes WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn context_id_for_ontology_node(&self, id: i64) -> Result<i64> {
        Ok(self.conn.query_row("SELECT context_id FROM ontology_nodes WHERE id = ?1", [id], |row| row.get(0))?)
    }

    pub fn search_ontology_nodes_semantic(&self, context_id: i64, query_vec: &[f32], limit: usize) -> Result<Vec<(i64, f32)>> {
        let mut stmt = self.conn.prepare("SELECT id, vector_blob FROM ontology_nodes WHERE context_id = ?1 AND vector_blob IS NOT NULL")?;
        let rows = stmt.query_map([context_id], |row| {
            let id: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            let vec = crate::db::embeddings::blob_to_vector(&blob).unwrap_or_default();
            Ok((id, vec))
        })?;

        let mut results = Vec::new();
        for r in rows {
            let (id, v) = r?;
            let score = crate::db::embeddings::cosine(query_vec, &v);
            results.push((id, score));
        }

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        Ok(results)
    }
}
