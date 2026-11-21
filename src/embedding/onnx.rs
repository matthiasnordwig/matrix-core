//! Local ONNX embedder targeting the Apple Neural Engine via the CoreML
//! execution provider. Compiled only under the `onnx` feature.
//!
//! Input-adaptive: BERT-family exports (MiniLM, e5) need `token_type_ids`,
//! others (Jina v2) do not — we pass it only if the model declares that input.
//! Pipeline: tokenize → run → attention-masked mean-pool → optional Matryoshka
//! truncation → L2-normalize.
//!
//! Multimodal-capable exports with in-graph pooling (EmbeddingGemma 2):
//! - if the session has an output named `sentence_embedding` (pooling + dense
//!   projection already inside the graph), it is used directly instead of
//!   mean-pooling `outputs[0]`; Matryoshka truncation + L2-normalize as above;
//! - every extra f32 input whose name ends in `_features` (`image_features`,
//!   `video_features`, `audio_features`) gets an EMPTY `[0, D]` tensor (D from
//!   the declared last dim, default 512) — text-only use.
//! - models with external weights (`<model>.onnx_data`/`.data` next to the
//!   `.onnx`) need nothing extra: the session loads from the file path, so ORT
//!   resolves the sidecar relative to the model's directory.
//!
//! Models without these inputs/outputs (jina-de) take the unchanged path.
//! `embed_with_features` fills `image_features` for real (image embedding via
//! `vision.rs`); plain text embedding keeps feeding it empty.
//! Task prefixes (`query_prefix`/`document_prefix`) are NOT applied here — the
//! callers add them via `embedding::prefix::apply_task_prefix`.
//!
//! Execution provider (apple only): `EmbeddingModel::execution_provider` is a
//! real per-model choice, default CPU, also on iOS (no runtime force-override;
//! the iOS seed merely sets `Ane` as the creation default). `Ane` →
//! `CPUAndNeuralEngine`, `Coreml` → `All`; they are distinct EPs, not aliases.
//! Non-apple targets always use the CPU EP (the `coreml` ort feature exists
//! only on apple, see `core/Cargo.toml`). CoreML is slower than CPU for the
//! jina-de embedder (graph partitioning) and cannot run EmbeddingGemma 2 at all
//! (it refuses the empty `*_features` inputs), hence the CPU default. `load_on`
//! picks the unit (`onnx_ep.rs`); only the measurement (`examples/onnx_unit_bench.rs`)
//! asks for the graphics unit, see `handbuch/core.md` "Rechenweg". `load_padded` fixes the input
//! shape (padded to one length, attention mask 0 on the padding) for DirectML.

use std::sync::Mutex;

use ndarray::Array2;
// CoreML EP is apple-only (the `coreml` ort feature is enabled only on apple
// targets — see core/Cargo.toml AP7 split). On Linux the symbol does not exist,
// so the import + the `use_coreml` branch below are gated to apple; non-apple
// always takes the CPU path.
#[cfg(target_vendor = "apple")]
use ort::ep::coreml::{ComputeUnits as CoreMLComputeUnits, CoreML as CoreMLExecutionProvider};
use ort::session::Session;
use ort::value::Tensor;
use tokenizers::{Tokenizer, TruncationParams};

use super::QueryEmbedder;
use crate::db::embeddings::l2_normalize;
use crate::db::models::EmbeddingModel;
// Only referenced inside the apple-only CoreML EP branch below.
#[cfg(target_vendor = "apple")]
use crate::db::models::ExecutionProvider;
use crate::{CoreError, Result};

impl<R> From<ort::Error<R>> for CoreError {
    fn from(e: ort::Error<R>) -> Self {
        CoreError::Embedding(e.to_string())
    }
}

pub struct OrtEmbedder {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    out_dim: usize,
    is_matryoshka: bool,
    has_token_type: bool,
    /// Extra `*_features` f32 inputs (name, last dim) fed as empty `[0, D]`.
    feature_inputs: Vec<(String, usize)>,
    /// Graph emits a pooled `sentence_embedding` output (skip mean-pool).
    has_sentence_embedding: bool,
    /// Fixed input length (`load_padded`): every input is padded to it, longer ones are refused.
    pad_to: Option<usize>,
}

