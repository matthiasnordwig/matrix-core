//! Local GGUF/llama.cpp cross-encoder reranker (RERANKER_PERF_PLAN.md Phase 2).
//! Compiled only under the `gguf` feature (Metal via `gguf-metal`).
//!
//! Counterpart to [`super::rerank::OrtReranker`] (ONNX), but running the
//! bge-reranker-v2-m3 GGUF on the llama.cpp backend with **Rank pooling**. A
//! (query, document) pair goes in, a single raw rank logit comes out (higher =
//! more relevant) — same score semantics as the ONNX reranker (`num_labels=1`),
//! so `rank::rank_merge` orders by it directly with no normalization.
//!
//! ## Model / pair format
//! bge-reranker-v2-m3 is an XLM-RoBERTa backbone (bert arch in GGUF) with a
//! classification head (`n_cls_out=1`). The reranker pair is encoded as the
//! XLM-R sequence-pair `<s> query </s> </s> document </s>`, built from the GGUF's
//! own tokenizer (its bos/eos come from GGUF metadata: `<s>`=0, `</s>`=2).
//!
//! ## FFI caveat (the reason this is not `embeddings_seq_ith`)
//! `llama-cpp-2` 0.1.146's safe `LlamaContext::embeddings_seq_ith(seq)` slices
//! `model.n_embd()` (=1024) floats over the pointer returned by
//! `llama_get_embeddings_seq`. But under `LLAMA_POOLING_TYPE_RANK` llama.cpp
//! resizes that sequence's embedding buffer to `n_cls_out` (=1) float and copies
//! only 1 float (verified in `llama.cpp/src/llama-context.cpp`, the
//! `LLAMA_POOLING_TYPE_RANK` arm: `embd_seq_out[seq_id].resize(n_cls_out)`). The
//! returned buffer is a `std::vector<float>` of length 1, so building a 1024-len
//! slice over it is an out-of-bounds read (UB / heap overread). We therefore call
//! `llama_cpp_sys_2::llama_get_embeddings_seq` directly and read **exactly one**
//! float (see `read_rank_score` below).

/// Build the XLM-R reranker pair token id sequence for
/// `<s> query </s> </s> document </s>` from separately-tokenized query and
/// document token id vectors. Pure (no model), so the pair layout is unit-tested.
///
/// `query_with_specials` / `doc_with_specials` are the token ids produced by the
/// GGUF tokenizer with add_bos/add_eos ON, i.e. each is `<s> … </s>`. The result
/// is the XLM-R sequence-pair `<s> query </s> </s> document </s>`: the query is
/// kept verbatim (its leading `<s>` and trailing `</s>`), the document's leading
/// `<s>` is **replaced by** a second `</s>` (giving the `</s> </s>` segment
/// boundary XLM-R expects), and the document's own trailing `</s>` is kept.
pub fn build_pair_tokens(query_with_specials: &[i32], doc_with_specials: &[i32]) -> Vec<i32> {
    // The `</s>` id is whatever the tokenizer put at the end of the query (bge-m3:
    // 2). We reuse it as the segment-boundary separator so this stays tokenizer-
    // agnostic instead of hard-coding 2.
    let sep = query_with_specials.last().copied().unwrap_or(2);
    let mut out = Vec::with_capacity(query_with_specials.len() + doc_with_specials.len() + 1);
    out.extend_from_slice(query_with_specials); // <s> q… </s>
    out.push(sep); // second </s> (segment boundary)
    if doc_with_specials.len() > 1 {
        out.extend_from_slice(&doc_with_specials[1..]); // d… </s> (drop doc's <s>)
    } else {
        // Degenerate doc (only <s>, or empty) — append verbatim; nothing to drop.
        out.extend_from_slice(doc_with_specials);
    }
    out
}

#[cfg(feature = "gguf")]
pub use imp::GgufReranker;

#[cfg(feature = "gguf")]
mod imp {
    use std::num::NonZeroU32;
    use std::sync::Mutex;

    use llama_cpp_2::context::params::{LlamaContextParams, LlamaPoolingType};
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::params::LlamaModelParams;
    use llama_cpp_2::model::{AddBos, LlamaModel};
    use llama_cpp_2::token::LlamaToken;

    use super::build_pair_tokens;
    use crate::{CoreError, Result};

    /// XLM-R effective max sequence ≈ 1024 (positions offset by 2). Truncate each
    /// pair to fit; the caller already caps the doc snippet upstream (Phase 1).
    const MAX_SEQ_LEN: usize = 1024;

