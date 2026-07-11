//! Local GGUF embedder running the llama.cpp backend (MODEL_INFRA_PLAN.md AP4b).
//! Compiled only under the `gguf` feature (Metal via `gguf-metal`).
//!
//! Counterpart to [`super::onnx::OrtEmbedder`], but a *distinct embedding space*
//! even for the identical model: the GGUF/quantized/llama.cpp representation of
//! jina-de is never comparable to its ONNX vectors, so a `local_gguf`
//! `EmbeddingModel` always carries its own id and contexts must be
//! re-vectorized when switched onto it (see HANDBUCH.md design decisions).
//!
//! Pipeline: llama.cpp tokenizes from the GGUF's own tokenizer (gpt2/jina-v2-de,
//! read from GGUF metadata) → encode (non-causal, `encode()` not `decode()`) →
//! llama.cpp applies the model's declared pooling (`pooling_type=1` = mean for
//! jina-de) so `embeddings_seq_ith` already returns the pooled vector →
//! optional Matryoshka truncation → L2-normalize (ourselves, for parity with
//! the ONNX path and store/query invariants).

use crate::db::models::EmbeddingModel;

/// Attention-masked **mean pool** of a `(seq_len, hidden)` row-major token
/// matrix. Pure, model-free, and unit-tested: llama.cpp normally does this
/// internally when `pooling_type=mean`, but we keep the reference implementation
/// here so the pooling contract is testable (and available as a fallback if a
/// GGUF ever declares `pooling_type=none`). `mask[t] == 0` tokens are excluded;
/// an all-zero mask yields a zero vector. Panics never: an empty/hidden-0 input
/// returns an empty vector.
pub fn mean_pool(token_states: &[f32], seq_len: usize, hidden: usize, mask: &[i64]) -> Vec<f32> {
    if hidden == 0 || seq_len == 0 {
        return vec![0.0; hidden];
    }
    let mut pooled = vec![0.0f32; hidden];
    let mut count = 0.0f32;
    for t in 0..seq_len {
        if mask.get(t).copied().unwrap_or(1) == 0 {
            continue;
        }
        count += 1.0;
        let base = t * hidden;
        for (h, p) in pooled.iter_mut().enumerate() {
            *p += token_states[base + h];
        }
    }
    if count > 0.0 {
        for p in pooled.iter_mut() {
            *p /= count;
        }
    }
    pooled
}

#[cfg(feature = "gguf")]
pub use imp::GgufEmbedder;

#[cfg(feature = "gguf")]
mod imp {
    use std::sync::Mutex;

    use std::num::NonZeroU32;

    use llama_cpp_2::context::params::{LlamaContextParams, LlamaPoolingType};
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::params::LlamaModelParams;
    use llama_cpp_2::model::{AddBos, LlamaModel};

    use super::EmbeddingModel;
    use crate::db::embeddings::l2_normalize;
    use crate::embedding::QueryEmbedder;
    use crate::{CoreError, Result};

