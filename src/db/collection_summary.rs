//! Cheap per-collection numbers for the Library cards: document count, concept
//! (node) count and a size-capped graph preview, all computed in SQL so the UI
//! never has to load whole document/node/edge lists.
//!
//! Semantics mirror the lens-aware list commands the cards used to call:
//! concepts = `ontology_nodes` of the context (a lens only re-types nodes, it
//! never hides them); the preview graph counts degree over the active lens's
//! edges (`deleted` verdicts excluded, `reversed` ones keep their endpoints).
//! A self-loop counts twice, like the former client-side degree count.
use std::collections::HashMap;

use rusqlite::params_from_iter;
use serde::{Deserialize, Serialize};

use crate::db::{Database, Result};

/// One concept in the preview: node id and its degree over the lens-aware edges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewNode {
    pub id: i64,
    pub degree: i64,
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
    /// (ties by ascending id).
    pub fn top_ontology_nodes_by_degree(&self, context_id: i64, limit: usize) -> Result<Vec<PreviewNode>> {
        let sql = format!(
            "WITH {LENS_EDGES_CTE},
                  deg AS (SELECT id, COUNT(*) AS d FROM (SELECT s AS id FROM le UNION ALL SELECT t FROM le) GROUP BY id)
             SELECT n.id, COALESCE(deg.d, 0) AS d
             FROM ontology_nodes n LEFT JOIN deg ON deg.id = n.id
             WHERE n.context_id = ?1
             ORDER BY d DESC, n.id ASC
             LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![context_id, limit as i64], |r| {
            Ok(PreviewNode { id: r.get(0)?, degree: r.get(1)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Distinct undirected links `(low_id, high_id)` among `ids` over the
    /// lens-aware edges of the context (self-loops dropped).
    pub fn ontology_links_among(&self, context_id: i64, ids: &[i64]) -> Result<Vec<(i64, i64)>> {
        if ids.len() < 2 {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; ids.len()].join(",");
        let sql = format!(
            "WITH {LENS_EDGES_CTE}
             SELECT DISTINCT MIN(s, t), MAX(s, t) FROM le
             WHERE s != t AND s IN ({marks}) AND t IN ({marks})
             ORDER BY 1, 2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let args = std::iter::once(context_id).chain(ids.iter().copied()).chain(ids.iter().copied());
        let rows = stmt.query_map(params_from_iter(args), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}
