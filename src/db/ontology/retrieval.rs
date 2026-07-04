//! GraphRAG retrieval: brute-force cosine search over node/community
//! embeddings followed by a recursive-CTE hop expansion, formatted into
//! plain-text `GraphContext` snippets for the chat prompt. Split out of the
//! former monolithic `db/ontology.rs` — see HANDBUCH.md.
//!
//! `retrieve_graph_with` and `retrieve_graph_batch` implement the same
//! algorithm; the batch variant caches the node/community embedding scan
//! once across all rows of a Grid run instead of re-scanning per row. Kept
//! as two separate functions (rather than one generalized over a single vs.
//! multi query) to avoid restructuring working, performance-sensitive code
//! without a compiler available to verify the refactor.
use crate::db::{Database, Result};

impl Database {
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
}