    /// Shared llama.cpp backend init (mirrors `inference/gguf.rs`). The backend
    /// is a process-global singleton; both the LLM engine and this embedder may
    /// hold it concurrently.
    fn backend() -> Result<&'static LlamaBackend> {
        use std::sync::OnceLock;
        static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
        static INIT: Mutex<()> = Mutex::new(());
        if let Some(b) = BACKEND.get() {
            return Ok(b);
        }
        let _lock = INIT.lock().map_err(|_| CoreError::Embedding("backend init lock poisoned".into()))?;
        if let Some(b) = BACKEND.get() {
            return Ok(b);
        }
        let b = LlamaBackend::init()
            .map_err(|e| CoreError::Embedding(format!("llama backend init: {e}")))?;
        let _ = BACKEND.set(b);
        BACKEND
            .get()
            .ok_or_else(|| CoreError::Embedding("failed to set llama backend".into()))
    }

    /// Loaded GGUF embedder. Holds the model and, behind a mutex, a reusable
    /// embedding context (`Mutex` because llama.cpp contexts are `!Sync` and one
    /// embedder is shared/cached across queries).
    pub struct GgufEmbedder {
        model: LlamaModel,
        out_dim: usize,
        is_matryoshka: bool,
        n_ctx: u32,
    }

    impl GgufEmbedder {
        pub fn load(model: &EmbeddingModel) -> Result<Self> {
            let model_path = model
                .model_path
                .as_ref()
                .ok_or_else(|| CoreError::Embedding("local GGUF model_path missing".into()))?;
            if !std::path::Path::new(model_path).exists() {
                return Err(CoreError::Embedding(format!(
                    "GGUF model file not found: {model_path}"
                )));
            }
            let backend = backend()?;
            #[allow(unused_mut)]
            let mut params = LlamaModelParams::default().with_use_mmap(true);
            #[cfg(feature = "gguf-metal")]
            {
                params = params.with_n_gpu_layers(999);
            }
            let m = LlamaModel::load_from_file(backend, model_path, &params)
                .map_err(|e| CoreError::Embedding(format!("GGUF load {model_path}: {e}")))?;
            Ok(Self {
                model: m,
                out_dim: model.default_dim as usize,
                is_matryoshka: model.is_matryoshka,
                // jina-de trains at 8192; a fixed, generous embedding context is
                // fine — a single sequence is encoded per call.
                n_ctx: 8192,
            })
        }

        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            let backend = backend()?;
            // Non-causal encoder with the model's declared pooling (mean for
            // jina-de, `pooling_type=1` in GGUF metadata). Batch large enough for
            // one long sequence.
            let ctx_params = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(self.n_ctx))
                .with_n_batch(self.n_ctx)
                .with_embeddings(true)
                .with_pooling_type(LlamaPoolingType::Mean);
            let mut ctx = self
                .model
                .new_context(backend, ctx_params)
                .map_err(|e| CoreError::Embedding(format!("GGUF ctx create: {e}")))?;

            // GGUF carries its own tokenizer (add_bos/add_eos come from metadata).
            let tokens = self
                .model
                .str_to_token(text, AddBos::Always)
                .map_err(|e| CoreError::Embedding(format!("GGUF tokenize: {e}")))?;
            let n_tokens = tokens.len().min(self.n_ctx as usize);
            if n_tokens == 0 {
                return Ok(vec![0.0; self.out_dim.max(1)]);
            }

            let mut batch = LlamaBatch::new(self.n_ctx as usize, 1);
            for (i, &tok) in tokens.iter().take(n_tokens).enumerate() {
                // seq id 0; request logits/outputs so pooled embeddings are
                // produced (llama.cpp overrides to all-outputs for embeddings).
                batch
                    .add(tok, i as i32, &[0], true)
                    .map_err(|e| CoreError::Embedding(format!("batch add: {e}")))?;
            }

            // Non-causal → encode(), not decode() (matches the AP4a probe).
            ctx.encode(&mut batch)
                .map_err(|e| CoreError::Embedding(format!("GGUF encode: {e}")))?;

            // Pooled per-sequence embedding (mean pooling done by llama.cpp).
            let pooled = ctx
                .embeddings_seq_ith(0)
                .map_err(|e| CoreError::Embedding(format!("GGUF embeddings: {e}")))?;
            let mut out = pooled.to_vec();

            if self.is_matryoshka && self.out_dim > 0 && self.out_dim < out.len() {
                out.truncate(self.out_dim);
            }
            l2_normalize(&mut out);
            Ok(out)
        }
    }

    impl QueryEmbedder for GgufEmbedder {
        fn embed_query(&self, _model: &EmbeddingModel, query: &str) -> Result<Vec<f32>> {
            self.embed(query)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mean_pool;
    use crate::db::embeddings::l2_normalize;

    #[test]
    fn mean_pool_masks_and_averages() {
        // 3 tokens x 2 hidden; mask drops the 3rd token.
        let states = vec![
            1.0, 2.0, // t0
            3.0, 4.0, // t1
            100.0, 100.0, // t2 (masked out)
        ];
        let mask = vec![1i64, 1, 0];
        let pooled = mean_pool(&states, 3, 2, &mask);
        assert_eq!(pooled, vec![2.0, 3.0]); // mean of t0,t1 only
    }

    #[test]
    fn mean_pool_all_tokens_when_no_mask_zeros() {
        let states = vec![2.0, 0.0, 4.0, 0.0];
        let pooled = mean_pool(&states, 2, 2, &[1, 1]);
        assert_eq!(pooled, vec![3.0, 0.0]);
    }

    #[test]
    fn mean_pool_all_masked_yields_zero() {
        let states = vec![5.0, 6.0, 7.0, 8.0];
        let pooled = mean_pool(&states, 2, 2, &[0, 0]);
        assert_eq!(pooled, vec![0.0, 0.0]);
    }

    #[test]
    fn mean_pool_missing_mask_entries_default_included() {
        // Shorter mask than seq_len → the unspecified token counts as present.
        let states = vec![2.0, 4.0, 6.0];
        let pooled = mean_pool(&states, 3, 1, &[1]); // t1,t2 have no mask entry
        assert_eq!(pooled, vec![4.0]); // (2+4+6)/3
    }

    #[test]
    fn l2_normalize_unit_length() {
        let mut v = vec![3.0f32, 4.0];
        l2_normalize(&mut v);
        // 3-4-5 triangle → normalized to (0.6, 0.8)
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);
        let norm = (v[0] * v[0] + v[1] * v[1]).sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }
}
