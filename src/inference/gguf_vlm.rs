//! Local vision model: a GGUF chat model plus its image projector file
//! (`mmproj-*.gguf`) through llama.cpp's multimodal library (mtmd). One picture
//! + one instruction in, text out — the on-device twin of the remote
//! vision request in the app's `llm_vision.rs`.
//!
//! Flow per call: picture bytes (JPEG/PNG, decoded by mtmd's own stb_image) →
//! `MtmdBitmap::from_buffer` → user turn "marker + instruction"
//! (`vlm_prompt::user_turn_with_image`) wrapped by the model's own chat
//! template → `MtmdContext::tokenize` (text and image chunks) →
//! `eval_chunks` (image encoder + decode) → greedy generation up to
//! `max_tokens` (the endpoint's Output Reserve). The answer reports whether it
//! hit that limit; the caller turns a cut-off answer into an error.
//!
//! Uses the process-global llama.cpp backend (`crate::llama_backend`). Model
//! and projector are loaded once per engine; every call builds a fresh
//! context. mtmd's encoder keeps per-context output buffers, so calls on one
//! engine are serialized by an internal lock (callers run us inside
//! `spawn_blocking`). GPU (Metal) for both parts, except in the iOS Simulator:
//! there llama.cpp's Metal kernels produce garbage without an error, so the
//! model gets `n_gpu_layers(0)` and the projector `use_gpu = false` (same rule
//! as `gguf.rs`).

use std::ffi::CString;
use std::num::NonZeroU32;
use std::sync::Mutex;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaModel};
use llama_cpp_2::mtmd::{MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText};
use llama_cpp_2::sampling::LlamaSampler;

use super::vlm_prompt::{fits_context, user_turn_with_image, DEFAULT_MEDIA_MARKER};
use crate::llama_backend::backend as get_backend;

/// Prompt + image chunks are decoded in batches of at most this many tokens.
/// Non-causal image encoders (Gemma 3) need the whole image chunk in one
/// micro-batch, so the micro-batch is as large as the batch.
const MAX_BATCH: u32 = 1024;

/// Text of one answer and whether generation stopped at `max_tokens`.
#[derive(Debug, Clone)]
pub struct VisionAnswer {
    pub text: String,
    pub hit_limit: bool,
}

pub struct GgufVisionEngine {
    model: LlamaModel,
    mtmd: MtmdContext,
    n_ctx: u32,
    busy: Mutex<()>,
}

/// GPU only off the iOS Simulator (see the module comment).
const fn use_gpu() -> bool {
    !cfg!(all(target_os = "ios", target_abi = "sim"))
}

impl GgufVisionEngine {
    /// Load the chat model at `model_path` and its projector at `mmproj_path`.
    pub fn load(model_path: &str, mmproj_path: &str, n_ctx: u32) -> Result<Self, String> {
        if n_ctx == 0 {
            return Err("Endpoint has no Context Window configured (0) — set a real Context Window value for this local model in Settings → Endpoints before using it.".to_string());
        }
        for (what, p) in [("Model", model_path), ("Image projector (mmproj)", mmproj_path)] {
            if !std::path::Path::new(p).exists() {
                return Err(format!("{what} file not found: {p}"));
            }
        }
        let backend = get_backend()?;
        #[allow(unused_mut)]
        let mut model_params = LlamaModelParams::default().with_use_mmap(true);
        #[cfg(feature = "gguf-metal")]
        {
            model_params = model_params.with_n_gpu_layers(999);
        }
        #[cfg(all(target_os = "ios", target_abi = "sim"))]
        {
            model_params = model_params.with_n_gpu_layers(0);
        }
        let model = LlamaModel::load_from_file(backend, model_path, &model_params)
            .map_err(|e| format!("model load {model_path}: {e}"))?;
        let params = MtmdContextParams {
            use_gpu: use_gpu(),
            print_timings: false,
            n_threads: std::thread::available_parallelism().map_or(4, |n| n.get() as i32),
            media_marker: CString::new(DEFAULT_MEDIA_MARKER).map_err(|e| e.to_string())?,
        };
        let mtmd = MtmdContext::init_from_file(mmproj_path, &model, &params)
            .map_err(|e| format!("image projector load {mmproj_path}: {e} (does it belong to this model?)"))?;
        if !mtmd.support_vision() {
            return Err(format!("{mmproj_path} is not an image projector (no vision support)"));
        }
        Ok(Self { model, mtmd, n_ctx, busy: Mutex::new(()) })
    }

