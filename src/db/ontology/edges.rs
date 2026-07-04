//! CRUD for `ontology_edges` (+ their `ontology_edge_chunks` evidence rows).
//! Split out of the former monolithic `db/ontology.rs` — see HANDBUCH.md.
//! Manual-curation edge methods (invert/update/insert-primitive/add-chunk/
//! remove-chunk) were originally grouped under an "Admin / Manual Curation"
//! comment at the bottom of that file; they're grouped here by entity
//! instead.
use crate::db::{Database, Result};
use crate::db::models::{OntologyEdge, NewOntologyEdge};

impl Database {
    pub fn list_ontology_edges(&self, context_id: i64) -> Result<Vec<OntologyEdge>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.id, e.context_id, e.source_id, e.target_id, e.relation_type, json_group_array(json_object('chunk_id', c.chunk_id, 'evidence', c.evidence)) as chunk_data, e.created_at
             FROM ontology_edges e
             LEFT JOIN ontology_edge_chunks c ON e.id = c.edge_id
             WHERE e.context_id = ?1
             GROUP BY e.id"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            let chunk_data_str: Option<String> = row.get(5)?;
            let mut chunk_evidences = std::collections::HashMap::new();
            if let Some(s) = chunk_data_str {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&s) {
                    if let Some(arr) = parsed.as_array() {
                        for item in arr {
                            if let Some(obj) = item.as_object() {
                                if let Some(cid_val) = obj.get("chunk_id") {
                                    if let Some(cid) = cid_val.as_i64() {
                                        let ev = obj.get("evidence").and_then(|v| v.as_str()).map(|s| s.to_string());
                                        chunk_evidences.insert(cid, ev);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(OntologyEdge {
                id: row.get(0)?,
                context_id: row.get(1)?,
                source_id: row.get(2)?,
                target_id: row.get(3)?,
                relation_type: row.get(4)?,
                chunk_evidences,
                created_at: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn create_ontology_edge(&self, new: &NewOntologyEdge) -> Result<OntologyEdge> {
        self.conn.execute(
            "INSERT OR IGNORE INTO ontology_edges (context_id, source_id, target_id, relation_type)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![new.context_id, new.source_id, new.target_id, new.relation_type],
        )?;
        let edge_id: i64 = self.conn.query_row(
            "SELECT id FROM ontology_edges WHERE context_id = ?1 AND source_id = ?2 AND target_id = ?3 AND LOWER(relation_type) = LOWER(?4)",
            rusqlite::params![new.context_id, new.source_id, new.target_id, new.relation_type],
            |row| row.get(0)
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO ontology_edge_chunks (edge_id, chunk_id) VALUES (?1, ?2)",
            rusqlite::params![edge_id, new.chunk_id],
        )?;
        let mut stmt = self.conn.prepare(
            "SELECT e.id, e.context_id, e.source_id, e.target_id, e.relation_type, json_group_array(json_object('chunk_id', c.chunk_id, 'evidence', c.evidence)) as chunk_data, e.created_at
             FROM ontology_edges e
             LEFT JOIN ontology_edge_chunks c ON e.id = c.edge_id
             WHERE e.id = ?1
             GROUP BY e.id"
        )?;
        let edge = stmt.query_row([edge_id], |row| {
            let chunk_data_str: Option<String> = row.get(5)?;
            let mut chunk_evidences = std::collections::HashMap::new();
            if let Some(s) = chunk_data_str {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&s) {
                    if let Some(arr) = parsed.as_array() {
                        for item in arr {
                            if let Some(obj) = item.as_object() {
                                if let Some(cid_val) = obj.get("chunk_id") {
                                    if let Some(cid) = cid_val.as_i64() {
                                        let ev = obj.get("evidence").and_then(|v| v.as_str()).map(|s| s.to_string());
                                        chunk_evidences.insert(cid, ev);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(OntologyEdge {
                id: row.get(0)?,
                context_id: row.get(1)?,
                source_id: row.get(2)?,
                target_id: row.get(3)?,
                relation_type: row.get(4)?,
                chunk_evidences,
                created_at: row.get(6)?,
            })
        })?;
        Ok(edge)
    }

    pub fn insert_ontology_edge_fast(&self, new: &NewOntologyEdge) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO ontology_edges (context_id, source_id, target_id, relation_type) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![new.context_id, new.source_id, new.target_id, new.relation_type],
        )?;
        let edge_id: i64 = self.conn.query_row(
            "SELECT id FROM ontology_edges WHERE context_id = ?1 AND source_id = ?2 AND target_id = ?3 AND LOWER(relation_type) = LOWER(?4)",
            rusqlite::params![new.context_id, new.source_id, new.target_id, new.relation_type],
            |row| row.get(0)
        )?;
        self.conn.execute(
            "INSERT INTO ontology_edge_chunks (edge_id, chunk_id, evidence) VALUES (?1, ?2, ?3) ON CONFLICT(edge_id, chunk_id) DO UPDATE SET evidence = COALESCE(excluded.evidence, ontology_edge_chunks.evidence)",
            rusqlite::params![edge_id, new.chunk_id, new.evidence],
        )?;
        Ok(())
    }

    pub fn update_ontology_edge_type(&self, edge_id: i64, new_type: &str) -> Result<()> {
        self.conn.execute("UPDATE ontology_edges SET relation_type = ?1 WHERE id = ?2", rusqlite::params![new_type, edge_id])?;
        Ok(())
    }

    pub fn get_ontology_edges(&self, context_id: i64) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, relation_type FROM ontology_edges WHERE context_id = ?1")?;
        let iter = stmt.query_map([context_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut list = Vec::new();
        for item in iter { if let Ok(i) = item { list.push(i); } }
        Ok(list)
    }

    pub fn get_ontology_edges_for_sanitization(&self, context_id: i64) -> Result<Vec<(i64, i64, i64, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, source_id, target_id, relation_type FROM ontology_edges WHERE context_id = ?1")?;
        let iter = stmt.query_map([context_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        let mut list = Vec::new();
        for item in iter { if let Ok(i) = item { list.push(i); } }
        Ok(list)
    }

    pub fn reverse_ontology_edge(&self, edge_id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let mut stmt = tx.prepare("SELECT source_id, target_id FROM ontology_edges WHERE id = ?1")?;
        let (source, target): (i64, i64) = stmt.query_row([edge_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        drop(stmt);
        tx.execute("UPDATE ontology_edges SET source_id = ?1, target_id = ?2 WHERE id = ?3", rusqlite::params![target, source, edge_id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_ontology_edge(&self, edge_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM ontology_edges WHERE id = ?1", rusqlite::params![edge_id])?;
        Ok(())
    }

    pub fn get_ontology_edges_full(&self, context_id: i64) -> Result<Vec<(i64, i64, String)>> {
        let mut stmt = self.conn.prepare("SELECT source_id, target_id, relation_type FROM ontology_edges WHERE context_id = ?1")?;
        let iter = stmt.query_map([context_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;

        let mut list = Vec::new();
        for item in iter {
            if let Ok(i) = item { list.push(i); }
        }
        Ok(list)
    }

    pub fn get_node_edge_counts(&self, context_id: i64) -> Result<std::collections::HashMap<i64, i64>> {
        let mut map = std::collections::HashMap::new();
        let mut stmt = self.conn.prepare("
            SELECT node_id, SUM(cnt) FROM (
                SELECT source_id as node_id, COUNT(*) as cnt FROM ontology_edges WHERE context_id = ?1 GROUP BY source_id
                UNION ALL
                SELECT target_id as node_id, COUNT(*) as cnt FROM ontology_edges WHERE context_id = ?1 GROUP BY target_id
            ) GROUP BY node_id
        ")?;
        let mut rows = stmt.query([context_id])?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let count: i64 = row.get(1)?;
            map.insert(id, count);
        }
        Ok(map)
    }

    pub fn invert_ontology_edge(&self, id: i64) -> Result<()> {
        let (source_id, target_id): (i64, i64) = self.conn.query_row(
            "SELECT source_id, target_id FROM ontology_edges WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        self.conn.execute(
            "UPDATE ontology_edges SET source_id = ?1, target_id = ?2 WHERE id = ?3",
            rusqlite::params![target_id, source_id, id],
        )?;
        Ok(())
    }

    pub fn update_ontology_edge(&self, id: i64, relation_type: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE ontology_edges SET relation_type = ?1 WHERE id = ?2",
            rusqlite::params![relation_type, id],
        )?;
        Ok(())
    }

    pub fn insert_ontology_edge_fast_primitive(&self, context_id: i64, source_id: i64, target_id: i64, relation_type: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_edges (context_id, source_id, target_id, relation_type) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![context_id, source_id, target_id, relation_type],
        )?;
        Ok(())
    }

    pub fn add_ontology_edge_chunk(&self, edge_id: i64, chunk_id: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO ontology_edge_chunks (edge_id, chunk_id) VALUES (?1, ?2)",
            rusqlite::params![edge_id, chunk_id],
        )?;
        Ok(())
    }

    pub fn remove_ontology_edge_chunk(&self, edge_id: i64, chunk_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM ontology_edge_chunks WHERE edge_id = ?1 AND chunk_id = ?2",
            rusqlite::params![edge_id, chunk_id],
        )?;
        Ok(())
    }
}