    /// Shared llama.cpp backend init (mirrors `gguf_embed.rs`/`inference/gguf.rs`).
    /// Process-global singleton; the LLM engine, GGUF embedder and this reranker
    /// may all hold it concurrently.
    fn backend() -> Result<&'static LlamaBackend> {
        use std::sync::OnceLock;
        static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
        static INIT: Mutex<()> = Mutex::new(());
        if let Some(b) = BACKEND.get() {
            return Ok(b);
        }
        let _lock = INIT
            .lock()
            .map_err(|_| CoreError::Embedding("backend init lock poisoned".into()))?;
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

    /// Loaded GGUF reranker. Holds the model; a fresh context is created per
    /// `score_pairs` call (like `gguf_embed.rs`), since a rank/embedding context
    /// is cheap relative to the encode and this keeps state per-scoring clean.
    pub struct GgufReranker {
        model: LlamaModel,
        n_ctx: u32,
    }

    impl GgufReranker {
        /// Load a `.gguf` reranker from `model_path`.
        pub fn load(model_path: &str) -> Result<Self> {
            if !std::path::Path::new(model_path).exists() {
                return Err(CoreError::Embedding(format!(
                    "GGUF reranker file not found: {model_path}"
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
                .map_err(|e| CoreError::Embedding(format!("GGUF reranker load {model_path}: {e}")))?;
            Ok(Self { model: m, n_ctx: MAX_SEQ_LEN as u32 })
        }

        /// Score each (query, doc) pair; returns one raw rank logit per doc, in
        /// `docs` order. Higher = more relevant. Empty `docs` → empty vec.
        pub fn score_pairs(&self, query: &str, docs: &[&str]) -> Result<Vec<f32>> {
            if docs.is_empty() {
                return Ok(Vec::new());
            }
            let backend = backend()?;
            // Rank pooling context: embeddings ON, pooling = Rank so llama.cpp
            // runs the cls head and stores the single rank logit per sequence.
            // n_ubatch (physical micro-batch) must cover a full pair: `encode()`
            // hard-asserts `n_ubatch >= n_tokens`. Its default is 512, so a pair
            // longer than 512 tokens (possible up to MAX_SEQ_LEN=1024 when the
            // caller feeds long/uncapped doc text) would abort the process
            // instead of scoring. Production caps doc text to 512 chars
            // (RERANK_SNIPPET_CAP_CHARS) so pairs stay short, but sizing ubatch
            // to n_ctx makes long pairs safe too. Mirrors `gguf_embed.rs`.
            let ctx_params = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(self.n_ctx))
                .with_n_batch(self.n_ctx)
                .with_n_ubatch(self.n_ctx)
                .with_embeddings(true)
                .with_pooling_type(LlamaPoolingType::Rank);
            let mut ctx = self
                .model
                .new_context(backend, ctx_params)
                .map_err(|e| CoreError::Embedding(format!("GGUF rerank ctx create: {e}")))?;

            // Tokenize the query once (with the model's own bos/eos), reuse per doc.
            let query_tokens: Vec<i32> = self
                .model
                .str_to_token(query, AddBos::Always)
                .map_err(|e| CoreError::Embedding(format!("GGUF rerank tokenize query: {e}")))?
                .into_iter()
                .map(|t| t.0)
                .collect();

            let mut out: Vec<f32> = Vec::with_capacity(docs.len());
            for &doc in docs {
                let doc_tokens: Vec<i32> = self
                    .model
                    .str_to_token(doc, AddBos::Always)
                    .map_err(|e| CoreError::Embedding(format!("GGUF rerank tokenize doc: {e}")))?
                    .into_iter()
                    .map(|t| t.0)
                    .collect();
                let mut pair = build_pair_tokens(&query_tokens, &doc_tokens);
                if pair.len() > MAX_SEQ_LEN {
                    // Keep the head (query + start of doc); the trailing </s> is
                    // then implicitly dropped, which is acceptable for scoring.
                    pair.truncate(MAX_SEQ_LEN);
                }
                out.push(self.score_one_pair(&mut ctx, &pair)?);
            }
            Ok(out)
        }

        /// Encode one pair (single sequence, seq id 0) and read its rank score.
        fn score_one_pair(
            &self,
            ctx: &mut llama_cpp_2::context::LlamaContext,
            pair: &[i32],
        ) -> Result<f32> {
            let mut batch = LlamaBatch::new(self.n_ctx as usize, 1);
            for (i, &tok) in pair.iter().enumerate() {
                // Mark EVERY token as an output (`true`): with pooling on, llama.cpp
                // requires all input tokens to carry outputs (it otherwise logs
                // "embeddings required but some input tokens were not marked as
                // outputs -> overriding" and can crash). Mirrors `gguf_embed.rs`.
                batch
                    .add(LlamaToken(tok), i as i32, &[0], true)
                    .map_err(|e| CoreError::Embedding(format!("GGUF rerank batch add: {e}")))?;
            }
            // Non-causal encoder path (matches the GGUF embedder).
            ctx.encode(&mut batch)
                .map_err(|e| CoreError::Embedding(format!("GGUF rerank encode: {e}")))?;
            read_rank_score(ctx)
        }
    }

    /// Read the single Rank-pooling score for sequence 0, reading **exactly one**
    /// float (see the module-level FFI caveat).
    ///
    /// `llama-cpp-2` 0.1.146 gives no way to obtain the raw `*mut llama_context`
    /// (the field is `pub(crate)`, and its `#[repr(Rust)]` struct layout is not
    /// guaranteed — reading "the first field" as a pointer crashes, the compiler
    /// reorders fields). The only public accessor is `embeddings_seq_ith(0)`,
    /// which returns `Ok(&[f32])` where **the data pointer is exactly**
    /// `llama_get_embeddings_seq(ctx, 0)` — the correct pointer — but whose
    /// *length* is `model.n_embd()` (=1024). Under Rank pooling llama.cpp only
    /// wrote `n_cls_out` (=1) float there, so the slice's length is wrong (a
    /// 1024-len view over a 1-float buffer). We therefore take **only the data
    /// pointer** from that slice and dereference index 0 — we never read the
    /// slice at any index ≥ 1, so no out-of-bounds access happens at runtime.
    fn read_rank_score(ctx: &llama_cpp_2::context::LlamaContext) -> Result<f32> {
        // `embeddings_seq_ith` returns Err(NonePoolType) if the pooled buffer is
        // null; propagate that as our own error. Its Ok slice has the correct
        // data pointer (the raw `llama_get_embeddings_seq` result); its reported
        // length (n_embd) is not trustworthy under Rank pooling, so we ignore it.
        let slice = ctx
            .embeddings_seq_ith(0)
            .map_err(|e| CoreError::Embedding(format!("GGUF rerank rank embedding: {e}")))?;
        let ptr = slice.as_ptr();
        // SAFETY: `ptr` is the pointer llama.cpp returned from
        // `llama_get_embeddings_seq(ctx, 0)` for a Rank-pooling context that just
        // encoded sequence 0; it points at a live `std::vector<float>` holding
        // `n_cls_out` (=1) float. Reading index 0 is in-bounds for that buffer.
        // We deliberately do NOT touch `slice[1..]` (the slice's length is
        // `n_embd`, which overstates the real 1-float buffer — see doc above).
        let score = unsafe { *ptr };
        Ok(score)
    }
}

#[cfg(test)]
mod tests {
    use super::build_pair_tokens;

    #[test]
    fn build_pair_drops_doc_leading_bos_keeps_boundary() {
        // query = <s>(0) q1 q2 </s>(2) ; doc = <s>(0) d1 </s>(2)
        // expect <s> q1 q2 </s> </s> d1 </s> = [0,11,12,2, 2,21,2]
        let q = [0, 11, 12, 2];
        let d = [0, 21, 2];
        assert_eq!(build_pair_tokens(&q, &d), vec![0, 11, 12, 2, 2, 21, 2]);
    }

    #[test]
    fn build_pair_preserves_double_eos_segment_boundary() {
        // The two </s> tokens (query trailing + doc trailing-after-drop) must be
        // adjacent — that "</s> </s>" is the XLM-R sentence-pair separator.
        let q = [0, 5, 2];
        let d = [0, 7, 8, 2];
        let pair = build_pair_tokens(&q, &d);
        // find the first double-2 run
        let has_double_eos = pair.windows(2).any(|w| w == [2, 2]);
        assert!(has_double_eos, "pair must contain the </s></s> boundary: {pair:?}");
        assert_eq!(pair, vec![0, 5, 2, 2, 7, 8, 2]);
    }

    #[test]
    fn build_pair_handles_degenerate_doc() {
        // Doc with a single token (only <s>) → appended verbatim (no [1..] slice);
        // the inserted separator </s> still appears after the query.
        let q = [0, 9, 2];
        let d = [0];
        assert_eq!(build_pair_tokens(&q, &d), vec![0, 9, 2, 2, 0]);
    }

    #[test]
    fn build_pair_empty_doc() {
        // Empty doc → query + separator </s> only.
        let q = [0, 9, 2];
        let d: [i32; 0] = [];
        assert_eq!(build_pair_tokens(&q, &d), vec![0, 9, 2, 2]);
    }
}
