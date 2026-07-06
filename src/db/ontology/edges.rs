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
            "SELECT e.id, e.context_id, e.source_id, e.target_id, e.relation_type, e.raw_relation_type, json_group_array(json_object('chunk_id', c.chunk_id, 'evidence', c.evidence)) as chunk_data, e.created_at
             FROM ontology_edges e
             LEFT JOIN ontology_edge_chunks c ON e.id = c.edge_id
             WHERE e.context_id = ?1
             GROUP BY e.id"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            let chunk_data_str: Option<String> = row.get(6)?;
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
                raw_relation_type: row.get(5)?,
                chunk_evidences,
                created_at: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Like `list_ontology_edges`, but applies the context's active lens as a
    /// display-time overlay (mirrors `retrieval.rs`, which the chat/grid graph
    /// context already uses): `deleted`-verdict edges are excluded, `reversed`
    /// edges have their source/target swapped in the result, and
    /// `relation_type` becomes `COALESCE(resolved_relation_type,
    /// raw_relation_type)`. With `active_lens_id = NULL` no verdict row
    /// matches, so every edge shows raw/unfiltered. Used by the Ontology graph
    /// tab; the raw variant above stays for the pipeline/export paths.
    /// The swap is display-only — `id` still identifies the stored edge, so
    /// curation actions keyed on it are unaffected.
    pub fn list_ontology_edges_for_active_lens(&self, context_id: i64) -> Result<Vec<OntologyEdge>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.id, e.context_id,
                    CASE WHEN v.verdict = 'reversed' THEN e.target_id ELSE e.source_id END AS source_id,
                    CASE WHEN v.verdict = 'reversed' THEN e.source_id ELSE e.target_id END AS target_id,
                    COALESCE(v.resolved_relation_type, e.raw_relation_type) AS relation_type,
                    e.raw_relation_type,
                    json_group_array(json_object('chunk_id', c.chunk_id, 'evidence', c.evidence)) as chunk_data,
                    e.created_at
             FROM ontology_edges e
             JOIN contexts ctx ON ctx.id = e.context_id
             LEFT JOIN ontology_lens_edge_verdicts v ON v.edge_id = e.id AND v.lens_id = ctx.active_lens_id
             LEFT JOIN ontology_edge_chunks c ON e.id = c.edge_id
             WHERE e.context_id = ?1 AND COALESCE(v.verdict, 'valid') != 'deleted'
             GROUP BY e.id"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            let chunk_data_str: Option<String> = row.get(6)?;
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
                raw_relation_type: row.get(5)?,
                chunk_evidences,
                created_at: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// The `SELECT id FROM ontology_edges WHERE ...` lookup duplicated inline
    /// in `create_ontology_edge`/`insert_ontology_edge_fast`, extracted so
    /// callers that only have the natural key (e.g. import remapping) can
    /// resolve the id without re-deriving that query themselves.
    pub fn get_ontology_edge_id(&self, context_id: i64, source_id: i64, target_id: i64, relation_type: &str) -> Result<Option<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM ontology_edges WHERE context_id = ?1 AND source_id = ?2 AND target_id = ?3 AND LOWER(relation_type) = LOWER(?4)"
        )?;
        let mut rows = stmt.query(rusqlite::params![context_id, source_id, target_id, relation_type])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// `raw_relation_type` mirrors `relation_type` at insert time — see
    /// `nodes.rs::create_ontology_node`'s doc comment for the same reasoning.
    pub fn create_ontology_edge(&self, new: &NewOntologyEdge) -> Result<OntologyEdge> {
        self.conn.execute(
            "INSERT OR IGNORE INTO ontology_edges (context_id, source_id, target_id, relation_type, raw_relation_type)
             VALUES (?1, ?2, ?3, ?4, ?4)",
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
            "SELECT e.id, e.context_id, e.source_id, e.target_id, e.relation_type, e.raw_relation_type, json_group_array(json_object('chunk_id', c.chunk_id, 'evidence', c.evidence)) as chunk_data, e.created_at
             FROM ontology_edges e
             LEFT JOIN ontology_edge_chunks c ON e.id = c.edge_id
             WHERE e.id = ?1
             GROUP BY e.id"
        )?;
        let edge = stmt.query_row([edge_id], |row| {
            let chunk_data_str: Option<String> = row.get(6)?;
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
                raw_relation_type: row.get(5)?,
                chunk_evidences,
                created_at: row.get(7)?,
            })
        })?;
        Ok(edge)
    }

    pub fn insert_ontology_edge_fast(&self, new: &NewOntologyEdge) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO ontology_edges (context_id, source_id, target_id, relation_type, raw_relation_type) VALUES (?1, ?2, ?3, ?4, ?4)",
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

    /// Feeds `materialize_lens`'s exhaustive resolve loop (every edge, not
    /// just ones violating a constraint — see BACKLOG.md's Lens system).
    pub fn get_ontology_edges_raw(&self, context_id: i64) -> Result<Vec<(i64, i64, i64, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, source_id, target_id, raw_relation_type FROM ontology_edges WHERE context_id = ?1")?;
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

    /// Like `get_ontology_edges_full`, but excludes edges the context's
    /// active lens verdicted `'deleted'` and uses the lens-resolved relation
    /// type (falling back to raw where no lens/no mapping row exists) — see
    /// BACKLOG.md's Lens system. Used by community detection so a lens's
    /// "removed" edges don't shape community structure.
    pub fn get_ontology_edges_full_for_lens(&self, context_id: i64) -> Result<Vec<(i64, i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.source_id, e.target_id, COALESCE(v.resolved_relation_type, e.raw_relation_type)
             FROM ontology_edges e
             JOIN contexts c ON c.id = e.context_id
             LEFT JOIN ontology_lens_edge_verdicts v ON v.edge_id = e.id AND v.lens_id = c.active_lens_id
             WHERE e.context_id = ?1 AND COALESCE(v.verdict, 'valid') != 'deleted'"
        )?;
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

    /// Manual curation: mirrors `update_ontology_node`'s reasoning — a user's
    /// edit supersedes the extracted value, so `raw_relation_type` is updated
    /// alongside `relation_type`.
    pub fn update_ontology_edge(&self, id: i64, relation_type: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE ontology_edges SET relation_type = ?1, raw_relation_type = ?1 WHERE id = ?2",
            rusqlite::params![relation_type, id],
        )?;
        Ok(())
    }

    pub fn insert_ontology_edge_fast_primitive(&self, context_id: i64, source_id: i64, target_id: i64, relation_type: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_edges (context_id, source_id, target_id, relation_type, raw_relation_type) VALUES (?1, ?2, ?3, ?4, ?4)",
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

    /// Like `add_ontology_edge_chunk`, but also records (or updates) the
    /// evidence text for that (edge, chunk) pair — needed when importing an
    /// edge whose second-and-later evidence chunks would otherwise lose
    /// their evidence text (unlike `create_ontology_edge`/
    /// `insert_ontology_edge_fast`, which only accept one chunk_id/evidence
    /// pair at creation time).
    pub fn add_ontology_edge_chunk_with_evidence(&self, edge_id: i64, chunk_id: i64, evidence: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_edge_chunks (edge_id, chunk_id, evidence) VALUES (?1, ?2, ?3)
             ON CONFLICT(edge_id, chunk_id) DO UPDATE SET evidence = COALESCE(excluded.evidence, ontology_edge_chunks.evidence)",
            rusqlite::params![edge_id, chunk_id, evidence],
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
