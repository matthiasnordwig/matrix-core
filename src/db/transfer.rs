//! Row-level access for moving a whole context between databases
//! (`services::context_transfer`, the export/import bundle) plus the schema
//! introspection its completeness test needs.
//!
//! - Readers return exactly what a table holds (raw vector BLOBs, lens rows),
//!   so the bundle can reproduce the context without a detour through the
//!   pipeline's own (filtered/joined) read paths.
//! - Writers insert rows with their exact values (raw types, timestamps)
//!   and **never open a transaction of their own** — the import runs them
//!   inside its single `BEGIN … COMMIT`, where a nested `BEGIN` would fail.
//! - [`Database::schema_tables`] / [`Database::count_context_rows`] read the
//!   live schema (`sqlite_master`, `PRAGMA table_info`/`foreign_key_list`):
//!   the transfer's completeness test derives the set of context-bound tables
//!   from it instead of a hand-kept list. [`TableSchema`] lives here, not in
//!   `models.rs`: it describes the schema, it is no domain row.

use rusqlite::params;

use super::models::*;
use super::{CoreError, Database, Result};

/// One table of the live schema as `Database::schema_tables` reads it:
/// columns and outgoing foreign keys `(from column, parent table)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<String>,
    pub foreign_keys: Vec<(String, String)>,
    /// `CREATE VIRTUAL TABLE` (FTS5); its shadow tables are ordinary tables.
    pub is_virtual: bool,
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

