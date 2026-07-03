use super::{Database, Result};
use crate::db::models::{OntologyProfile, NewOntologyProfile, OntologyNode, NewOntologyNode, OntologyEdge, NewOntologyEdge};
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

    // --- Nodes & Edges ---

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

    pub fn list_ontology_edges(&self, context_id: i64) -> Result<Vec<OntologyEdge>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.id, e.context_id, e.source_id, e.target_id, e.relation_type, GROUP_CONCAT(c.chunk_id) as chunk_ids, e.created_at 
             FROM ontology_edges e
             LEFT JOIN ontology_edge_chunks c ON e.id = c.edge_id
             WHERE e.context_id = ?1
             GROUP BY e.id"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            let chunk_ids_str: Option<String> = row.get(5)?;
            let mut chunk_ids = Vec::new();
            if let Some(s) = chunk_ids_str {
                for id_str in s.split(',') {
                    if let Ok(id) = id_str.parse::<i64>() {
                        chunk_ids.push(id);
                    }
                }
            }
            Ok(OntologyEdge {
                id: row.get(0)?,
                context_id: row.get(1)?,
                source_id: row.get(2)?,
                target_id: row.get(3)?,
                relation_type: row.get(4)?,
                chunk_ids,
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
            "SELECT e.id, e.context_id, e.source_id, e.target_id, e.relation_type, GROUP_CONCAT(c.chunk_id) as chunk_ids, e.created_at 
             FROM ontology_edges e
             LEFT JOIN ontology_edge_chunks c ON e.id = c.edge_id
             WHERE e.id = ?1
             GROUP BY e.id"
        )?;
        let edge = stmt.query_row([edge_id], |row| {
            let chunk_ids_str: Option<String> = row.get(5)?;
            let mut chunk_ids = Vec::new();
            if let Some(s) = chunk_ids_str {
                for id_str in s.split(',') {
                    if let Ok(id) = id_str.parse::<i64>() {
                        chunk_ids.push(id);
                    }
                }
            }
            Ok(OntologyEdge {
                id: row.get(0)?,
                context_id: row.get(1)?,
                source_id: row.get(2)?,
                target_id: row.get(3)?,
                relation_type: row.get(4)?,
                chunk_ids,
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
            "INSERT OR IGNORE INTO ontology_edge_chunks (edge_id, chunk_id) VALUES (?1, ?2)",
            rusqlite::params![edge_id, new.chunk_id],
        )?;
        Ok(())
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

    pub fn update_ontology_edge_type(&self, edge_id: i64, new_type: &str) -> Result<()> {
        self.conn.execute("UPDATE ontology_edges SET relation_type = ?1 WHERE id = ?2", rusqlite::params![new_type, edge_id])?;
        Ok(())
    }
    
    pub fn update_ontology_node_community(&self, node_id: i64, community_id: Option<i64>) -> Result<()> {
        self.conn.execute("UPDATE ontology_nodes SET community_id = ?1 WHERE id = ?2", rusqlite::params![community_id, node_id])?;
        Ok(())
    }

    pub fn create_ontology_community(&self, context_id: i64, community_label: &str, node_count: i64, summary_text: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO ontology_communities (context_id, community_label, node_count, summary_text) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![context_id, community_label, node_count, summary_text]
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_community_vector(&self, community_id: i64, vector_blob: &[u8]) -> Result<()> {
        self.conn.execute("UPDATE ontology_communities SET vector_blob = ?1 WHERE id = ?2", rusqlite::params![vector_blob, community_id])?;
        Ok(())
    }

    pub fn list_ontology_communities(&self, context_id: i64) -> Result<Vec<crate::db::models::OntologyCommunity>> {
        let mut stmt = self.conn.prepare("SELECT id, context_id, community_label, node_count, summary_text, created_at FROM ontology_communities WHERE context_id = ?1")?;
        let rows = stmt.query_map([context_id], |row| {
            Ok(crate::db::models::OntologyCommunity {
                id: row.get(0)?,
                context_id: row.get(1)?,
                community_label: row.get(2)?,
                node_count: row.get(3)?,
                summary_text: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    pub fn delete_communities_for_context(&self, context_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM ontology_communities WHERE context_id = ?1", [context_id])?;
        self.conn.execute("UPDATE ontology_nodes SET community_id = NULL WHERE context_id = ?1", [context_id])?;
        Ok(())
    }
    
    pub fn delete_ontology_for_context(&self, context_id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM ontology_edges WHERE context_id = ?1", [context_id])?;
        tx.execute("DELETE FROM ontology_nodes WHERE context_id = ?1", [context_id])?;
        tx.execute("DELETE FROM ontology_extracted_chunks WHERE context_id = ?1", [context_id])?;
        tx.commit()?;
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

    pub fn assign_communities(&self, assignments: &std::collections::HashMap<i64, i64>) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for (&node_id, &comm_id) in assignments {
            tx.execute("UPDATE ontology_nodes SET community_id = ?1 WHERE id = ?2", rusqlite::params![comm_id, node_id])?;
        }
        tx.commit()?;
        Ok(())
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

    pub fn retrieve_graph_with(
        &self,
        context_ids: &[i64],
        query_by_model: &std::collections::HashMap<i64, Vec<f32>>,
        top_k_nodes: usize,
        hops: usize,
        top_k_communities: usize,
    ) -> Result<crate::db::models::GraphContext> {
        let mut hit_node_ids = Vec::new();
        let mut hit_community_ids = Vec::new();
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
                
                // Brute force cosine on ontology_communities
                let mut stmt_comm = self.conn.prepare("SELECT id, vector_blob FROM ontology_communities WHERE context_id = ?1 AND vector_blob IS NOT NULL")?;
                let iter_comm = stmt_comm.query_map([cid], |row| {
                    let id: i64 = row.get(0)?;
                    let blob: Vec<u8> = row.get(1)?;
                    let vec = crate::db::embeddings::blob_to_vector(&blob).unwrap_or_default();
                    Ok((id, vec))
                })?;
                
                let mut scored_comms = Vec::new();
                for item in iter_comm {
                    if let Ok((id, vec)) = item {
                        if vec.len() == qvec.len() && !vec.is_empty() {
                            let score = crate::db::embeddings::cosine(qvec, &vec);
                            scored_comms.push((score, id));
                        }
                    }
                }
                scored_comms.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                scored_comms.truncate(top_k_communities);
                hit_community_ids.extend(scored_comms.into_iter().map(|s| s.1));
            }
        }
        
        let mut community_summaries = Vec::new();
        if !hit_community_ids.is_empty() {
            let placeholders = std::iter::repeat("?").take(hit_community_ids.len()).collect::<Vec<_>>().join(",");
            let query = format!("SELECT community_label, summary_text FROM ontology_communities WHERE id IN ({})", placeholders);
            let mut stmt = self.conn.prepare_cached(&query)?;
            let summaries = stmt.query_map(rusqlite::params_from_iter(hit_community_ids.iter()), |row| {
                let label: String = row.get(0)?;
                let text: String = row.get(1)?;
                Ok(format!("{}: {}", label, text))
            })?.filter_map(|r| r.ok()).collect::<Vec<_>>();
            community_summaries = summaries;
        }

        // 2. Recursive CTE to get hops
        if hit_node_ids.is_empty() {
            return Ok(crate::db::models::GraphContext { nodes: vec![], edges: vec![], community_summaries });
        }
        
        // Construct CTE query
        let in_placeholders = std::iter::repeat("?").take(hit_node_ids.len()).collect::<Vec<_>>().join(",");
        
        let query = format!("
            WITH RECURSIVE traverse(node_id, depth) AS (
                SELECT id, 0 FROM ontology_nodes WHERE id IN ({})
                UNION
                SELECT e.target_id, t.depth + 1
                FROM traverse t
                JOIN ontology_edges e ON e.source_id = t.node_id
                WHERE t.depth < ?
                UNION
                SELECT e.source_id, t.depth + 1
                FROM traverse t
                JOIN ontology_edges e ON e.target_id = t.node_id
                WHERE t.depth < ?
            )
            SELECT DISTINCT node_id FROM traverse;
        ", in_placeholders);
        
        let mut stmt = self.conn.prepare_cached(&query)?;
        
        let mut params: Vec<rusqlite::types::Value> = hit_node_ids.iter().map(|&id| rusqlite::types::Value::Integer(id)).collect();
        params.push(rusqlite::types::Value::Integer(hops as i64));
        params.push(rusqlite::types::Value::Integer(hops as i64));
        
        let expanded_node_ids: Vec<i64> = stmt.query_map(rusqlite::params_from_iter(params), |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
            
        if expanded_node_ids.is_empty() {
            return Ok(crate::db::models::GraphContext { nodes: vec![], edges: vec![], community_summaries });
        }
        
        let exp_placeholders = std::iter::repeat("?").take(expanded_node_ids.len()).collect::<Vec<_>>().join(",");
        
        let mut n_stmt = self.conn.prepare_cached(&format!("SELECT label, description FROM ontology_nodes WHERE id IN ({})", exp_placeholders))?;
        let nodes = n_stmt.query_map(rusqlite::params_from_iter(expanded_node_ids.iter()), |row| {
            let label: String = row.get(0)?;
            let desc: String = row.get(1)?;
            Ok(format!("{}: {}", label, desc))
        })?.filter_map(|r| r.ok()).collect();
        
        let mut e_stmt = self.conn.prepare_cached(&format!("
            SELECT s.label, e.relation_type, t.label 
            FROM ontology_edges e
            JOIN ontology_nodes s ON e.source_id = s.id
            JOIN ontology_nodes t ON e.target_id = t.id
            WHERE e.source_id IN ({exp_placeholders}) AND e.target_id IN ({exp_placeholders})
        "))?;
        
        let mut double_params = Vec::with_capacity(expanded_node_ids.len() * 2);
        double_params.extend_from_slice(&expanded_node_ids);
        double_params.extend_from_slice(&expanded_node_ids);
        
        let edges = e_stmt.query_map(rusqlite::params_from_iter(double_params), |row| {
            let src: String = row.get(0)?;
            let rel: String = row.get(1)?;
            let tgt: String = row.get(2)?;
            Ok(format!("{} -[{}]-> {}", src, rel, tgt))
        })?.filter_map(|r| r.ok()).collect();
        
        Ok(crate::db::models::GraphContext { nodes, edges, community_summaries })
    }

    pub fn retrieve_graph_batch(
        &self,
        context_ids: &[i64],
        queries_by_row: &[std::collections::HashMap<i64, Vec<f32>>],
        top_k_nodes: usize,
        hops: usize,
        top_k_communities: usize,
    ) -> Result<Vec<crate::db::models::GraphContext>> {
        // 1. Cache nodes and communities
        let mut cached_nodes: Vec<(i64, i64, Vec<f32>)> = Vec::new();
        let mut cached_communities: Vec<(i64, i64, Vec<f32>)> = Vec::new();
        
        for (model_id, ctxs) in self.contexts_by_model(context_ids)? {
            for cid in ctxs {
                let mut stmt = self.conn.prepare("SELECT id, vector_blob FROM ontology_nodes WHERE context_id = ?1 AND vector_blob IS NOT NULL")?;
                let iter = stmt.query_map([cid], |row| {
                    let id: i64 = row.get(0)?;
                    let blob: Vec<u8> = row.get(1)?;
                    let vec = crate::db::embeddings::blob_to_vector(&blob).unwrap_or_default();
                    Ok((id, model_id, vec))
                })?;
                for item in iter { if let Ok(i) = item { cached_nodes.push(i); } }
                
                let mut stmt_comm = self.conn.prepare("SELECT id, vector_blob FROM ontology_communities WHERE context_id = ?1 AND vector_blob IS NOT NULL")?;
                let iter_comm = stmt_comm.query_map([cid], |row| {
                    let id: i64 = row.get(0)?;
                    let blob: Vec<u8> = row.get(1)?;
                    let vec = crate::db::embeddings::blob_to_vector(&blob).unwrap_or_default();
                    Ok((id, model_id, vec))
                })?;
                for item in iter_comm { if let Ok(i) = item { cached_communities.push(i); } }
            }
        }
        
        // 2. Loop over queries
        let mut batch_results = Vec::with_capacity(queries_by_row.len());
        for query_by_model in queries_by_row {
            let mut hit_node_ids = Vec::new();
            let mut hit_community_ids = Vec::new();
            
            for (model_id, qvec) in query_by_model {
                let mut scored_nodes = Vec::new();
                for (id, m_id, vec) in &cached_nodes {
                    if m_id == model_id && vec.len() == qvec.len() && !vec.is_empty() {
                        let score = crate::db::embeddings::cosine(qvec, vec);
                        scored_nodes.push((score, *id));
                    }
                }
                scored_nodes.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                scored_nodes.truncate(top_k_nodes);
                hit_node_ids.extend(scored_nodes.into_iter().map(|s| s.1));
                
                let mut scored_comms = Vec::new();
                for (id, m_id, vec) in &cached_communities {
                    if m_id == model_id && vec.len() == qvec.len() && !vec.is_empty() {
                        let score = crate::db::embeddings::cosine(qvec, vec);
                        scored_comms.push((score, *id));
                    }
                }
                scored_comms.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                scored_comms.truncate(top_k_communities);
                hit_community_ids.extend(scored_comms.into_iter().map(|s| s.1));
            }
            
            let mut community_summaries = Vec::new();
            if !hit_community_ids.is_empty() {
                let placeholders = std::iter::repeat("?").take(hit_community_ids.len()).collect::<Vec<_>>().join(",");
                let query = format!("SELECT community_label, summary_text FROM ontology_communities WHERE id IN ({})", placeholders);
                let mut stmt = self.conn.prepare_cached(&query)?;
                let summaries = stmt.query_map(rusqlite::params_from_iter(hit_community_ids.iter()), |row| {
                    let label: String = row.get(0)?;
                    let text: String = row.get(1)?;
                    Ok(format!("{}: {}", label, text))
                })?.filter_map(|r| r.ok()).collect::<Vec<_>>();
                community_summaries = summaries;
            }

            if hit_node_ids.is_empty() {
                batch_results.push(crate::db::models::GraphContext { nodes: vec![], edges: vec![], community_summaries });
                continue;
            }
            
            let placeholders = std::iter::repeat("?").take(hit_node_ids.len()).collect::<Vec<_>>().join(",");
            let query = format!("
                WITH RECURSIVE traverse(node_id, depth) AS (
                    SELECT id, 0 FROM ontology_nodes WHERE id IN ({})
                    UNION
                    SELECT e.target_id, t.depth + 1
                    FROM traverse t
                    JOIN ontology_edges e ON e.source_id = t.node_id
                    WHERE t.depth < ?
                    UNION
                    SELECT e.source_id, t.depth + 1
                    FROM traverse t
                    JOIN ontology_edges e ON e.target_id = t.node_id
                    WHERE t.depth < ?
                )
                SELECT DISTINCT node_id FROM traverse;
            ", placeholders);

            let mut stmt = self.conn.prepare_cached(&query)?;
            let mut params: Vec<rusqlite::types::Value> = hit_node_ids.iter().map(|&id| rusqlite::types::Value::Integer(id)).collect();
            params.push(rusqlite::types::Value::Integer(hops as i64));
            params.push(rusqlite::types::Value::Integer(hops as i64));
            
            let expanded_node_ids: Vec<i64> = stmt.query_map(rusqlite::params_from_iter(params), |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
                
            if expanded_node_ids.is_empty() {
                batch_results.push(crate::db::models::GraphContext { nodes: vec![], edges: vec![], community_summaries });
                continue;
            }
            
            let exp_placeholders = std::iter::repeat("?").take(expanded_node_ids.len()).collect::<Vec<_>>().join(",");
            
            let mut n_stmt = self.conn.prepare_cached(&format!("SELECT label, description FROM ontology_nodes WHERE id IN ({})", exp_placeholders))?;
            let nodes = n_stmt.query_map(rusqlite::params_from_iter(expanded_node_ids.iter()), |row| {
                let label: String = row.get(0)?;
                let desc: String = row.get(1)?;
                Ok(format!("{}: {}", label, desc))
            })?.filter_map(|r| r.ok()).collect();
            
            let mut e_stmt = self.conn.prepare_cached(&format!("
                SELECT s.label, e.relation_type, t.label 
                FROM ontology_edges e
                JOIN ontology_nodes s ON e.source_id = s.id
                JOIN ontology_nodes t ON e.target_id = t.id
                WHERE e.source_id IN ({exp_placeholders}) AND e.target_id IN ({exp_placeholders})
            "))?;
            
            let mut double_params = Vec::with_capacity(expanded_node_ids.len() * 2);
            double_params.extend_from_slice(&expanded_node_ids);
            double_params.extend_from_slice(&expanded_node_ids);
            
            let edges = e_stmt.query_map(rusqlite::params_from_iter(double_params), |row| {
                let src: String = row.get(0)?;
                let rel: String = row.get(1)?;
                let tgt: String = row.get(2)?;
                Ok(format!("{} -[{}]-> {}", src, rel, tgt))
            })?.filter_map(|r| r.ok()).collect();
            
            batch_results.push(crate::db::models::GraphContext { nodes, edges, community_summaries });
        }
        
        Ok(batch_results)
    }

    pub fn insert_phase_metric(&self, phase: &str, model_name: &str, ms_per_chunk: f64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_phase_metrics (phase_name, model_name, ms_per_chunk) VALUES (?1, ?2, ?3)",
            rusqlite::params![phase, model_name, ms_per_chunk],
        )?;
        // Keep only the last 3 runs per phase + model
        self.conn.execute(
            "DELETE FROM ontology_phase_metrics WHERE id NOT IN (
                SELECT id FROM ontology_phase_metrics 
                WHERE phase_name = ?1 AND model_name = ?2 
                ORDER BY created_at DESC LIMIT 3
            ) AND phase_name = ?1 AND model_name = ?2",
            rusqlite::params![phase, model_name],
        )?;
        Ok(())
    }

    pub fn get_phase_averages(&self, model_name: &str) -> Result<std::collections::HashMap<String, f64>> {
        let mut stmt = self.conn.prepare(
            "SELECT phase_name, AVG(ms_per_chunk) FROM ontology_phase_metrics WHERE model_name = ?1 GROUP BY phase_name"
        )?;
        let mut map = std::collections::HashMap::new();
        let rows = stmt.query_map([model_name], |row| {
            let phase: String = row.get(0)?;
            let avg: f64 = row.get(1)?;
            Ok((phase, avg))
        })?;
        for r in rows {
            if let Ok((phase, avg)) = r {
                map.insert(phase, avg);
            }
        }
        Ok(map)
    }

    pub fn cache_dedup_decision(&self, context_id: i64, id1: i64, id2: i64, identical: bool) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO ontology_dedup_cache (context_id, id1, id2, identical) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![context_id, id1, id2, identical],
        )?;
        Ok(())
    }

    pub fn get_dedup_cache(&self, context_id: i64) -> Result<std::collections::HashMap<(i64, i64), bool>> {
        let mut stmt = self.conn.prepare(
            "SELECT id1, id2, identical FROM ontology_dedup_cache WHERE context_id = ?1"
        )?;
        let mut map = std::collections::HashMap::new();
        let rows = stmt.query_map([context_id], |row| {
            let id1: i64 = row.get(0)?;
            let id2: i64 = row.get(1)?;
            let identical: bool = row.get(2)?;
            Ok((id1, id2, identical))
        })?;
        for r in rows {
            if let Ok((id1, id2, identical)) = r {
                map.insert((id1, id2), identical);
                map.insert((id2, id1), identical); // Store both directions
            }
        }
        Ok(map)
    }

    pub fn insert_quarantined_chunk(&self, context_id: i64, chunk_id: i64, graph_json: &str, error_reason: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO ontology_quarantine (chunk_id, context_id, graph_json, error_reason) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![chunk_id, context_id, graph_json, error_reason],
        )?;
        Ok(())
    }

    pub fn get_quarantined_chunks(&self, context_id: i64) -> Result<Vec<crate::db::models::OntologyQuarantineChunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT chunk_id, context_id, graph_json, error_reason, created_at FROM ontology_quarantine WHERE context_id = ?1"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            Ok(crate::db::models::OntologyQuarantineChunk {
                chunk_id: row.get(0)?,
                context_id: row.get(1)?,
                graph_json: row.get(2)?,
                error_reason: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn delete_quarantined_chunk(&self, chunk_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM ontology_quarantine WHERE chunk_id = ?1",
            [chunk_id]
        )?;
        Ok(())
    }

    pub fn save_chunk_state(&self, context_id: i64, chunk_id: i64, completed_batches_json: &str, partial_graph_json: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_chunk_states (context_id, chunk_id, completed_batches_json, partial_graph_json)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(context_id, chunk_id) DO UPDATE SET
             completed_batches_json = excluded.completed_batches_json,
             partial_graph_json = excluded.partial_graph_json",
            rusqlite::params![context_id, chunk_id, completed_batches_json, partial_graph_json]
        )?;
        Ok(())
    }

    pub fn load_chunk_state(&self, context_id: i64, chunk_id: i64) -> Result<Option<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT completed_batches_json, partial_graph_json FROM ontology_chunk_states WHERE context_id = ?1 AND chunk_id = ?2"
        )?;
        let mut iter = stmt.query_map(rusqlite::params![context_id, chunk_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        if let Some(res) = iter.next() {
            Ok(Some(res?))
        } else {
            Ok(None)
        }
    }

    pub fn delete_chunk_state(&self, context_id: i64, chunk_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM ontology_chunk_states WHERE context_id = ?1 AND chunk_id = ?2",
            rusqlite::params![context_id, chunk_id]
        )?;
        Ok(())
    }

    // --- Admin / Manual Curation ---

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
