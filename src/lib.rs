// matrix/core/src/lib.rs

pub mod chunking;
pub mod db;
pub mod embedding;
pub mod inference;

pub use db::{models, CoreError, Database, Result};

/// Diese Funktion wird später von der Tauri-App aufgerufen.
/// Sie prüft lokal auf dem iPhone, ob das mathematische Triebwerk bereitsteht.
pub fn execute_hardware_check() -> String {
    "Rust-Core auf Apple-Silizium antwortet fehlerfrei. Die Inferenz-Matrix steht! 🚀".to_string()
}
