// matrix/core/src/lib.rs

pub mod chunking;
pub mod db;
pub mod embedding;
pub mod inference;
#[cfg(feature = "gguf")]
pub mod llama_backend;
pub mod refs;
pub mod secrets;
pub mod text_language;
pub mod text_match;
pub mod token_estimate;

pub use db::{models, CoreError, Database, Result};
