//! CRUD for the model registries: `embedding_models` and `llm_endpoints`.

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{Database, Result};

fn row_to_embedding_model(row: &Row<'_>) -> rusqlite::Result<EmbeddingModel> {
    Ok(EmbeddingModel {
        id: row.get("id")?,
        identifier: row.get("identifier")?,
        kind: row.get("kind")?,
        model_path: row.get("model_path")?,
        tokenizer_path: row.get("tokenizer_path")?,
        api_config: row.get("api_config")?,
        execution_provider: row.get("execution_provider")?,
        is_matryoshka: row.get("is_matryoshka")?,
        native_dim: row.get("native_dim")?,
        default_dim: row.get("default_dim")?,
        normalize: row.get("normalize")?,
        tpm_limit: row.get("tpm_limit")?,
        rpm_limit: row.get("rpm_limit")?,
        max_concurrency: row.get("max_concurrency")?,
        created_at: row.get("created_at")?,
    })
}

fn row_to_reranker_model(row: &Row<'_>) -> rusqlite::Result<RerankerModel> {
    Ok(RerankerModel {
        id: row.get("id")?,
        name: row.get("name")?,
        kind: row.get("kind")?,
        model_dir: row.get("model_dir")?,
        api_config: row.get("api_config")?,
        execution_provider: row.get("execution_provider")?,
        created_at: row.get("created_at")?,
    })
}

pub(super) fn row_to_llm_endpoint(row: &Row<'_>) -> rusqlite::Result<LlmEndpoint> {
    Ok(LlmEndpoint {
        id: row.get("id")?,
        name: row.get("name")?,
        base_url: row.get("base_url")?,
        model_id: row.get("model_id")?,
        api_key_ref: row.get("api_key_ref")?,
        timeout_ms: row.get("timeout_ms")?,
        max_retries: row.get("max_retries")?,
        provider: row.get("provider")?,
        window_tokens: row.get("window_tokens")?,
        context_window: row.get("context_window")?,
        output_reserve_tokens: row.get("output_reserve_tokens")?,
        tpm_limit: row.get("tpm_limit")?,
        rpm_limit: row.get("rpm_limit")?,
        max_concurrency: row.get("max_concurrency")?,
        is_reasoning: row.get("is_reasoning")?,
        supports_structured_output: row.get("supports_structured_output")?,
        supports_tools: row.get("supports_tools")?,
        stream_fallback: row.get("stream_fallback")?,
        kv_quantization: row.get("kv_quantization")?,
        cpu_threads: row.get("cpu_threads")?,
        reasoning_list_id: row.get("reasoning_list_id")?,
        created_at: row.get("created_at")?,
    })
}

impl Database {
    // --- embedding_models --------------------------------------------------

    pub fn create_embedding_model(&self, m: &NewEmbeddingModel) -> Result<EmbeddingModel> {
        self.conn.execute(
            "INSERT INTO embedding_models
                (identifier, kind, model_path, tokenizer_path, api_config,
                 execution_provider, is_matryoshka, native_dim, default_dim, normalize,
                 tpm_limit, rpm_limit, max_concurrency)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                m.identifier,
                m.kind,
                m.model_path,
                m.tokenizer_path,
                m.api_config,
                m.execution_provider,
                m.is_matryoshka,
                m.native_dim,
                m.default_dim,
                m.normalize,
                m.tpm_limit,
                m.rpm_limit,
                m.max_concurrency,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self
            .embedding_model(id)?
            .expect("row just inserted must exist"))
    }

