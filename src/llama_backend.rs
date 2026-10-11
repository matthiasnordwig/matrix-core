//! Process-global llama.cpp backend singleton, shared by every GGUF consumer
//! (LLM engine `inference/gguf.rs`, embedder `embedding/gguf_embed.rs`,
//! reranker `embedding/gguf_rerank.rs`).
//!
//! llama.cpp allows exactly one backend init per process. Three module-local
//! `OnceLock` singletons each calling `LlamaBackend::init()` meant whichever
//! module initialized *second* failed with `BackendAlreadyInitialized` (seen
//! in the wild: local-LLM grid run first, GGUF query embedding afterwards).
//! All modules must go through this one accessor.

use std::sync::{Mutex, OnceLock};

use llama_cpp_2::llama_backend::LlamaBackend;

static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
static INIT: Mutex<()> = Mutex::new(());

/// Returns the process-wide llama.cpp backend, initializing it on first use.
/// Callers map the `String` error into their own error type.
pub fn backend() -> Result<&'static LlamaBackend, String> {
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let _lock = INIT
        .lock()
        .map_err(|_| "llama backend init lock poisoned".to_string())?;
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let b = LlamaBackend::init().map_err(|e| format!("llama backend init: {e}"))?;
    let _ = BACKEND.set(b);
    BACKEND
        .get()
        .ok_or_else(|| "failed to set llama backend".to_string())
}

#[cfg(test)]
mod tests {
    use super::backend;

    /// Regression for `BackendAlreadyInitialized`: repeated calls — as issued
    /// by the LLM engine, embedder and reranker within one process — must all
    /// yield the same shared backend instead of re-running `LlamaBackend::init`.
    #[test]
    fn backend_is_reusable_across_callers() {
        let first = backend().expect("first init");
        let second = backend().expect("second call must reuse, not re-init");
        assert!(std::ptr::eq(first, second));
    }
}
