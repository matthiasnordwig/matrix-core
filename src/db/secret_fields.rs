//! Which registry fields hold API keys, and their sealing on write / opening on
//! read (`crate::secrets` does the cryptography).
//!
//! Fields: `llm_endpoints.api_key_ref` (the whole value),
//! `embedding_models.api_config` → only the JSON field `api_key`,
//! `reranker_models.api_config` → only the JSON field `key_ref` (it carries the
//! key inline). The rest of the JSON stays readable. Each field has its own
//! associated data (`AAD_*`), so a value moved to another field won't decrypt.
//!
//! Rules: with a key set (`set_secret_key`), every write seals a plaintext
//! value; a value already starting with `enc:` is stored unchanged (no double
//! encryption, and an undecryptable value survives a form round trip). Every
//! read opens `enc:` values; if that fails (no key, replaced key) the raw
//! `enc:` string stays in place — never silently empty — and the services
//! helper (`matrix_services::secrets`) refuses to send it. Without a key
//! (Python wheel, tests) reads and writes are unchanged plaintext.
//! `registries.rs` and `pools.rs` route every row through here.

use std::path::Path;

use rusqlite::params;
use serde_json::Value;

use super::models::{EmbeddingModel, LlmEndpoint, RerankerModel};
use super::{Database, Result};
use crate::secrets::{self, SecretKey};

pub const AAD_LLM_KEY: &str = "llm_endpoints.api_key_ref";
pub const AAD_EMBED_KEY: &str = "embedding_models.api_config.api_key";
pub const AAD_RERANK_KEY: &str = "reranker_models.api_config.key_ref";
const EMBED_JSON_FIELD: &str = "api_key";
const RERANK_JSON_FIELD: &str = "key_ref";

fn seal(key: Option<&SecretKey>, aad: &str, v: &str) -> Option<String> {
    let key = key?;
    (!v.is_empty() && !secrets::is_encrypted(v)).then(|| secrets::encrypt(key, aad, v))
}

fn open(key: Option<&SecretKey>, aad: &str, v: &str) -> Option<String> {
    if !secrets::is_encrypted(v) {
        return None;
    }
    secrets::decrypt(key?, aad, v).ok()
}

/// Apply `f` to the string at `field` of a JSON object; `None` when nothing
/// changed (not JSON, not an object, field absent / not a string, `f` = None).
fn map_json_field(
    json: &str,
    field: &str,
    f: impl FnOnce(&str) -> Option<String>,
) -> Option<String> {
    let mut v: Value = serde_json::from_str(json).ok()?;
    let slot = v.as_object_mut()?.get_mut(field)?;
    let new = f(slot.as_str()?)?;
    *slot = Value::String(new);
    serde_json::to_string(&v).ok()
}

impl Database {
    /// Set (or clear) the key used to seal/open API keys. Call right after
    /// `open`, before the handle is shared; see `enable_secrets`.
    pub fn set_secret_key(&mut self, key: Option<SecretKey>) {
        self.secret_key = key;
    }

    pub fn has_secret_key(&self) -> bool {
        self.secret_key.is_some()
    }

    /// Load (or create) `dir/secrets.key`, set it, and convert stored plaintext
    /// keys. Returns the number of converted values.
    pub fn enable_secrets(&mut self, dir: &Path) -> Result<usize> {
        self.set_secret_key(Some(secrets::load_or_create(dir)?));
        self.encrypt_plaintext_secrets()
    }

    pub(super) fn seal_value(&self, aad: &str, v: Option<&str>) -> Option<String> {
        let v = v?;
        Some(seal(self.secret_key.as_ref(), aad, v).unwrap_or_else(|| v.to_string()))
    }

    fn seal_json(&self, aad: &str, field: &str, json: Option<&str>) -> Option<String> {
        let json = json?;
        let key = self.secret_key.as_ref();
        Some(map_json_field(json, field, |s| seal(key, aad, s)).unwrap_or_else(|| json.to_string()))
    }

    pub(super) fn seal_embed_config(&self, json: Option<&str>) -> Option<String> {
        self.seal_json(AAD_EMBED_KEY, EMBED_JSON_FIELD, json)
    }

    pub(super) fn seal_rerank_config(&self, json: Option<&str>) -> Option<String> {
        self.seal_json(AAD_RERANK_KEY, RERANK_JSON_FIELD, json)
    }

    fn open_json(&self, aad: &str, field: &str, json: &mut Option<String>) {
        let key = self.secret_key.as_ref();
        if let Some(new) = json.as_deref().and_then(|j| map_json_field(j, field, |s| open(key, aad, s))) {
            *json = Some(new);
        }
    }

    pub(super) fn open_llm_endpoint(&self, mut e: LlmEndpoint) -> LlmEndpoint {
        let key = self.secret_key.as_ref();
        if let Some(plain) = e.api_key_ref.as_deref().and_then(|v| open(key, AAD_LLM_KEY, v)) {
            e.api_key_ref = Some(plain);
        }
        e
    }

    pub(super) fn open_embedding_model(&self, mut m: EmbeddingModel) -> EmbeddingModel {
        self.open_json(AAD_EMBED_KEY, EMBED_JSON_FIELD, &mut m.api_config);
        m
    }

    pub(super) fn open_reranker_model(&self, mut m: RerankerModel) -> RerankerModel {
        self.open_json(AAD_RERANK_KEY, RERANK_JSON_FIELD, &mut m.api_config);
        m
    }

    /// One-time, idempotent conversion of stored plaintext keys to `enc:`
    /// values, in one transaction. No key set → nothing to do (`Ok(0)`).
    pub fn encrypt_plaintext_secrets(&self) -> Result<usize> {
        if self.secret_key.is_none() {
            return Ok(0);
        }
        let tx = self.conn.unchecked_transaction()?;
        let mut n = 0;
        type Convert<'a> = &'a dyn Fn(&str) -> Option<String>;
        let jobs: [(&str, &str, Convert); 3] = [
            ("llm_endpoints", "api_key_ref", &|v| seal(self.secret_key.as_ref(), AAD_LLM_KEY, v)),
            ("embedding_models", "api_config", &|v| {
                map_json_field(v, EMBED_JSON_FIELD, |s| seal(self.secret_key.as_ref(), AAD_EMBED_KEY, s))
            }),
            ("reranker_models", "api_config", &|v| {
                map_json_field(v, RERANK_JSON_FIELD, |s| seal(self.secret_key.as_ref(), AAD_RERANK_KEY, s))
            }),
        ];
        for (table, col, convert) in jobs {
            let rows: Vec<(i64, String)> = {
                let mut stmt = tx.prepare(&format!(
                    "SELECT id, {col} FROM {table} WHERE {col} IS NOT NULL AND {col} <> ''"
                ))?;
                stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<_>>()?
            };
            for (id, v) in rows {
                if let Some(sealed) = convert(&v) {
                    tx.execute(&format!("UPDATE {table} SET {col} = ?2 WHERE id = ?1"), params![id, sealed])?;
                    n += 1;
                }
            }
        }
        tx.commit()?;
        Ok(n)
    }
}

#[cfg(test)]
#[path = "secret_fields_tests.rs"]
mod secret_fields_tests;
