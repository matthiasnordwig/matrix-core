//! Key/value application settings. Values are stored JSON-encoded so any
//! serde type round-trips through the same two methods.

use rusqlite::{params, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::{Database, Result};

/// Setting key: filesystem path to the local ONNX reranker model dir (must
/// contain `model.onnx` + `tokenizer.json`). Empty/unset = reranker OFF.
/// Resolved against `dirs.model_dirs` the same way local embedder/gguf dirs are
/// (absolute as-is, `~/`-relative against `$HOME`, else the last model dir).
pub const KEY_RERANKER_MODEL_DIR: &str = "reranker_model_dir";

/// Setting key: default state of the Chat/Grid "Rerank" toggle (bool).
pub const KEY_RERANKER_ENABLED_DEFAULT: &str = "reranker_enabled_default";

impl Database {
    /// Read and deserialize a setting; `None` if the key is absent.
    pub fn get_setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT value FROM app_settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        match raw {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    /// Serialize and upsert a setting.
    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let json = serde_json::to_string(value)?;
        self.conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, json],
        )?;
        Ok(())
    }
}