impl Database {
    /// Every ordinary table of the schema (no `sqlite_*` internals) with its
    /// columns and outgoing foreign keys; virtual tables are flagged (FTS5
    /// shadow tables are ordinary tables without foreign keys).
    pub fn schema_tables(&self) -> Result<Vec<TableSchema>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, COALESCE(sql, '') FROM sqlite_master
              WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let tables: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut out = Vec::with_capacity(tables.len());
        for (name, sql) in tables {
            let is_virtual = sql.trim_start().to_ascii_uppercase().starts_with("CREATE VIRTUAL");
            let q = quote_ident(&name);
            let columns: Vec<String> = self
                .conn
                .prepare(&format!("PRAGMA table_info({q})"))?
                .query_map([], |r| r.get::<_, String>(1))?
                .collect::<rusqlite::Result<_>>()?;
            let foreign_keys: Vec<(String, String)> = self
                .conn
                .prepare(&format!("PRAGMA foreign_key_list({q})"))?
                .query_map([], |r| Ok((r.get::<_, String>(3)?, r.get::<_, String>(2)?)))?
                .collect::<rusqlite::Result<_>>()?;
            out.push(TableSchema { name, columns, foreign_keys, is_virtual });
        }
        Ok(out)
    }

    /// Rows of `table` that belong to `context_id`: `contexts` by id, a table
    /// with a `context_id` column directly, otherwise through its first
    /// foreign key whose parent has a `context_id` column (one hop, e.g.
    /// `ontology_edge_chunks` → `ontology_edges`). Error for an unknown table
    /// or one without such a path.
    pub fn count_context_rows(&self, table: &str, context_id: i64) -> Result<i64> {
        let tables = self.schema_tables()?;
        let find = |n: &str| tables.iter().find(|t| t.name == n);
        let t = find(table).ok_or_else(|| CoreError::NotFound(format!("table {table}")))?;
        let q = quote_ident(&t.name);
        let sql = if t.name == "contexts" {
            "SELECT COUNT(*) FROM contexts WHERE id = ?1".to_string()
        } else if t.columns.iter().any(|c| c == "context_id") {
            format!("SELECT COUNT(*) FROM {q} WHERE context_id = ?1")
        } else {
            let (col, parent) = t
                .foreign_keys
                .iter()
                .find(|(_, p)| find(p).is_some_and(|pt| pt.columns.iter().any(|c| c == "context_id")))
                .ok_or_else(|| CoreError::InvalidState(format!("table {table} has no path to a context")))?;
            format!(
                "SELECT COUNT(*) FROM {q} t JOIN {p} p ON p.id = t.{c} WHERE p.context_id = ?1",
                p = quote_ident(parent),
                c = quote_ident(col)
            )
        };
        Ok(self.conn.query_row(&sql, [context_id], |r| r.get(0))?)
    }

    /// `(chunk_id, dim, vector BLOB)` of a context's chunk vectors made by
    /// `embedding_model_id` (vectors of an earlier model are left out).
    pub fn chunk_vector_blobs(&self, context_id: i64, embedding_model_id: i64) -> Result<Vec<(i64, i64, Vec<u8>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT chunk_id, dim, vector FROM embeddings
              WHERE context_id = ?1 AND embedding_model_id = ?2 ORDER BY chunk_id",
        )?;
        let rows = stmt.query_map(params![context_id, embedding_model_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// `(community_id, vector BLOB)` of a context's communities that have a
    /// vector of the context's current model (schema_v67; others are left out).
    pub fn community_vector_blobs(&self, context_id: i64) -> Result<Vec<(i64, Vec<u8>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT co.id, co.vector_blob FROM ontology_communities co JOIN contexts c ON c.id = co.context_id
              WHERE co.context_id = ?1 AND co.vector_blob IS NOT NULL
                AND co.vector_model_id = c.embedding_model_id ORDER BY co.id",
        )?;
        let rows = stmt.query_map([context_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// `(node_id, resolved_type)` rows of one lens.
    pub fn lens_node_types(&self, lens_id: i64) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT node_id, resolved_type FROM ontology_lens_node_types WHERE lens_id = ?1 ORDER BY node_id",
        )?;
        let rows = stmt.query_map([lens_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// `(edge_id, verdict, resolved_relation_type)` rows of one lens.
    pub fn lens_edge_verdicts(&self, lens_id: i64) -> Result<Vec<(i64, String, Option<String>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT edge_id, verdict, resolved_relation_type FROM ontology_lens_edge_verdicts
              WHERE lens_id = ?1 ORDER BY edge_id",
        )?;
        let rows = stmt.query_map([lens_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Sum of the JPEG sizes of a context's page images.
    pub fn page_image_total_bytes(&self, context_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(image)), 0) FROM page_images WHERE context_id = ?1",
            [context_id],
            |r| r.get(0),
        )?)
    }

    /// Insert a node with its raw type as given (`create_ontology_node` would
    /// set the raw type to `entity_type`). Returns the new id.
    pub fn insert_ontology_node_exact(
        &self,
        context_id: i64,
        label: &str,
        entity_type: &str,
        raw_entity_type: &str,
        description: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO ontology_nodes (context_id, label, entity_type, raw_entity_type, description)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![context_id, label, entity_type, raw_entity_type, description],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert an edge with its raw relation type as given. Returns the new id.
    pub fn insert_ontology_edge_exact(
        &self,
        context_id: i64,
        source_id: i64,
        target_id: i64,
        relation_type: &str,
        raw_relation_type: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO ontology_edges (context_id, source_id, target_id, relation_type, raw_relation_type)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![context_id, source_id, target_id, relation_type, raw_relation_type],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert a lens row as given (incl. `created_at`, which orders
    /// `list_lenses_for_context`). Returns the new id.
    pub fn insert_ontology_lens_exact(
        &self,
        context_id: i64,
        name: &str,
        ontology_profile_id: i64,
        is_extraction_lens: bool,
        created_at: i64,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO ontology_lenses (context_id, name, ontology_profile_id, is_extraction_lens, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![context_id, name, ontology_profile_id, is_extraction_lens, created_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert an edge review with every stored field (`attempts`,
    /// `created_at`); the denormalized labels of `r` are ignored.
    pub fn insert_ontology_edge_review_exact(&self, r: &OntologyEdgeReview) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_edge_reviews
                (context_id, edge_id, chunk_id, relation_type, evidence, reason, created_at, attempts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![r.context_id, r.edge_id, r.chunk_id, r.relation_type, r.evidence, r.reason, r.created_at, r.attempts],
        )?;
        Ok(())
    }

    /// Insert a merge-log row as given (incl. `merged_at`).
    pub fn insert_ontology_merge_log_exact(&self, e: &MergeLogEntry) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ontology_merge_log (context_id, winner_id, loser_id, loser_label, loser_entity_type, merged_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![e.context_id, e.winner_id, e.loser_id, e.loser_label, e.loser_entity_type, e.merged_at],
        )?;
        Ok(())
    }

    /// A node id that no live node has and none will ever get: a placeholder
    /// row is inserted and deleted again (`AUTOINCREMENT` never reuses an
    /// id). Stands in for a merged-away node in an imported merge log, just
    /// as the original id did on the source device.
    pub fn reserve_ontology_node_id(&self, context_id: i64) -> Result<i64> {
        let id = self.insert_ontology_node_exact(context_id, "", "", "", "")?;
        self.conn.execute("DELETE FROM ontology_nodes WHERE id = ?1", [id])?;
        Ok(id)
    }

    /// Insert one `chunk_refs` row as given (no parsing).
    pub fn insert_chunk_ref(&self, chunk_id: i64, context_id: i64, ref_key: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chunk_refs (chunk_id, context_id, ref_key) VALUES (?1, ?2, ?3)",
            params![chunk_id, context_id, ref_key],
        )?;
        Ok(())
    }

    /// Insert a page image without the replace-same-page step (and without a
    /// transaction of its own, unlike `insert_page_image`). Returns the new id.
    pub fn insert_page_image_exact(&self, p: &NewPageImage) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO page_images (context_id, document_id, page, char_start, char_end, width, height, image)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![p.context_id, p.document_id, p.page, p.char_start, p.char_end, p.width, p.height, p.image],
        )?;
        Ok(self.conn.last_insert_rowid())
    }
}

#[cfg(test)]
#[path = "transfer_tests.rs"]
mod transfer_tests;