    pub fn embedding_model(&self, id: i64) -> Result<Option<EmbeddingModel>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM embedding_models WHERE id = ?1",
                [id],
                row_to_embedding_model,
            )
            .optional()?)
    }

    pub fn list_embedding_models(&self) -> Result<Vec<EmbeddingModel>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM embedding_models ORDER BY identifier")?;
        let rows = stmt.query_map([], row_to_embedding_model)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn update_embedding_model(
        &self,
        id: i64,
        m: &NewEmbeddingModel,
    ) -> Result<EmbeddingModel> {
        self.conn.execute(
            "UPDATE embedding_models SET
                identifier = ?2, kind = ?3, model_path = ?4, tokenizer_path = ?5,
                api_config = ?6, execution_provider = ?7, is_matryoshka = ?8,
                native_dim = ?9, default_dim = ?10, normalize = ?11,
                tpm_limit = ?12, rpm_limit = ?13, max_concurrency = ?14
             WHERE id = ?1",
            params![
                id,
                m.identifier,
                m.kind,
                m.model_path,
                m.tokenizer_path,
                m.api_config,
                m.execution_provider,
                m.is_matryoshka,
                m.native_dim,
                m.default_dim,
                m.normalize,
                m.tpm_limit,
                m.rpm_limit,
                m.max_concurrency,
            ],
        )?;
        self.embedding_model(id)?
            .ok_or_else(|| super::CoreError::NotFound(format!("embedding_model {id}")))
    }

    /// Self-healing dimension: once a model's true embedding length is known (from
    /// the first produced vector), persist it so the UI never asks for it by hand.
    /// Sets both native and default dim (we do not Matryoshka-truncate by default).
    pub fn set_embedding_model_dim(&self, id: i64, dim: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE embedding_models SET native_dim = ?2, default_dim = ?2 WHERE id = ?1",
            params![id, dim],
        )?;
        Ok(())
    }

    pub fn delete_embedding_model(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM embedding_models WHERE id = ?1", [id])?
            > 0)
    }

    // --- reranker_models (MODEL_INFRA_PLAN.md AP2) --------------------------

    pub fn create_reranker_model(&self, m: &NewRerankerModel) -> Result<RerankerModel> {
        self.conn.execute(
            "INSERT INTO reranker_models (name, kind, model_dir, api_config, execution_provider)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![m.name, m.kind, m.model_dir, m.api_config, m.execution_provider],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self.reranker_model(id)?.expect("row just inserted must exist"))
    }

    pub fn reranker_model(&self, id: i64) -> Result<Option<RerankerModel>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM reranker_models WHERE id = ?1",
                [id],
                row_to_reranker_model,
            )
            .optional()?)
    }

    pub fn list_reranker_models(&self) -> Result<Vec<RerankerModel>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM reranker_models ORDER BY name")?;
        let rows = stmt.query_map([], row_to_reranker_model)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn update_reranker_model(&self, id: i64, m: &NewRerankerModel) -> Result<RerankerModel> {
        self.conn.execute(
            "UPDATE reranker_models SET
                name = ?2, kind = ?3, model_dir = ?4, api_config = ?5, execution_provider = ?6
             WHERE id = ?1",
            params![id, m.name, m.kind, m.model_dir, m.api_config, m.execution_provider],
        )?;
        self.reranker_model(id)?
            .ok_or_else(|| super::CoreError::NotFound(format!("reranker_model {id}")))
    }

    /// Delete a reranker. If it was the active one, clears `active_reranker_id`
    /// so the setting never dangles at a deleted id (reranker falls back to OFF).
    pub fn delete_reranker_model(&self, id: i64) -> Result<bool> {
        let deleted = self
            .conn
            .execute("DELETE FROM reranker_models WHERE id = ?1", [id])?
            > 0;
        if deleted {
            if let Ok(Some(active)) =
                self.get_setting::<i64>(super::settings::KEY_ACTIVE_RERANKER_ID)
            {
                if active == id {
                    self.conn.execute(
                        "DELETE FROM app_settings WHERE key = ?1",
                        [super::settings::KEY_ACTIVE_RERANKER_ID],
                    )?;
                }
            }
        }
        Ok(deleted)
    }

    /// The active reranker row (via `active_reranker_id`), or `None` when unset
    /// or pointing at a deleted row (reranker OFF).
    pub fn active_reranker_model(&self) -> Result<Option<RerankerModel>> {
        match self.get_setting::<i64>(super::settings::KEY_ACTIVE_RERANKER_ID)? {
            Some(id) => self.reranker_model(id),
            None => Ok(None),
        }
    }

    // --- llm_endpoints -----------------------------------------------------

    pub fn create_llm_endpoint(&self, e: &NewLlmEndpoint) -> Result<LlmEndpoint> {
        self.conn.execute(
            "INSERT INTO llm_endpoints
                (name, base_url, model_id, api_key_ref, timeout_ms, max_retries, provider,
                 window_tokens, context_window, output_reserve_tokens, tpm_limit, rpm_limit, max_concurrency, is_reasoning, supports_structured_output, supports_tools, stream_fallback, kv_quantization, cpu_threads, reasoning_list_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
            params![
                e.name,
                e.base_url,
                e.model_id,
                e.api_key_ref,
                e.timeout_ms,
                e.max_retries,
                e.provider,
                e.window_tokens,
                e.context_window,
                e.output_reserve_tokens,
                e.tpm_limit,
                e.rpm_limit,
                e.max_concurrency,
                e.is_reasoning,
                e.supports_structured_output,
                e.supports_tools,
                e.stream_fallback,
                e.kv_quantization,
                e.cpu_threads,
                e.reasoning_list_id,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self
            .llm_endpoint(id)?
            .expect("row just inserted must exist"))
    }

    pub fn llm_endpoint(&self, id: i64) -> Result<Option<LlmEndpoint>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM llm_endpoints WHERE id = ?1",
                [id],
                row_to_llm_endpoint,
            )
            .optional()?)
    }

    pub fn list_llm_endpoints(&self) -> Result<Vec<LlmEndpoint>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM llm_endpoints ORDER BY name")?;
        let rows = stmt.query_map([], row_to_llm_endpoint)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn update_llm_endpoint(&self, id: i64, e: &NewLlmEndpoint) -> Result<LlmEndpoint> {
        self.conn.execute(
            "UPDATE llm_endpoints SET
                name = ?2, base_url = ?3, model_id = ?4, api_key_ref = ?5,
                timeout_ms = ?6, max_retries = ?7, provider = ?8, window_tokens = ?9,
                context_window = ?10, output_reserve_tokens = ?11, tpm_limit = ?12,
                rpm_limit = ?13, max_concurrency = ?14, is_reasoning = ?15, supports_structured_output = ?16,
                supports_tools = ?17, stream_fallback = ?18, kv_quantization = ?19, cpu_threads = ?20, reasoning_list_id = ?21
             WHERE id = ?1",
            params![
                id,
                e.name,
                e.base_url,
                e.model_id,
                e.api_key_ref,
                e.timeout_ms,
                e.max_retries,
                e.provider,
                e.window_tokens,
                e.context_window,
                e.output_reserve_tokens,
                e.tpm_limit,
                e.rpm_limit,
                e.max_concurrency,
                e.is_reasoning,
                e.supports_structured_output,
                e.supports_tools,
                e.stream_fallback,
                e.kv_quantization,
                e.cpu_threads,
                e.reasoning_list_id,
            ],
        )?;
        self.llm_endpoint(id)?
            .ok_or_else(|| super::CoreError::NotFound(format!("llm_endpoint {id}")))
    }

    pub fn delete_llm_endpoint(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM llm_endpoints WHERE id = ?1", [id])?
            > 0)
    }
}
