//! Embedding & retrieval.
//!
//! Core owns the *retrieval merge* (group by embedding space → per-space
//! brute-force cosine → merge) and the [`QueryEmbedder`] trait. Concrete
//! embedders that actually run a model are pluggable:
//! - the local ONNX/ANE embedder lives behind the `onnx` cargo feature
//!   ([`onnx`]); it needs the ONNX Runtime iOS static libs + model files.
//! - a remote-API embedder is provided by the Tauri bridge.
//!
//! Crucially, there is **no global cross-model vector**: queries are embedded
//! separately per embedding space and only the *scores* are merged.

use crate::db::models::EmbeddingModel;
use crate::Result;

pub mod retrieval;

/// Cross-encoder reranker (AP3). `rank_merge` (pure) is always compiled;
/// `OrtReranker` is behind the `onnx` feature.
pub mod rerank;

#[cfg(feature = "onnx")]
pub mod onnx;

/// Local GGUF/llama.cpp embedder (MODEL_INFRA_PLAN.md AP4b). The pure
/// pooling/normalize helpers compile unconditionally (and are tested); the
/// `GgufEmbedder` itself is behind the `gguf` feature.
pub mod gguf_embed;

/// Local GGUF/llama.cpp cross-encoder reranker (RERANKER_PERF_PLAN.md Phase 2).
/// The pure pair-token layout (`build_pair_tokens`) compiles unconditionally
/// (and is tested); the `GgufReranker` itself is behind the `gguf` feature.
pub mod gguf_rerank;

/// Produces a query vector in a specific model's embedding space. Implemented
/// by concrete embedders (local ONNX, remote API, or a test fake).
pub trait QueryEmbedder {
    fn embed_query(&self, model: &EmbeddingModel, query: &str) -> Result<Vec<f32>>;
}

#[cfg(test)]
mod tests;
