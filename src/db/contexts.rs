//! CRUD for `contexts` and their `documents`.
//!
//! `copy_context_chunks` duplicates a context's documents + chunks + graphics
//! page images (verbatim rows, new ids) into an empty target context — used to
//! embed the identical chunk set with a second embedding model for side-by-side
//! retrieval evals (`services/examples/embed_compare.rs`). Embeddings (text and
//! image), prechunks, chunk_refs and ontology rows are NOT copied.

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
        llm_id: row.get("llm_id")?,
        fallback_llm_id: row.get("fallback_llm_id")?,
        ontology_profile_id: row.get("ontology_profile_id")?,
        ontology_pool_id: row.get("ontology_pool_id")?,
        ontology_extract_llm_id: row.get("ontology_extract_llm_id")?,
        ontology_extract_pool_id: row.get("ontology_extract_pool_id")?,
        ontology_extract_reasoning_effort: row.get("ontology_extract_reasoning_effort")?,
        extract_title_llm: row.get("extract_title_llm")?,
        auto_merge_ontology: row.get("auto_merge_ontology")?,
        active_lens_id: row.get("active_lens_id")?,
        status: row.get("status")?,
        embed_images: row.get("embed_images")?,
        image_llm_id: row.get("image_llm_id")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_document(row: &Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: row.get("id")?,
        context_id: row.get("context_id")?,
        name: row.get("name")?,
        description: row.get("description")?,
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
                (name, description, chunking_strategy, chunking_profile_id, structural_profile_id, embedding_model_id, embedding_dim, llm_id, fallback_llm_id, ontology_profile_id, ontology_pool_id, ontology_extract_llm_id, ontology_extract_pool_id, ontology_extract_reasoning_effort, extract_title_llm, auto_merge_ontology, image_llm_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                c.name,
                c.description,
                c.chunking_strategy,
                c.chunking_profile_id,
                c.structural_profile_id,
                c.embedding_model_id,
                c.embedding_dim,
                c.llm_id,
                c.fallback_llm_id,
                c.ontology_profile_id,
                c.ontology_pool_id,
                c.ontology_extract_llm_id,
                c.ontology_extract_pool_id,
                c.ontology_extract_reasoning_effort,
                c.extract_title_llm,
                c.auto_merge_ontology,
                c.image_llm_id,
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

    /// Internal progress marker, written by the chunking/ingestion/embedding
    /// pipelines; deliberately not exposed as an IPC command or HTTP route.
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

    /// Whether the context embeds graphics pages automatically after a
    /// document's text chunks (schema_v61). Not part of `NewContext`, so
    /// `update_context` leaves it untouched.
    pub fn set_context_embed_images(&self, id: i64, on: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE contexts SET embed_images = ?2, updated_at = unixepoch() WHERE id = ?1",
            params![id, on],
        )?;
        Ok(())
    }

    pub fn update_context(&self, id: i64, c: &NewContext) -> Result<Context> {
        self.conn.execute(
            "UPDATE contexts SET
                name = ?2, description = ?3, chunking_strategy = ?4, chunking_profile_id = ?5,
                structural_profile_id = ?6, embedding_model_id = ?7, embedding_dim = ?8, llm_id = ?9, fallback_llm_id = ?10, ontology_profile_id = ?11, ontology_pool_id = ?12, ontology_extract_llm_id = ?13, ontology_extract_pool_id = ?14, ontology_extract_reasoning_effort = ?15, extract_title_llm = ?16, auto_merge_ontology = ?17, image_llm_id = ?18, updated_at = unixepoch()
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
                c.llm_id,
                c.fallback_llm_id,
                c.ontology_profile_id,
                c.ontology_pool_id,
                c.ontology_extract_llm_id,
                c.ontology_extract_pool_id,
                c.ontology_extract_reasoning_effort,
                c.extract_title_llm,
                c.auto_merge_ontology,
                c.image_llm_id,
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

    /// Set (or clear, with `None`/empty) a document's tool-manifest description
    /// (schema_v54, AP8). An empty string is normalized to `NULL` so the manifest
    /// builder can simply skip files without a description.
    pub fn set_document_description(&self, id: i64, description: Option<&str>) -> Result<bool> {
        let normalized = description.map(str::trim).filter(|s| !s.is_empty());
        Ok(self
            .conn
            .execute(
                "UPDATE documents SET description = ?2 WHERE id = ?1",
                params![id, normalized],
            )?
            > 0)
    }

    /// Rename a document and carry the new name into the denormalised
    /// `metadata.document_title` of its chunks (written at ingest/rechunk time and
    /// shown as the source title in answers) — otherwise a renamed document keeps
    /// answering under its old title. Returns `false` for an unknown id or a blank
    /// name. One transaction: either both places change or neither.
    pub fn rename_document(&self, id: i64, name: &str) -> Result<bool> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(false);
        }
        let tx = self.conn.unchecked_transaction()?;
        let changed = tx.execute("UPDATE documents SET name = ?2 WHERE id = ?1", params![id, name])?;
        if changed == 0 {
            return Ok(false);
        }
        tx.execute(
            "UPDATE chunks SET metadata = json_set(metadata, '$.document_title', ?2)
             WHERE document_id = ?1 AND json_valid(metadata)
               AND json_type(metadata) = 'object'",
            params![id, name],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn delete_document(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM documents WHERE id = ?1", [id])?
            > 0)
    }

    /// Copy all documents of context `src` with their chunks (text, offsets,
    /// signature, omitted flag, metadata) and page images (without vectors)
    /// into context `dst`, in one
    /// transaction. `dst` must have no documents yet (guards against a double
    /// copy). Returns the number of chunks copied. FTS rows follow via the
    /// `chunks_fts_*` triggers.
    pub fn copy_context_chunks(&self, src: i64, dst: i64) -> Result<usize> {
        if src == dst {
            return Err(super::CoreError::InvalidState("copy source and target are the same context".into()));
        }
        let tx = self.conn.unchecked_transaction()?;
        let existing: i64 =
            tx.query_row("SELECT COUNT(*) FROM documents WHERE context_id = ?1", [dst], |r| r.get(0))?;
        if existing > 0 {
            return Err(super::CoreError::InvalidState(format!(
                "target context {dst} already has {existing} document(s)"
            )));
        }
        let doc_ids: Vec<i64> = {
            let mut stmt = tx.prepare("SELECT id FROM documents WHERE context_id = ?1 ORDER BY id")?;
            let rows = stmt.query_map([src], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut copied = 0usize;
        for doc_id in doc_ids {
            tx.execute(
                "INSERT INTO documents
                    (context_id, name, zip_entry, byte_size, page_count, content_hash,
                     extracted_text, ingested_at, description)
                 SELECT ?2, name, zip_entry, byte_size, page_count, content_hash,
                        extracted_text, ingested_at, description
                 FROM documents WHERE id = ?1",
                params![doc_id, dst],
            )?;
            let new_doc = tx.last_insert_rowid();
            copied += tx.execute(
                "INSERT INTO chunks
                    (context_id, document_id, chunk_index, char_start, char_end, text,
                     signature, is_omitted, metadata, created_at)
                 SELECT ?2, ?3, chunk_index, char_start, char_end, text,
                        signature, is_omitted, metadata, created_at
                 FROM chunks WHERE document_id = ?1 AND context_id = ?4
                 ORDER BY chunk_index",
                params![doc_id, dst, new_doc, src],
            )?;
            tx.execute(
                "INSERT INTO page_images
                    (context_id, document_id, page, char_start, char_end, width, height, image, created_at)
                 SELECT ?2, ?3, page, char_start, char_end, width, height, image, created_at
                 FROM page_images WHERE document_id = ?1 ORDER BY page",
                params![doc_id, dst, new_doc],
            )?;
        }
        tx.commit()?;
        Ok(copied)
    }
}

#[cfg(test)]
#[path = "rename_tests.rs"]
mod rename_tests;

#[cfg(test)]
#[path = "context_copy_tests.rs"]
mod context_copy_tests;
