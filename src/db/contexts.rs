//! CRUD for `contexts` and their `documents`.

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{Database, Result};

fn row_to_context(row: &Row<'_>) -> rusqlite::Result<Context> {
    Ok(Context {
        id: row.get("id")?,
        name: row.get("name")?,
        description: row.get("description")?,
        chunking_strategy: row.get("chunking_strategy")?,
        chunking_profile_id: row.get("chunking_profile_id")?,
        structural_profile_id: row.get("structural_profile_id")?,
        embedding_model_id: row.get("embedding_model_id")?,
        embedding_dim: row.get("embedding_dim")?,
        chunk_endpoint_id: row.get("chunk_endpoint_id")?,
        status: row.get("status")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_document(row: &Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: row.get("id")?,
        context_id: row.get("context_id")?,
        name: row.get("name")?,
        zip_entry: row.get("zip_entry")?,
        byte_size: row.get("byte_size")?,
        page_count: row.get("page_count")?,
        content_hash: row.get("content_hash")?,
        extracted_text: row.get("extracted_text")?,
        ingested_at: row.get("ingested_at")?,
    })
}

impl Database {
    // --- contexts ----------------------------------------------------------

    pub fn create_context(&self, c: &NewContext) -> Result<Context> {
        self.conn.execute(
            "INSERT INTO contexts
                (name, description, chunking_strategy, chunking_profile_id, structural_profile_id, embedding_model_id, embedding_dim, chunk_endpoint_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                c.name,
                c.description,
                c.chunking_strategy,
                c.chunking_profile_id,
                c.structural_profile_id,
                c.embedding_model_id,
                c.embedding_dim,
                c.chunk_endpoint_id,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self.context(id)?.expect("row just inserted must exist"))
    }

    pub fn context(&self, id: i64) -> Result<Option<Context>> {
        Ok(self
            .conn
            .query_row("SELECT * FROM contexts WHERE id = ?1", [id], row_to_context)
            .optional()?)
    }

    pub fn list_contexts(&self) -> Result<Vec<Context>> {
        let mut stmt = self.conn.prepare("SELECT * FROM contexts ORDER BY name")?;
        let rows = stmt.query_map([], row_to_context)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_context_status(&self, id: i64, status: ContextStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE contexts SET status = ?2, updated_at = unixepoch() WHERE id = ?1",
            params![id, status],
        )?;
        Ok(())
    }

    /// Persist a context's embedding dimension once it is known from the stored
    /// vectors (keeps the scope-pane badge correct without a manual dim field).
    pub fn set_context_embedding_dim(&self, id: i64, dim: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE contexts SET embedding_dim = ?2, updated_at = unixepoch() WHERE id = ?1",
            params![id, dim],
        )?;
        Ok(())
    }

    pub fn update_context(&self, id: i64, c: &NewContext) -> Result<Context> {
        self.conn.execute(
            "UPDATE contexts SET
                name = ?2, description = ?3, chunking_strategy = ?4, chunking_profile_id = ?5,
                structural_profile_id = ?6, embedding_model_id = ?7, embedding_dim = ?8, chunk_endpoint_id = ?9, updated_at = unixepoch()
             WHERE id = ?1",
            params![
                id,
                c.name,
                c.description,
                c.chunking_strategy,
                c.chunking_profile_id,
                c.structural_profile_id,
                c.embedding_model_id,
                c.embedding_dim,
                c.chunk_endpoint_id,
            ],
        )?;
        self.context(id)?
            .ok_or_else(|| super::CoreError::NotFound(format!("context {id}")))
    }

    pub fn delete_context(&self, id: i64) -> Result<bool> {
        self.conn.execute(
            "DELETE FROM grid_chat_results 
             WHERE row_ref_type = 'chunk' 
               AND row_ref_id IN (SELECT id FROM chunks WHERE context_id = ?1)",
            [id],
        )?;
        Ok(self
            .conn
            .execute("DELETE FROM contexts WHERE id = ?1", [id])?
            > 0)
    }

    // --- documents ---------------------------------------------------------

    pub fn create_document(&self, d: &NewDocument) -> Result<Document> {
        self.conn.execute(
            "INSERT INTO documents
                (context_id, name, zip_entry, byte_size, page_count, content_hash, extracted_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                d.context_id,
                d.name,
                d.zip_entry,
                d.byte_size,
                d.page_count,
                d.content_hash,
                d.extracted_text,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self.document(id)?.expect("row just inserted must exist"))
    }

    pub fn document(&self, id: i64) -> Result<Option<Document>> {
        Ok(self
            .conn
            .query_row("SELECT * FROM documents WHERE id = ?1", [id], row_to_document)
            .optional()?)
    }

    pub fn list_documents(&self, context_id: i64) -> Result<Vec<Document>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM documents WHERE context_id = ?1 ORDER BY name")?;
        let rows = stmt.query_map([context_id], row_to_document)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_document(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM documents WHERE id = ?1", [id])?
            > 0)
    }
}