/// Free dimensions of the search model's inputs fixed by [`OrtEmbedder::load_padded`]: one input
/// of `len` tokens, no image/video/audio tokens. Names absent from a model are ignored by ONNX Runtime.
fn fixed_dimensions(len: usize) -> [(&'static str, i64); 5] {
    [("batch_size", 1), ("sequence_length", len as i64), ("num_image_tokens", 0), ("num_video_tokens", 0), ("num_audio_tokens", 0)]
}

/// Input lengths in tokens and the memory one call needs (EmbeddingGemma 2 on CPU, measured; the ONNX
/// arena keeps its peak, and memory grows faster than the text: 16 384 tokens took 26 GB).
const INPUT_STEPS: [(usize, u64); 3] = [(2_048, 1_400 << 20), (4_096, 2_900 << 20), (8_192, 7_800 << 20)];
/// Share of the device's memory one call may take.
const MEMORY_DIVISOR: u64 = 3;
/// Where the memory cannot be read: enough for a passage of the structural chunkers.
const UNKNOWN_MEMORY_TOKENS: usize = 4_096;

/// Longest input of one embedding call on a device with `memory` bytes, the same rule on every
/// platform. The vector of a longer text stands for its start; keyword search still sees all of it.
pub(super) fn input_token_limit(memory: Option<u64>) -> usize {
    let Some(memory) = memory else { return UNKNOWN_MEMORY_TOKENS };
    let fitting = INPUT_STEPS.iter().rev().find(|(_, needs)| *needs <= memory / MEMORY_DIVISOR);
    fitting.map_or(INPUT_STEPS[0].0, |(tokens, _)| *tokens)
}

pub(super) fn cap_input_tokens(tokenizer: &mut Tokenizer, max_length: usize) -> Result<()> {
    let limit = TruncationParams { max_length, ..Default::default() };
    tokenizer
        .with_truncation(Some(limit))
        .map_err(|e| CoreError::Embedding(format!("tokenizer truncation: {e}")))?;
    Ok(())
}

/// Declared last dimension of a `*_features` input, default 512 when dynamic/unknown.
fn feature_dim(input: &ort::value::Outlet) -> usize {
    match input.dtype() {
        ort::value::ValueType::Tensor { shape, .. } => match shape.last() {
            Some(&d) if d > 0 => d as usize,
            _ => 512,
        },
        _ => 512,
    }
}

impl OrtEmbedder {
    pub fn load(model: &EmbeddingModel) -> Result<Self> {
        Self::load_on(model, crate::compute::Unit::Cpu)
    }

    /// Like [`Self::load`], on `unit` (`onnx_ep::session_on`); `Unit::Cpu` keeps the model's own Apple EP.
    pub fn load_on(model: &EmbeddingModel, unit: crate::compute::Unit) -> Result<Self> {
        Self::load_with(model, unit, None)
    }

    /// Like [`Self::load_on`] with every input padded to `len` tokens and the input shape fixed:
    /// DirectML fuses the graph only for one shape (`handbuch/core.md` "Rechenweg"). An input longer
    /// than `len` tokens is refused (`embed` errors); [`Self::token_count`] tells beforehand.
    pub fn load_padded(model: &EmbeddingModel, unit: crate::compute::Unit, len: usize) -> Result<Self> {
        Self::load_with(model, unit, Some(len))
    }

    /// Tokens `text` becomes (after the length cap), as the model sees it.
    pub fn token_count(&self, text: &str) -> Result<usize> {
        let encoding = self.tokenizer.encode(text, true).map_err(|e| CoreError::Embedding(format!("tokenize: {e}")))?;
        Ok(encoding.get_ids().len())
    }

    fn load_with(model: &EmbeddingModel, unit: crate::compute::Unit, pad_to: Option<usize>) -> Result<Self> {
        let model_path = model
            .model_path
            .as_ref()
            .ok_or_else(|| CoreError::Embedding("local model_path missing".into()))?;
        let tokenizer_path = model
            .tokenizer_path
            .as_ref()
            .ok_or_else(|| CoreError::Embedding("tokenizer_path missing".into()))?;

        let mut builder = super::onnx_ep::session_on(unit)?;
        // Apple only: which EP a model uses is now a real per-model choice
        // (MODEL_INFRA_PLAN.md AP3), defaulting to CPU — including on iOS: the
        // bundled iOS model is *seeded* with `Ane` (see
        // `src-tauri/src/lib.rs::seed_ios_local_model`), but the runtime here no
        // longer force-overrides to CoreML, so a user can pick CPU on iOS too.
        // CoreML does NOT crash per se (superseded claim) — the earlier "flaky
        // in `cargo run`" note was about session *compilation*, not execution.
        // The actual tradeoff, measured on this jina-de embedder: CoreML graph-
        // partitions the model (97 partitions here, `word_embeddings` exceeds
        // CoreML's 16384 static-dim limit so large-vocab ops fall back to CPU
        // mid-graph), which made it ~3.6x slower than plain CPU for this small
        // model. `Ane` maps to `CPUAndNeuralEngine` (ANE + CPU fallback for
        // unsupported ops), `Coreml` maps to `All` (CoreML picks CPU/GPU/ANE per
        // op) — both are real, distinct compute-unit configs, not aliases.
        // Non-apple (Linux) has no CoreML EP → always the CPU path (no explicit EP).
        #[cfg(target_vendor = "apple")]
        {
            let compute_units = match model.execution_provider {
                Some(ExecutionProvider::Ane) => Some(CoreMLComputeUnits::CPUAndNeuralEngine),
                Some(ExecutionProvider::Coreml) => Some(CoreMLComputeUnits::All),
                _ => None,
            };
            if let (Some(units), crate::compute::Unit::Cpu) = (compute_units, unit) {
                builder = builder.with_execution_providers([CoreMLExecutionProvider::default()
                    .with_compute_units(units)
                    .build()])?;
            }
        }
        for (name, size) in pad_to.map(fixed_dimensions).into_iter().flatten() {
            builder = builder.with_dimension_override(name, size)?;
        }
        let session = builder.commit_from_file(model_path)?;

        let has_token_type = session.inputs().iter().any(|i| i.name() == "token_type_ids");
        let feature_inputs: Vec<(String, usize)> = session
            .inputs()
            .iter()
            .filter(|i| i.name().ends_with("_features"))
            .filter(|i| {
                matches!(
                    i.dtype(),
                    ort::value::ValueType::Tensor { ty: ort::value::TensorElementType::Float32, .. }
                )
            })
            .map(|i| (i.name().to_string(), feature_dim(i)))
            .collect();
        let has_sentence_embedding = session.outputs().iter().any(|o| o.name() == "sentence_embedding");

        let mut tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| CoreError::Embedding(format!("tokenizer load: {e}")))?;
        cap_input_tokens(&mut tokenizer, input_token_limit(crate::device_memory::device_memory_bytes()))?;

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            out_dim: model.default_dim as usize,
            is_matryoshka: model.is_matryoshka,
            has_token_type,
            feature_inputs,
            has_sentence_embedding,
            pad_to,
        })
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.embed_with_features(text, None)
    }

    /// Embed `text`, feeding `image_features` (`[n, D]`, from the vision
    /// encoder) into the `image_features` input instead of an empty tensor.
    /// `text` then carries the image placeholder tokens (`vision.rs`).
    pub(crate) fn embed_with_features(&self, text: &str, image_features: Option<Array2<f32>>) -> Result<Vec<f32>> {
        let mut image_features = image_features;
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| CoreError::Embedding(format!("tokenize: {e}")))?;
        let mut ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
        let mut mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&x| x as i64)
            .collect();
        if let Some(len) = self.pad_to {
            if ids.len() > len {
                return Err(CoreError::Embedding(format!("{} tokens, the fixed input holds {len}", ids.len())));
            }
            let pad = self.tokenizer.get_padding().map(|p| p.pad_id).or_else(|| self.tokenizer.token_to_id("<pad>")).unwrap_or(0);
            let pad = pad as i64;
            ids.resize(len, pad);
            mask.resize(len, 0);
        }
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
        let mut inputs = if self.has_token_type {
            let token_type = to_arr(vec![0i64; mask.len()])?;
            ort::inputs![
                "input_ids" => Tensor::from_array(input_ids)?,
                "attention_mask" => Tensor::from_array(attention)?,
                "token_type_ids" => Tensor::from_array(token_type)?,
            ]
        } else {
            ort::inputs![
                "input_ids" => Tensor::from_array(input_ids)?,
                "attention_mask" => Tensor::from_array(attention)?,
            ]
        };
        for (name, dim) in &self.feature_inputs {
            let feats = match (name.as_str(), image_features.take()) {
                ("image_features", Some(f)) => f,
                (_, other) => {
                    image_features = other;
                    Array2::<f32>::zeros((0, *dim))
                }
            };
            inputs.push((name.clone().into(), Tensor::from_array(feats)?.into()));
        }
        if image_features.is_some() {
            return Err(CoreError::Embedding("model has no image_features input".into()));
        }
        let outputs = session.run(inputs)?;

        let mut pooled = if self.has_sentence_embedding {
            let (_shape, data) = outputs["sentence_embedding"].try_extract_tensor::<f32>()?;
            data.to_vec()
        } else {
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
            pooled
        };

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
