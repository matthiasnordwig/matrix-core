//! Local ONNX embedder targeting the Apple Neural Engine via the CoreML
//! execution provider. Compiled only under the `onnx` feature.
//!
//! Input-adaptive: BERT-family exports (MiniLM, e5) need `token_type_ids`,
//! others (Jina v2) do not — we pass it only if the model declares that input.
//! Pipeline: tokenize → run → attention-masked mean-pool → optional Matryoshka
//! truncation → L2-normalize.

use std::sync::Mutex;

use ndarray::Array2;
use ort::execution_providers::CoreMLExecutionProvider;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use tokenizers::Tokenizer;

use super::QueryEmbedder;
use crate::db::embeddings::l2_normalize;
use crate::db::models::{EmbeddingModel, ExecutionProvider};
use crate::{CoreError, Result};

impl From<ort::Error> for CoreError {
    fn from(e: ort::Error) -> Self {
        CoreError::Embedding(e.to_string())
    }
}

pub struct OrtEmbedder {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    out_dim: usize,
    is_matryoshka: bool,
    has_token_type: bool,
}

impl OrtEmbedder {
    pub fn load(model: &EmbeddingModel) -> Result<Self> {
        let model_path = model
            .model_path
            .as_ref()
            .ok_or_else(|| CoreError::Embedding("local model_path missing".into()))?;
        let tokenizer_path = model
            .tokenizer_path
            .as_ref()
            .ok_or_else(|| CoreError::Embedding("tokenizer_path missing".into()))?;

        let model_bytes = std::fs::read(model_path)
            .map_err(|e| CoreError::Embedding(format!("read model {model_path}: {e}")))?;
        let mut builder = Session::builder()?;
        // iOS always runs on the ANE via CoreML (proper app bundle → stable).
        // On macOS, CoreML model compilation inside the `cargo run` GUI process is
        // flaky, so use it only if explicitly requested; otherwise CPU.
        let use_coreml = cfg!(target_os = "ios")
            || matches!(
                model.execution_provider,
                Some(ExecutionProvider::Coreml) | Some(ExecutionProvider::Ane)
            );
        if use_coreml {
            builder = builder.with_execution_providers([CoreMLExecutionProvider::default().build()])?;
        }
        let session = builder
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .commit_from_memory(&model_bytes)?;

        let has_token_type = session.inputs.iter().any(|i| i.name == "token_type_ids");

        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| CoreError::Embedding(format!("tokenizer load: {e}")))?;

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            out_dim: model.default_dim as usize,
            is_matryoshka: model.is_matryoshka,
            has_token_type,
        })
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| CoreError::Embedding(format!("tokenize: {e}")))?;
        let ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
        let mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&x| x as i64)
            .collect();
        let seq = ids.len().max(1);

        let to_arr = |v: Vec<i64>| {
            Array2::from_shape_vec((1, v.len()), v).map_err(|e| CoreError::Embedding(e.to_string()))
        };
        let input_ids = to_arr(ids)?;
        let attention = to_arr(mask.clone())?;

        let mut session = self
            .session
            .lock()
            .map_err(|_| CoreError::Embedding("session lock poisoned".into()))?;
        let outputs = if self.has_token_type {
            let token_type = to_arr(vec![0i64; mask.len()])?;
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

        let (_shape, data) = outputs[0].try_extract_tensor::<f32>()?;
        let hidden = data.len() / seq;

        let mut pooled = vec![0.0f32; hidden];
        let mut count = 0.0f32;
        for (t, &m) in mask.iter().enumerate() {
            if m == 0 {
                continue;
            }
            count += 1.0;
            let base = t * hidden;
            for (h, p) in pooled.iter_mut().enumerate() {
                *p += data[base + h];
            }
        }
        if count > 0.0 {
            for p in pooled.iter_mut() {
                *p /= count;
            }
        }

        if self.is_matryoshka && self.out_dim > 0 && self.out_dim < pooled.len() {
            pooled.truncate(self.out_dim);
        }
        l2_normalize(&mut pooled);
        Ok(pooled)
    }
}

impl QueryEmbedder for OrtEmbedder {
    fn embed_query(&self, _model: &EmbeddingModel, query: &str) -> Result<Vec<f32>> {
        self.embed(query)
    }
}
