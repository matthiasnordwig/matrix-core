//! Cheap per-collection numbers for the Library cards: document count, concept
//! (node) count and a size-capped graph preview (counts and degrees in SQL, the
//! connected selection in [`grow_preview_selection`]), so the UI never has to
//! load whole document/node/edge lists.
//!
//! Semantics mirror the lens-aware list commands the cards used to call:
//! concepts = `ontology_nodes` of the context (a lens only re-types nodes, it
//! never hides them); the preview graph counts degree over the active lens's
//! edges (`deleted` verdicts excluded, `reversed` ones keep their endpoints).
//! A self-loop counts twice, like the former client-side degree count. Each
//! preview concept carries its community's colour slot under the active lens —
//! the same position the graph view's palette uses (communities by id).
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::db::{Database, Result};

/// One concept in the preview: node id, its degree over the lens-aware edges
/// and its community's colour slot (0-based position among the active lens's
/// communities ordered by id; `None` = in no community, drawn grey).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewNode {
    pub id: i64,
    pub degree: i64,
    pub community: Option<i64>,
}

/// Lens-aware edges of a context as `(source, target)` rows (`deleted` excluded);
/// shared by the degree and the link query. `?1` = context id.
const LENS_EDGES_CTE: &str = "le AS (
    SELECT e.source_id AS s, e.target_id AS t
    FROM ontology_edges e
    JOIN contexts ctx ON ctx.id = e.context_id
    LEFT JOIN ontology_lens_edge_verdicts v ON v.edge_id = e.id AND v.lens_id = ctx.active_lens_id
    WHERE e.context_id = ?1 AND COALESCE(v.verdict, 'valid') != 'deleted'
)";