    /// Describe the picture `image` (encoded JPEG/PNG bytes) following `prompt`;
    /// at most `max_tokens` of answer.
    pub fn describe(&self, image: &[u8], prompt: &str, max_tokens: u32, cpu_threads: Option<i64>) -> Result<VisionAnswer, String> {
        let _guard = self.busy.lock().unwrap_or_else(|p| p.into_inner());
        let backend = get_backend()?;
        let batch = MAX_BATCH.min(self.n_ctx);
        let mut ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(self.n_ctx))
            .with_n_batch(batch)
            .with_n_ubatch(batch);
        if let Some(t) = cpu_threads {
            ctx_params = ctx_params.with_n_threads(t as i32).with_n_threads_batch(t as i32);
        }
        let mut ctx = self.model.new_context(backend, ctx_params).map_err(|e| format!("context create: {e}"))?;

        let bitmap = MtmdBitmap::from_buffer(&self.mtmd, image).map_err(|e| format!("picture could not be read by the local model: {e}"))?;
        let tmpl = self.model.chat_template(None).map_err(|e| format!("model has no default chat template or invalid: {e}"))?;
        let user = LlamaChatMessage::new("user".to_string(), user_turn_with_image(prompt, DEFAULT_MEDIA_MARKER))
            .map_err(|e| format!("invalid chat message: {e}"))?;
        let text = self.model.apply_chat_template(&tmpl, &[user], true).map_err(|e| format!("failed to apply chat template: {e}"))?;

        let chunks = self
            .mtmd
            .tokenize(MtmdInputText { text, add_special: true, parse_special: true }, &[&bitmap])
            .map_err(|e| format!("image tokenize: {e}"))?;
        let prompt_tokens = chunks.total_tokens();
        if !fits_context(prompt_tokens, max_tokens, self.n_ctx) {
            return Err(format!(
                "Picture + prompt need {prompt_tokens} tokens plus {max_tokens} answer tokens, the endpoint's context is {} — raise its Context Window.",
                self.n_ctx
            ));
        }
        let mut n_past = chunks
            .eval_chunks(&self.mtmd, &ctx, 0, 0, batch as i32, true)
            .map_err(|e| format!("image evaluation: {e}"))?;

        // Greedy (deterministic, like temperature 0 on the remote path) with a
        // mild repetition penalty: small vision models otherwise loop.
        let mut sampler = LlamaSampler::chain_simple([LlamaSampler::penalties(64, 1.1, 0.0, 0.0), LlamaSampler::greedy()]);
        let mut out = Vec::<u8>::new();
        let mut step = LlamaBatch::new(1, 1);
        let mut idx = -1;
        for _ in 0..max_tokens {
            let tok = sampler.sample(&ctx, idx);
            if self.model.is_eog_token(tok) {
                return Ok(VisionAnswer { text: String::from_utf8_lossy(&out).into_owned(), hit_limit: false });
            }
            if let Ok(bytes) = self.model.token_to_piece_bytes(tok, 128, true, None) {
                out.extend_from_slice(&bytes);
            }
            step.clear();
            step.add(tok, n_past, &[0], true).map_err(|e| format!("batch: {e}"))?;
            n_past += 1;
            ctx.decode(&mut step).map_err(|e| format!("decode: {e}"))?;
            idx = 0;
        }
        Ok(VisionAnswer { text: String::from_utf8_lossy(&out).into_owned(), hit_limit: true })
    }
}
