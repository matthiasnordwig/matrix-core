//! CRUD + embedding/merge logic for `ontology_nodes`. Split out of the former
//! monolithic `db/ontology.rs` — see HANDBUCH.md. Manual-curation node
//! methods (insert/update/delete/context lookup/semantic search) were
//! originally grouped under an "Admin / Manual Curation" comment at the
//! bottom of that file; they're grouped here by entity instead.
use crate::db::{Database, Result};
use crate::db::models::{OntologyNode, NewOntologyNode, MergeLogEntry};
use rusqlite::OptionalExtension;

impl Database {
    pub fn list_ontology_nodes(&self, context_id: i64) -> Result<Vec<OntologyNode>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, context_id, label, entity_type, raw_entity_type, description, community_id, created_at
             FROM ontology_nodes WHERE context_id = ?1"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            Ok(OntologyNode {
                id: row.get(0)?,
                context_id: row.get(1)?,
                label: row.get(2)?,
                entity_type: row.get(3)?,
                raw_entity_type: row.get(4)?,
                description: row.get(5)?,
                community_id: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Single-node fetch (same columns as `list_ontology_nodes`) — used by the
    /// edge-review "re-check" action (`recheck_edge_review` command), which
    /// needs an edge's two endpoint nodes' *current* label/`entity_type`.
    /// Deliberately not `get_ontology_nodes_with_descriptions` (misleadingly
    /// named — it returns `(label, description)`, not `(label, entity_type)`,
    /// see ISSUES.md): re-check wants the real `entity_type` field.
    pub fn get_ontology_node(&self, id: i64) -> Result<Option<OntologyNode>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, context_id, label, entity_type, raw_entity_type, description, community_id, created_at
             FROM ontology_nodes WHERE id = ?1"
        )?;
        let mut rows = stmt.query_map([id], |row| {
            Ok(OntologyNode {
                id: row.get(0)?,
                context_id: row.get(1)?,
                label: row.get(2)?,
                entity_type: row.get(3)?,
                raw_entity_type: row.get(4)?,
                description: row.get(5)?,
                community_id: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    /// Like `list_ontology_nodes`, but `entity_type` reflects the context's
    /// active lens: `COALESCE(active-lens resolved_type, raw_entity_type)`.
    /// With `active_lens_id = NULL` the LEFT JOIN never matches, so every row
    /// falls back to its raw type (identical to the raw view). Used by the
    /// Ontology graph tab so the visualization matches the lens the chat/grid
    /// retrieval already applies (see `retrieval.rs`); the raw variant above
    /// stays for the pipeline/export paths that must see un-lensed data.
    pub fn list_ontology_nodes_for_active_lens(&self, context_id: i64) -> Result<Vec<OntologyNode>> {
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.context_id, n.label,
                    COALESCE(lnt.resolved_type, n.raw_entity_type) AS entity_type,
                    n.raw_entity_type, n.description, n.community_id, n.created_at
             FROM ontology_nodes n
             JOIN contexts c ON c.id = n.context_id
             LEFT JOIN ontology_lens_node_types lnt ON lnt.node_id = n.id AND lnt.lens_id = c.active_lens_id
             WHERE n.context_id = ?1"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            Ok(OntologyNode {
                id: row.get(0)?,
                context_id: row.get(1)?,
                label: row.get(2)?,
                entity_type: row.get(3)?,
                raw_entity_type: row.get(4)?,
                description: row.get(5)?,
                community_id: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// `raw_entity_type` mirrors `entity_type` at insert time — nothing has
    /// snapped anything yet, so they start out identical (see BACKLOG.md's
    /// Lens system: only `materialize_lens` reads/writes a distinct resolved
    /// type from here on, in the separate `ontology_lens_node_types` table).
    pub fn create_ontology_node(&self, new: &NewOntologyNode) -> Result<OntologyNode> {
        self.conn.execute(
            "INSERT INTO ontology_nodes (context_id, label, entity_type, raw_entity_type, description)
             VALUES (?1, ?2, ?3, ?3, ?4)",
            rusqlite::params![new.context_id, new.label, new.entity_type, new.description],
        )?;
        let id = self.conn.last_insert_rowid();
        let mut stmt = self.conn.prepare(
            "SELECT id, context_id, label, entity_type, raw_entity_type, description, community_id, created_at
             FROM ontology_nodes WHERE id = ?1"
        )?;
        let node = stmt.query_row([id], |row| {
            Ok(OntologyNode {
                id: row.get(0)?,
                context_id: row.get(1)?,
                label: row.get(2)?,
                entity_type: row.get(3)?,
                raw_entity_type: row.get(4)?,
                description: row.get(5)?,
                community_id: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        Ok(node)
    }

    pub fn insert_ontology_node_fast(&self, new: &NewOntologyNode) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO ontology_nodes (context_id, label, entity_type, raw_entity_type, description) VALUES (?1, ?2, ?3, ?3, ?4)",
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

    /// Feeds `materialize_lens`'s exhaustive resolve loop (every node, not
    /// just ones whose raw type isn't allowed — see BACKLOG.md's Lens system).
    pub fn get_ontology_nodes_raw(&self, context_id: i64) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, raw_entity_type FROM ontology_nodes WHERE context_id = ?1")?;
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

    /// Batched `id -> (label, entity_type)` for a whole context. Mirrors
    /// `get_ontology_node`'s `label, entity_type` columns, just for every node
    /// at once. Distinct from `get_ontology_nodes_with_descriptions` (which
    /// yields the free-text *description* as the second field) — callers that
    /// reason about a node's *type* (e.g. the `LabelEqualsType` structural
    /// lint) must use this one, not the description variant.
    pub fn get_ontology_nodes_with_types(&self, context_id: i64) -> Result<std::collections::HashMap<i64, (String, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, label, entity_type FROM ontology_nodes WHERE context_id = ?1")?;
        let iter = stmt.query_map([context_id], |row| {
            let id: i64 = row.get(0)?;
            let label: String = row.get(1)?;
            let entity_type: String = row.get(2)?;
            Ok((id, (label, entity_type)))
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

    /// Count nodes with a vector without decoding them — used for export
    /// size estimates (`get_ontology_nodes_with_embeddings` would decode
    /// every BLOB).
    pub fn count_ontology_nodes_with_embeddings(&self, context_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM ontology_nodes WHERE context_id = ?1 AND vector_blob IS NOT NULL",
            [context_id],
            |row| row.get(0),
        )?)
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

    /// Like `get_ontology_nodes_with_embeddings`, but the type in tuple
    /// position `.1` is the context's *active lens*-resolved type (falling
    /// back to the raw type where no lens/no mapping row exists) instead of
    /// the raw type directly — used by dedup bucketing so pair volume scales
    /// with the schema's type count, not with however many distinct raw
    /// types extraction happened to produce (see BACKLOG.md's Lens system).
    pub fn get_ontology_nodes_with_embeddings_and_lens_type(&self, context_id: i64) -> Result<Vec<(i64, String, String, String, Vec<f32>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT o.id, COALESCE(lnt.resolved_type, o.raw_entity_type), o.label, o.description, o.vector_blob
             FROM ontology_nodes o
             JOIN contexts c ON c.id = o.context_id
             LEFT JOIN ontology_lens_node_types lnt ON lnt.node_id = o.id AND lnt.lens_id = c.active_lens_id
             WHERE o.context_id = ?1 AND o.vector_blob IS NOT NULL"
        )?;
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

    /// Merges each `(keep, drop)` pair: rewires the loser's edges onto the
    /// winner, collapses would-be duplicate edges (evidence chunks carried
    /// over), logs the loser's label/type to `ontology_merge_log` (schema_v38,
    /// see ISSUES.md "ontology_dedup_cache — keine Merge-Historie ..." — the
    /// log write and the DELETE below happen in the same transaction, so the
    /// history survives the hard-delete atomically), then hard-deletes the
    /// loser node. Returns the pairs actually executed: a pair whose WINNER no
    /// longer exists is skipped instead of failing the whole transaction —
    /// with `PRAGMA foreign_keys = ON`, the edge-rewire UPDATE would otherwise
    /// hit a FOREIGN KEY error and roll back every merge in the batch (seen in
    /// production when a stale dedup candidate elected an already-merged node
    /// as winner; chain resolution happens in the caller, this is the
    /// last-line safety net). A pair whose LOSER no longer exists (already
    /// merged away by an earlier pair in the same batch) is skipped the same
    /// way — nothing left to log or delete.
    pub fn merge_ontology_nodes(&self, merges: &[(i64, i64)]) -> Result<Vec<(i64, i64)>> {
        let tx = self.conn.unchecked_transaction()?;
        let mut executed = Vec::with_capacity(merges.len());
        for &(keep_id, drop_id) in merges {
            let keeper_alive: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM ontology_nodes WHERE id = ?1)",
                [keep_id],
                |row| row.get(0),
            )?;
            if !keeper_alive {
                continue;
            }
            let loser: Option<(i64, String, String)> = tx
                .query_row(
                    "SELECT context_id, label, entity_type FROM ontology_nodes WHERE id = ?1",
                    [drop_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            let Some((context_id, loser_label, loser_entity_type)) = loser else {
                continue;
            };
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

            // Rewiring can turn an edge that connected the two merged nodes
            // (keep<->drop) into a self-loop keep->keep — a meaningless "A
            // relates to A" artifact the duplicate-collapse above doesn't
            // catch (it only merges edges sharing the *other* endpoint). Drop
            // any self-loop on the winner; evidence rows cascade with the edge.
            tx.execute("DELETE FROM ontology_edges WHERE source_id = ?1 AND target_id = ?1", rusqlite::params![keep_id])?;

            tx.execute(
                "INSERT INTO ontology_merge_log (context_id, winner_id, loser_id, loser_label, loser_entity_type)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![context_id, keep_id, drop_id, loser_label, loser_entity_type],
            )?;

            tx.execute("DELETE FROM ontology_nodes WHERE id = ?1", rusqlite::params![drop_id])?;
            executed.push((keep_id, drop_id));
        }
        tx.commit()?;
        Ok(executed)
    }

    /// Reads back `ontology_merge_log` for a context (schema_v38, see
    /// ISSUES.md "ontology_dedup_cache — keine Merge-Historie ..."), newest
    /// first. Never called by the extraction/dedup pipeline itself — only
    /// for later retrieval (e.g. `scripts/eval_extraction.py`) and the
    /// `list_ontology_merge_log` Tauri command.
    pub fn list_ontology_merge_log(&self, context_id: i64) -> Result<Vec<MergeLogEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, context_id, winner_id, loser_id, loser_label, loser_entity_type, merged_at
             FROM ontology_merge_log
             WHERE context_id = ?1
             ORDER BY id DESC"
        )?;
        let rows = stmt.query_map([context_id], |row| {
            Ok(MergeLogEntry {
                id: row.get(0)?,
                context_id: row.get(1)?,
                winner_id: row.get(2)?,
                loser_id: row.get(3)?,
                loser_label: row.get(4)?,
                loser_entity_type: row.get(5)?,
                merged_at: row.get(6)?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn insert_ontology_node(&self, context_id: i64, label: &str, entity_type: &str, description: &str, vector_blob: &[u8]) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_nodes (context_id, label, entity_type, raw_entity_type, description, vector_blob) VALUES (?1, ?2, ?3, ?3, ?4, ?5)",
            rusqlite::params![context_id, label, entity_type, description, vector_blob],
        )?;
        Ok(())
    }

    /// Manual curation: a user's edit supersedes whatever was extracted, so
    /// `raw_entity_type` is updated alongside `entity_type` (lens
    /// materialization never touches these columns, see
    /// `ontology_lens_node_types`).
    pub fn update_ontology_node(&self, id: i64, label: &str, entity_type: &str, description: &str, vector_blob: &[u8]) -> Result<()> {
        self.conn.execute(
            "UPDATE ontology_nodes SET label = ?1, entity_type = ?2, raw_entity_type = ?2, description = ?3, vector_blob = ?4 WHERE id = ?5",
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
