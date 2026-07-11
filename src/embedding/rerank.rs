//! Local ONNX cross-encoder reranker (RETRIEVAL_QUALITY_PLAN.md AP3).
//!
//! The pure rank-merge (`rank_merge`) is always compiled + unit-tested (no
//! model, no I/O). The actual `OrtReranker` (ort/tokenizers/ndarray) is behind
//! the `onnx` feature.
//!
//! Model: `jinaai/jina-reranker-v2-base-multilingual`
//! (XLMRobertaForSequenceClassification, `num_labels=1`). A tokenized
//! (query, document) **pair** goes in, a single classification logit comes out
//! = the relevance score (higher = more relevant).

/// Pure rank-merge (RETRIEVAL_QUALITY_PLAN.md AP3): given the reranker scores
/// for `n` candidates, return the indices of the `top_k` highest-scoring
/// candidates, score-descending. Stable on ties (lower original index first).
/// No model / no I/O — unit-tested in `tests.rs`.
pub fn rank_merge(scores: &[f32], top_k: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..scores.len()).collect();
    // Sort by score desc; ties keep the earlier candidate (sort_by is stable).
    idx.sort_by(|&a, &b| {
        scores[b]
            .partial_cmp(&scores[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    idx.truncate(top_k);
    idx
}

#[cfg(feature = "onnx")]
mod imp {
    //! EP selection mirrors `onnx.rs::OrtEmbedder::load` (MODEL_INFRA_PLAN.md
    //! AP3): a per-model `ExecutionProvider` choice (default CPU), apple-only.
    //!
    //! Measured (AP3 3a probe, jina-reranker-v2, XLM-R backbone): the CoreML
    //! default compute-units (`All`) reliably **crashes** at inference time on
    //! this model — CoreML rejects `roberta.embeddings.word_embeddings.weight`
    //! (250002x768, exceeds CoreML's 16384 static-dim limit), splits the graph
    //! into 86 partitions, and one of the resulting CoreML-assigned kernels
    //! fails at run time ("Unable to compute the prediction"). Restricting
    //! compute units to `CPUAndNeuralEngine` (the `Ane` option) avoids the crash
    //! (unsupported ops fall back to CPU) but is ~1.9x slower than plain CPU
    //! (measured ~46ms/pair vs. ~25ms/pair for a 16-pair batch) — so CPU stays
    //! the default and `Coreml`/`All` is not recommended for this reranker, but
    //! is left selectable since a future/different reranker model may fit
    //! CoreML's limits better.

    use std::path::Path;
    use std::sync::Mutex;

    use ndarray::Array2;
    // CoreML EP is apple-only (see core/Cargo.toml AP7 split); non-apple always
    // takes the CPU path, so the import + branch are apple-gated like in onnx.rs.
    #[cfg(target_vendor = "apple")]
    use ort::execution_providers::coreml::CoreMLComputeUnits;
    #[cfg(target_vendor = "apple")]
    use ort::execution_providers::CoreMLExecutionProvider;
    use ort::session::{builder::GraphOptimizationLevel, Session};
    use ort::value::Tensor;
    use tokenizers::Tokenizer;

    use crate::db::models::ExecutionProvider;
    use crate::{CoreError, Result};

    /// XLM-R offsets positions by 2 (`max_position_embeddings=1026` → effective
    /// max sequence ≈ 1024). Truncate each (query, doc) pair to fit.
    const MAX_SEQ_LEN: usize = 1024;

    /// Score at most this many pairs per session run, to bound peak memory on the
    /// 1.11 GB model. Pairs are padded to the batch's longest sequence.
    const BATCH_SIZE: usize = 16;

    pub struct OrtReranker {
        session: Mutex<Session>,
        tokenizer: Tokenizer,
        has_token_type: bool,
    }

    impl OrtReranker {
        /// Load `model.onnx` + `tokenizer.json` from `model_dir`, honoring
        /// `execution_provider` (the reranker model's configured EP; `None`/
        /// non-apple → CPU) on every apple target, including iOS — no runtime
        /// force-override; iOS's own default is set at model-creation time.
        pub fn load(model_dir: &Path, execution_provider: Option<ExecutionProvider>) -> Result<Self> {
            let model_path = model_dir.join("model.onnx");
            let tokenizer_path = model_dir.join("tokenizer.json");

            let model_bytes = std::fs::read(&model_path).map_err(|e| {
                CoreError::Embedding(format!("read reranker model {}: {e}", model_path.display()))
            })?;
            #[cfg_attr(not(target_vendor = "apple"), allow(unused_mut))]
            let mut builder = Session::builder()?;
            // No iOS force-override here either (MODEL_INFRA_PLAN.md AP3): the
            // model's own `execution_provider` is respected on every apple
            // target, including iOS — a user can pick CPU on iOS too. Whatever
            // seeds an iOS reranker default can set `Ane` there, same as the
            // embedder's iOS seed.
            #[cfg(target_vendor = "apple")]
            {
                let compute_units = match execution_provider {
                    Some(ExecutionProvider::Ane) => Some(CoreMLComputeUnits::CPUAndNeuralEngine),
                    Some(ExecutionProvider::Coreml) => Some(CoreMLComputeUnits::All),
                    _ => None,
                };
                if let Some(units) = compute_units {
                    builder = builder.with_execution_providers([CoreMLExecutionProvider::default()
                        .with_compute_units(units)
                        .build()])?;
                }
            }
            let session = builder
                .with_optimization_level(GraphOptimizationLevel::Level3)?
                .commit_from_memory(&model_bytes)?;

            let has_token_type = session.inputs.iter().any(|i| i.name == "token_type_ids");

            let tokenizer = Tokenizer::from_file(&tokenizer_path)
                .map_err(|e| CoreError::Embedding(format!("reranker tokenizer load: {e}")))?;

            Ok(Self {
                session: Mutex::new(session),
                tokenizer,
                has_token_type,
            })
        }

        /// Score each (query, doc) pair; returns one relevance logit per doc, in
        /// the same order as `docs`. Batches in small groups to bound memory;
        /// each pair is truncated to `MAX_SEQ_LEN`.
        pub fn score_pairs(&self, query: &str, docs: &[&str]) -> Result<Vec<f32>> {
            if docs.is_empty() {
                return Ok(Vec::new());
            }
            let mut out: Vec<f32> = Vec::with_capacity(docs.len());
            for batch in docs.chunks(BATCH_SIZE) {
                out.extend(self.score_batch(query, batch)?);
            }
            Ok(out)
        }

        fn score_batch(&self, query: &str, docs: &[&str]) -> Result<Vec<f32>> {
            // Encode each (query, doc) pair; tokenizers 0.20 accepts a pair input.
            let mut encoded: Vec<(Vec<i64>, Vec<i64>)> = Vec::with_capacity(docs.len());
            let mut max_len = 1usize;
            for &doc in docs {
                let enc = self
                    .tokenizer
                    .encode((query, doc), true)
                    .map_err(|e| CoreError::Embedding(format!("rerank tokenize: {e}")))?;
                let mut ids: Vec<i64> = enc.get_ids().iter().map(|&x| x as i64).collect();
                let mut mask: Vec<i64> =
                    enc.get_attention_mask().iter().map(|&x| x as i64).collect();
                if ids.len() > MAX_SEQ_LEN {
                    ids.truncate(MAX_SEQ_LEN);
                    mask.truncate(MAX_SEQ_LEN);
                }
                max_len = max_len.max(ids.len());
                encoded.push((ids, mask));
            }

            // Right-pad every pair to the batch's longest sequence (pad id 1 =
            // XLM-R <pad>; the attention mask zeroes it out so the value is
            // irrelevant).
            let batch = encoded.len();
            let mut ids_flat: Vec<i64> = Vec::with_capacity(batch * max_len);
            let mut mask_flat: Vec<i64> = Vec::with_capacity(batch * max_len);
            for (ids, mask) in &encoded {
                for i in 0..max_len {
                    ids_flat.push(*ids.get(i).unwrap_or(&1));
                    mask_flat.push(*mask.get(i).unwrap_or(&0));
                }
            }

            let input_ids = Array2::from_shape_vec((batch, max_len), ids_flat)
                .map_err(|e| CoreError::Embedding(e.to_string()))?;
            let attention = Array2::from_shape_vec((batch, max_len), mask_flat)
                .map_err(|e| CoreError::Embedding(e.to_string()))?;

            let mut session = self
                .session
                .lock()
                .map_err(|_| CoreError::Embedding("reranker session lock poisoned".into()))?;
            let outputs = if self.has_token_type {
                let token_type = Array2::<i64>::zeros((batch, max_len));
                session.run(ort::inputs![
                    "input_ids" => Tensor::from_array(input_ids)?,
                    "attention_mask" => Tensor::from_array(attention)?,
                    "token_type_ids" => Tensor::from_array(token_type)?,
                ])?
            } else {
                session.run(ort::inputs![
                    "input_ids" => Tensor::from_array(input_ids)?,
                    "attention_mask" => Tensor::from_array(attention)?,
                ])?
            };

            // Output is [batch, num_labels=1] → one logit per pair.
            let (_shape, data) = outputs[0].try_extract_tensor::<f32>()?;
            if data.len() < batch {
                return Err(CoreError::Embedding(format!(
                    "reranker output too short: {} logits for {batch} pairs",
                    data.len()
                )));
            }
            // Take the first logit of each row (num_labels=1 → row length 1).
            let per_row = data.len() / batch;
            Ok((0..batch).map(|r| data[r * per_row]).collect())
        }
    }
}

#[cfg(feature = "onnx")]
pub use imp::OrtReranker;

#[cfg(test)]
mod tests {
    use super::rank_merge;

    #[test]
    fn rank_merge_sorts_desc_and_truncates() {
        // scores: idx0=0.1, idx1=0.9, idx2=0.5, idx3=0.7
        let scores = [0.1, 0.9, 0.5, 0.7];
        assert_eq!(rank_merge(&scores, 2), vec![1, 3]);
        assert_eq!(rank_merge(&scores, 4), vec![1, 3, 2, 0]);
    }

    #[test]
    fn rank_merge_top_k_larger_than_len_returns_all() {
        let scores = [0.2, 0.4];
        assert_eq!(rank_merge(&scores, 10), vec![1, 0]);
    }

    #[test]
    fn rank_merge_stable_on_ties_keeps_earlier_index() {
        // idx1 and idx2 tie at 0.5 → the earlier index (1) comes first.
        let scores = [0.1, 0.5, 0.5, 0.3];
        assert_eq!(rank_merge(&scores, 3), vec![1, 2, 3]);
    }

    #[test]
    fn rank_merge_empty_and_zero_k() {
        assert!(rank_merge(&[], 5).is_empty());
        assert!(rank_merge(&[0.1, 0.2], 0).is_empty());
    }
}