impl Database {
    /// Documents per context (contexts without documents are absent).
    pub fn count_documents_by_context(&self) -> Result<HashMap<i64, i64>> {
        let mut stmt = self.conn.prepare("SELECT context_id, COUNT(*) FROM documents GROUP BY context_id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
    }

    /// Concepts (ontology nodes) per context (contexts without nodes are absent).
    pub fn count_ontology_nodes_by_context(&self) -> Result<HashMap<i64, i64>> {
        let mut stmt = self.conn.prepare("SELECT context_id, COUNT(*) FROM ontology_nodes GROUP BY context_id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
    }

    /// The `limit` best-connected concepts of a context, most connected first
    /// (ties by ascending id). A node in several communities gets the last one's
    /// slot, like the graph view's membership map (later communities overwrite).
    pub fn top_ontology_nodes_by_degree(&self, context_id: i64, limit: usize) -> Result<Vec<PreviewNode>> {
        let sql = format!(
            "WITH {LENS_EDGES_CTE},
                  deg AS (SELECT id, COUNT(*) AS d FROM (SELECT s AS id FROM le UNION ALL SELECT t FROM le) GROUP BY id),
                  comm AS (
                    SELECT co.id, ROW_NUMBER() OVER (ORDER BY co.id) - 1 AS slot
                    FROM ontology_communities co JOIN contexts ctx ON ctx.id = co.context_id
                    WHERE co.context_id = ?1 AND co.lens_id IS ctx.active_lens_id
                  )
             SELECT n.id, COALESCE(deg.d, 0) AS d,
                    (SELECT MAX(comm.slot) FROM ontology_community_members m JOIN comm ON comm.id = m.community_id
                     WHERE m.node_id = n.id) AS slot
             FROM ontology_nodes n LEFT JOIN deg ON deg.id = n.id
             WHERE n.context_id = ?1
             ORDER BY d DESC, n.id ASC
             LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![context_id, limit as i64], |r| {
            Ok(PreviewNode { id: r.get(0)?, degree: r.get(1)?, community: r.get(2)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// All distinct undirected links `(low_id, high_id)` over the lens-aware
    /// edges of the context (self-loops dropped).
    pub fn ontology_links(&self, context_id: i64) -> Result<Vec<(i64, i64)>> {
        let sql = format!(
            "WITH {LENS_EDGES_CTE}
             SELECT DISTINCT MIN(s, t), MAX(s, t) FROM le WHERE s != t ORDER BY 1, 2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([context_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The card miniature's graph: up to `limit` concepts chosen by
    /// [`grow_preview_selection`] (most connected first) and the links among them.
    pub fn preview_graph(&self, context_id: i64, limit: usize) -> Result<(Vec<PreviewNode>, Vec<(i64, i64)>)> {
        let nodes = self.top_ontology_nodes_by_degree(context_id, i64::MAX as usize)?;
        let links = self.ontology_links(context_id)?;
        let picked: HashSet<i64> = grow_preview_selection(&nodes, &links, limit).into_iter().collect();
        let nodes: Vec<PreviewNode> = nodes.into_iter().filter(|n| picked.contains(&n.id)).collect();
        let links = links.into_iter().filter(|(a, b)| picked.contains(a) && picked.contains(b)).collect();
        Ok((nodes, links))
    }
}

/// Picks up to `limit` concepts that form one or a few connected shapes instead
/// of the top-by-degree list (whose links often lie outside the selection).
/// Grows from the strongest hub: next is always the frontier concept with the
/// most links into the selection, ties by degree (`nodes` order: most connected
/// first). When a shape is exhausted, the next one starts at the strongest
/// remaining hub of a graph component with at least 3 concepts; pieces of fewer
/// than 3 concepts left by the budget are dropped. Without any such component
/// (no links, only pairs) the first `limit` concepts are taken as they are.
/// Deterministic; `links` are undirected `(a, b)` pairs.
pub fn grow_preview_selection(nodes: &[PreviewNode], links: &[(i64, i64)], limit: usize) -> Vec<i64> {
    let rank: HashMap<i64, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
    let mut adj: HashMap<i64, Vec<i64>> = HashMap::new();
    for &(a, b) in links {
        if a != b && rank.contains_key(&a) && rank.contains_key(&b) {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
    }
    let component_size = component_sizes(nodes, &adj);
    let mut picked: Vec<i64> = Vec::new();
    let mut is_picked: HashSet<i64> = HashSet::new();
    let mut into: HashMap<i64, usize> = HashMap::new(); // frontier → links into the selection
    let mut seeds = nodes.iter().map(|n| n.id).filter(|id| component_size[id] >= 3);
    while picked.len() < limit {
        let next = into
            .iter()
            .max_by(|(a, ka), (b, kb)| ka.cmp(kb).then(rank[b].cmp(&rank[a])))
            .map(|(id, _)| *id)
            .or_else(|| seeds.find(|id| !is_picked.contains(id)));
        let Some(id) = next else { break };
        into.remove(&id);
        is_picked.insert(id);
        picked.push(id);
        for nb in adj.get(&id).into_iter().flatten() {
            if !is_picked.contains(nb) {
                *into.entry(*nb).or_insert(0) += 1;
            }
        }
    }
    if picked.is_empty() {
        return nodes.iter().take(limit).map(|n| n.id).collect();
    }
    // Drop pieces of fewer than 3 picked concepts (the budget ran out right after a new seed).
    let mut sub: HashMap<i64, Vec<i64>> = HashMap::new();
    for id in &picked {
        let nbs = adj.get(id).map(|v| v.iter().copied().filter(|n| is_picked.contains(n)).collect()).unwrap_or_default();
        sub.insert(*id, nbs);
    }
    let shown: Vec<PreviewNode> = nodes.iter().filter(|n| is_picked.contains(&n.id)).cloned().collect();
    let sizes = component_sizes(&shown, &sub);
    shown.iter().filter(|n| sizes[&n.id] >= 3).map(|n| n.id).collect()
}

/// Size of each node's connected component.
fn component_sizes(nodes: &[PreviewNode], adj: &HashMap<i64, Vec<i64>>) -> HashMap<i64, usize> {
    let mut size = HashMap::new();
    for n in nodes {
        if size.contains_key(&n.id) {
            continue;
        }
        let mut members = vec![n.id];
        let mut seen: HashSet<i64> = HashSet::from([n.id]);
        let mut i = 0;
        while i < members.len() {
            for nb in adj.get(&members[i]).into_iter().flatten() {
                if seen.insert(*nb) {
                    members.push(*nb);
                }
            }
            i += 1;
        }
        for m in &members {
            size.insert(*m, members.len());
        }
    }
    size
}
