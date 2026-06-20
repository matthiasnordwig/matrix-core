use super::{Database, Result};
use crate::db::models::{OntologyProfile, NewOntologyProfile, OntologyNode, NewOntologyNode, OntologyEdge, NewOntologyEdge};
use rusqlite::OptionalExtension;

impl Database {
    pub fn list_ontology_profiles(&self) -> Result<Vec<OntologyProfile>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, entity_types_json, relation_types_json, created_at, updated_at 
             FROM ontology_profiles ORDER BY name"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(OntologyProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                entity_types_json: row.get(2)?,
                relation_types_json: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn ontology_profile(&self, id: i64) -> Result<Option<OntologyProfile>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, entity_types_json, relation_types_json, created_at, updated_at 
             FROM ontology_profiles WHERE id = ?1"
        )?;
        let profile = stmt.query_row([id], |row| {
            Ok(OntologyProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                entity_types_json: row.get(2)?,
                relation_types_json: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        }).optional()?;
        Ok(profile)
    }

    pub fn create_ontology_profile(&self, new: &NewOntologyProfile) -> Result<OntologyProfile> {
        self.conn.execute(
            "INSERT INTO ontology_profiles (name, entity_types_json, relation_types_json) 
             VALUES (?1, ?2, ?3)",
            rusqlite::params![new.name, new.entity_types_json, new.relation_types_json],
        )?;
        let id = self.conn.last_insert_rowid();
        self.ontology_profile(id).map(|p| p.unwrap())
    }

    pub fn update_ontology_profile(&self, id: i64, new: &NewOntologyProfile) -> Result<OntologyProfile> {
        self.conn.execute(
            "UPDATE ontology_profiles 
             SET name = ?1, entity_types_json = ?2, relation_types_json = ?3, updated_at = strftime('%s', 'now') 
             WHERE id = ?4",
            rusqlite::params![new.name, new.entity_types_json, new.relation_types_json, id],
        )?;
        self.ontology_profile(id).map(|p| p.unwrap())
    }

    pub fn delete_ontology_profile(&self, id: i64) -> Result<bool> {
        let rows = self.conn.execute("DELETE FROM ontology_profiles WHERE id = ?1", [id])?;
        Ok(rows > 0)
    }

    // --- Nodes & Edges ---

    pub fn list_ontology_nodes(&self, context_id: i64) -> Result<Vec<OntologyNode>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, context_id, label, entity_type, description, created_at 
             FROM ontology_nodes WHERE context_id = ?1"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            Ok(OntologyNode {
                id: row.get(0)?,
                context_id: row.get(1)?,
                label: row.get(2)?,
                entity_type: row.get(3)?,
                description: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn list_ontology_edges(&self, context_id: i64) -> Result<Vec<OntologyEdge>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, context_id, source_id, target_id, relation_type, chunk_id, created_at 
             FROM ontology_edges WHERE context_id = ?1"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            Ok(OntologyEdge {
                id: row.get(0)?,
                context_id: row.get(1)?,
                source_id: row.get(2)?,
                target_id: row.get(3)?,
                relation_type: row.get(4)?,
                chunk_id: row.get(5)?,
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
            "SELECT id, context_id, label, entity_type, description, created_at 
             FROM ontology_nodes WHERE id = ?1"
        )?;
        let node = stmt.query_row([id], |row| {
            Ok(OntologyNode {
                id: row.get(0)?,
                context_id: row.get(1)?,
                label: row.get(2)?,
                entity_type: row.get(3)?,
                description: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        Ok(node)
    }

    pub fn create_ontology_edge(&self, new: &NewOntologyEdge) -> Result<OntologyEdge> {
        self.conn.execute(
            "INSERT INTO ontology_edges (context_id, source_id, target_id, relation_type, chunk_id) 
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![new.context_id, new.source_id, new.target_id, new.relation_type, new.chunk_id],
        )?;
        let id = self.conn.last_insert_rowid();
        let mut stmt = self.conn.prepare(
            "SELECT id, context_id, source_id, target_id, relation_type, chunk_id, created_at 
             FROM ontology_edges WHERE id = ?1"
        )?;
        let edge = stmt.query_row([id], |row| {
            Ok(OntologyEdge {
                id: row.get(0)?,
                context_id: row.get(1)?,
                source_id: row.get(2)?,
                target_id: row.get(3)?,
                relation_type: row.get(4)?,
                chunk_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        Ok(edge)
    }
    
    pub fn update_ontology_node_vector(&self, node_id: i64, vector_blob: &[u8]) -> Result<()> {
        self.conn.execute(
            "UPDATE ontology_nodes SET vector_blob = ?1 WHERE id = ?2",
            rusqlite::params![vector_blob, node_id],
        )?;
        Ok(())
    }
    
    pub fn delete_ontology_for_context(&self, context_id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM ontology_edges WHERE context_id = ?1", [context_id])?;
        tx.execute("DELETE FROM ontology_nodes WHERE context_id = ?1", [context_id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_chunks_with_ontology(&self, context_id: i64) -> Result<std::collections::HashSet<i64>> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT chunk_id FROM ontology_edges WHERE context_id = ?1")?;
        let iter = stmt.query_map([context_id], |row| row.get::<_, i64>(0))?;
        let mut set = std::collections::HashSet::new();
        for item in iter {
            if let Ok(id) = item {
                set.insert(id);
            }
        }
        Ok(set)
    }

    pub fn get_ontology_nodes_missing_embeddings(&self, context_id: i64) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, description FROM ontology_nodes WHERE context_id = ?1 AND vector_blob IS NULL")?;
        let iter = stmt.query_map([context_id], |row| {
            let id: i64 = row.get(0)?;
            let desc: String = row.get(1)?;
            Ok((id, desc))
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
            tx.execute("UPDATE ontology_edges SET source_id = ?1 WHERE source_id = ?2", [keep_id, drop_id])?;
            tx.execute("UPDATE ontology_edges SET target_id = ?1 WHERE target_id = ?2", [keep_id, drop_id])?;
            tx.execute("DELETE FROM ontology_nodes WHERE id = ?1", [drop_id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn retrieve_graph_with(
        &self,
        context_ids: &[i64],
        query_by_model: &std::collections::HashMap<i64, Vec<f32>>,
        top_k_nodes: usize,
        hops: usize,
    ) -> Result<crate::db::models::GraphContext> {
        let mut hit_node_ids = Vec::new();
        // 1. Find top K nodes across contexts
        for (model_id, ctxs) in self.contexts_by_model(context_ids)? {
            let Some(qvec) = query_by_model.get(&model_id) else { continue; };
            for cid in ctxs {
                // Brute force cosine on ontology_nodes
                let mut stmt = self.conn.prepare("SELECT id, vector_blob FROM ontology_nodes WHERE context_id = ?1 AND vector_blob IS NOT NULL")?;
                let iter = stmt.query_map([cid], |row| {
                    let id: i64 = row.get(0)?;
                    let blob: Vec<u8> = row.get(1)?;
                    let vec = crate::db::embeddings::blob_to_vector(&blob).unwrap_or_default();
                    Ok((id, vec))
                })?;
                
                let mut scored = Vec::new();
                for item in iter {
                    if let Ok((id, vec)) = item {
                        if vec.len() == qvec.len() && !vec.is_empty() {
                            let score = crate::db::embeddings::cosine(qvec, &vec);
                            scored.push((score, id));
                        }
                    }
                }
                scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                scored.truncate(top_k_nodes);
                hit_node_ids.extend(scored.into_iter().map(|s| s.1));
            }
        }
        
        // 2. Recursive CTE to get hops
        if hit_node_ids.is_empty() {
            return Ok(crate::db::models::GraphContext { nodes: vec![], edges: vec![] });
        }
        
        // Construct CTE query
        let in_clause = hit_node_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        
        let query = format!("
            WITH RECURSIVE traverse(node_id, depth) AS (
                SELECT id, 0 FROM ontology_nodes WHERE id IN ({})
                UNION
                SELECT e.target_id, t.depth + 1
                FROM traverse t
                JOIN ontology_edges e ON e.source_id = t.node_id
                WHERE t.depth < ?1
                UNION
                SELECT e.source_id, t.depth + 1
                FROM traverse t
                JOIN ontology_edges e ON e.target_id = t.node_id
                WHERE t.depth < ?1
            )
            SELECT DISTINCT node_id FROM traverse;
        ", in_clause);
        
        let mut stmt = self.conn.prepare(&query)?;
        let expanded_node_ids: Vec<i64> = stmt.query_map([hops as i64], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
            
        if expanded_node_ids.is_empty() {
            return Ok(crate::db::models::GraphContext { nodes: vec![], edges: vec![] });
        }
        
        let exp_in = expanded_node_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        
        let mut n_stmt = self.conn.prepare(&format!("SELECT label, description FROM ontology_nodes WHERE id IN ({})", exp_in))?;
        let nodes = n_stmt.query_map([], |row| {
            let label: String = row.get(0)?;
            let desc: String = row.get(1)?;
            Ok(format!("{}: {}", label, desc))
        })?.filter_map(|r| r.ok()).collect();
        
        let mut e_stmt = self.conn.prepare(&format!("
            SELECT s.label, e.relation_type, t.label 
            FROM ontology_edges e
            JOIN ontology_nodes s ON e.source_id = s.id
            JOIN ontology_nodes t ON e.target_id = t.id
            WHERE e.source_id IN ({exp_in}) AND e.target_id IN ({exp_in})
        "))?;
        let edges = e_stmt.query_map([], |row| {
            let src: String = row.get(0)?;
            let rel: String = row.get(1)?;
            let tgt: String = row.get(2)?;
            Ok(format!("{} -[{}]-> {}", src, rel, tgt))
        })?.filter_map(|r| r.ok()).collect();
        
        Ok(crate::db::models::GraphContext { nodes, edges })
    }
}
