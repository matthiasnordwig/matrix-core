//! Local chat engine: one GGUF chat model through llama.cpp, messages in, answer
//! text out. The prompt comes from the model's chat template (`chat_template.rs`),
//! sampling is temperature 0.7 with a mild repetition penalty, and a gpt-oss
//! Harmony reply is cut to its `final` channel (`chat_text::harmony_final`).
//!
//! The engine keeps one llama.cpp context with the tokens of its last call in memory
//! (`Session`): the next call reads only what its prompt does not share with them
//! (`prefix_reuse.rs` decides; models with a recurrent part restore a saved state), so
//! the rounds of a tool loop no longer read the whole conversation again. Where a prompt
//! stops sharing the previous call, the whole memory up to there is copied to host RAM
//! (`Snapshot`) — the instructions every Grid row or extraction chunk starts with — and
//! restored by a later call starting with it, also after another job's calls. Calls on one
//! local endpoint run one at a time (its gate, `max_concurrency` 1); a call that still finds
//! the session busy gets a context of its own and keeps nothing. A failed or stopped call
//! starts the next one fresh (snapshots stay). The kept context holds its memory (KV,
//! recurrent state, compute buffers) between calls.
//!
//! When the kept memory goes (the one rule, `handbuch/core.md`): the next call on this engine
//! keeps only what its prompt shares — a rewritten chat history (dropped or compacted turns)
//! or another job's call replaces the rest (snapshots of shared beginnings survive within their
//! bounds); under memory pressure the app calls [`GgufEngine::drop_saved`]. Embedding and
//! reranking models are other engines and never touch it.
use llama_cpp_2::context::session::LlamaStateSeqFlags;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::context::params::{LlamaContextParams, KvCacheType};
use llama_cpp_2::token::LlamaToken;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, TryLockError};

use super::prefix_reuse::{self, Reuse};

// Process-global backend shared with the GGUF embedder/reranker — a
// module-local singleton here caused `BackendAlreadyInitialized` for
// whichever GGUF consumer initialized second (see `crate::llama_backend`).
use crate::llama_backend::backend as get_backend;

/// Tokens read per decode call.
const CHUNK: usize = 512;

pub struct GgufEngine {
    // Declared before `model`: fields drop in order, and the session's context borrows it.
    session: Mutex<Option<Session>>,
    model: Box<LlamaModel>,
    n_ctx: u32,
    /// Memory that cannot be cut back to any position (recurrent part, sliding window).
    partial_memory: bool,
}

/// Context settings a session was made with; another call with other settings starts a new one.
#[derive(Clone, PartialEq, Eq)]
struct SessionKey {
    kv_quantization: Option<String>,
    cpu_threads: Option<i64>,
}

/// What the context's memory holds.
#[derive(Default)]
struct Memory {
    tokens: Vec<LlamaToken>,
    /// Saved partial states (`LlamaStateSeqFlags::PARTIAL_ONLY`) by the position they were
    /// taken at, ascending (`prefix_reuse::retain`).
    checkpoints: Vec<(usize, Vec<u8>)>,
    /// Saved beginnings, least recently used first (`prefix_reuse::snapshots_to_drop`).
    snapshots: Vec<Snapshot>,
}

/// The whole memory (every layer) after reading `tokens`.
struct Snapshot {
    tokens: Vec<LlamaToken>,
    state: Vec<u8>,
}

struct Session {
    // SAFETY: borrows the engine's boxed model, which never moves and outlives the session
    // (`GgufEngine` field order).
    ctx: LlamaContext<'static>,
    key: SessionKey,
    memory: Memory,
}

// SAFETY: a llama.cpp context may move between threads; it is only ever used by the one
// thread holding the engine's session lock.
unsafe impl Send for Session {}

/// Whether the model's attention uses a sliding window (its metadata names one).
fn has_sliding_window(model: &LlamaModel) -> bool {
    model
        .meta_val_str("general.architecture")
        .is_ok_and(|arch| model.meta_val_str(&format!("{arch}.attention.sliding_window")).is_ok())
}

impl GgufEngine {
    pub fn load(model_path: &str, n_ctx: u32) -> Result<Self, String> {
        // `n_ctx: 0` doesn't fail here — `with_n_ctx(NonZeroU32::new(0))` becomes
        // `None`, which llama.cpp silently interprets as "use the model's own
        // trained context length" — but the prompt-fits check in `generate()`
        // below uses the *stored* `n_ctx` (still 0), so `available_ctx` computes
        // to 0 and every single request fails with a confusing "Prompt zu lang:
        // N Tokens (Verfügbar: 0 Kontext...)" forever, regardless of how small
        // the prompt is. Fail loudly once here instead, with an actionable message.
        if n_ctx == 0 {
            return Err("Endpoint has no Context Window configured (0) — set a real Context Window value for this local model in Settings → Endpoints before using it.".to_string());
        }
        let backend = get_backend()?;
        #[allow(unused_mut)]
        let mut model_params = LlamaModelParams::default();
        
        model_params = model_params.with_use_mmap(true);
        
        #[cfg(feature = "gguf-metal")]
        {
            model_params = model_params.with_n_gpu_layers(999);
        }
        // The iOS Simulator's Metal runs llama.cpp's kernels wrongly (garbage
        // tokens, no error) — real devices are fine. CPU there; llama.cpp's own
        // default would otherwise offload everything.
        #[cfg(all(target_os = "ios", target_abi = "sim"))]
        {
            model_params = model_params.with_n_gpu_layers(0);
        }
        if !std::path::Path::new(model_path).exists() {
            return Err(format!("Model file not found: {}", model_path));
        }

        let model = LlamaModel::load_from_file(backend, model_path, &model_params)
            .map_err(|e| format!("model load {model_path}: {e}"))?;
            
        let partial_memory = model.is_recurrent() || model.is_hybrid() || has_sliding_window(&model);
        Ok(Self { session: Mutex::new(None), model: Box::new(model), n_ctx, partial_memory })
    }

    /// Tokens of `text` in this model's vocabulary (no BOS, no chat template) — the exact
    /// count an app can budget with instead of an estimate. Needs only the loaded weights'
    /// vocabulary, no context, so it is cheap and safe next to a running generation.
    pub fn count_tokens(&self, text: &str) -> Result<usize, String> {
        self.model
            .str_to_token(text, llama_cpp_2::model::AddBos::Never)
            .map(|t| t.len())
            .map_err(|e| format!("tokenize: {e}"))
    }

    /// One chat completion. `stop` is checked before every prompt chunk and every generated
    /// token: once set (the caller stopped waiting — a stopped chat), generation ends with
    /// `Err("cancelled")` instead of running to the end of the answer.
    #[allow(clippy::too_many_arguments)]
    pub fn generate(&self, messages: &[(&str, &str)], max_tokens: u32, _is_reasoning: bool, json_schema: Option<&str>, kv_quantization: Option<&str>, cpu_threads: Option<i64>, stop: &AtomicBool) -> Result<String, String> {
        // 1. Template: built-in list or the model's Jinja template (`chat_template.rs`).
        let formatted_prompt = super::chat_template::render(&self.model, messages)?;

        // 2. Tokenize (no second BOS when the template wrote one itself)
        let add_bos = if super::chat_template::starts_with_bos(&self.model, &formatted_prompt) {
            llama_cpp_2::model::AddBos::Never
        } else {
            llama_cpp_2::model::AddBos::Always
        };
        let tokens = self.model.str_to_token(&formatted_prompt, add_bos)
            .map_err(|e| format!("tokenize: {e}"))?;

        // Check if prompt fits in context window
        let available_ctx = self.n_ctx.saturating_sub(max_tokens);
        if tokens.len() > available_ctx as usize {
            return Err(format!("Prompt zu lang: {} Tokens. (Verfügbar: {} Kontext - {} Reserve = {})", 
                tokens.len(), self.n_ctx, max_tokens, available_ctx));
        }

        // WARNING: We are ignoring `json_schema` for GGUF models here.
        // llama.cpp's `LlamaSampler::grammar` has a known issue where it aborts/segfaults 
        // the entire process when initializing the state machine for complex, deeply nested 
        // JSON schemas (like our Ontology schema with arrays of objects).
        // The LLM will still output JSON due to the System Prompt, and extract.rs will parse it.
        if json_schema.is_some() {
            println!("WARN: json_schema was passed to GGUF but is ignored to prevent llama.cpp C++ parser crash.");
        }

        let key = SessionKey { kv_quantization: kv_quantization.map(str::to_string), cpu_threads };
        let mut slot = match self.session.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::Poisoned(p)) => {
                let mut slot = p.into_inner();
                *slot = None;
                slot
            }
            Err(TryLockError::WouldBlock) => {
                // Busy: a context of its own for this call, nothing kept.
                let mut ctx = self.new_context(&key)?;
                return self.complete(&mut ctx, &mut Memory::default(), &tokens, None, max_tokens, stop);
            }
        };
        if slot.as_ref().is_some_and(|s| s.key != key) {
            *slot = None;
        }
        if slot.is_none() {
            let ctx = self.new_context(&key)?;
            // SAFETY: see `Session::ctx`.
            let ctx = unsafe { std::mem::transmute::<LlamaContext<'_>, LlamaContext<'static>>(ctx) };
            *slot = Some(Session { ctx, key, memory: Memory::default() });
        }
        let session = slot.as_mut().expect("set above");
        let turn_start = self.turn_start(messages, &tokens, add_bos);
        let res = self.complete(&mut session.ctx, &mut session.memory, &tokens, turn_start, max_tokens, stop);
        if res.is_err() {
            // Memory state unknown: the next call starts fresh (snapshots are copies, still valid).
            session.memory.tokens.clear();
            session.memory.checkpoints.clear();
        }
        res
    }

    /// Where the content of the last message of `messages` begins in `tokens` (the prompt): what
    /// the prompt shares with the same messages and the last one emptied. (Rendering the earlier
    /// messages alone is no fit: templates such as Qwen3's render an assistant turn differently
    /// when it is the last one.) Only for memory that cannot be cut back — attention memory is
    /// cut at any position anyway.
    fn turn_start(&self, messages: &[(&str, &str)], tokens: &[LlamaToken], add_bos: llama_cpp_2::model::AddBos) -> Option<usize> {
        let ((role, _), earlier) = messages.split_last().filter(|(_, e)| self.partial_memory && !e.is_empty())?;
        let emptied: Vec<(&str, &str)> = earlier.iter().copied().chain([(*role, "")]).collect();
        let text = super::chat_template::render(&self.model, &emptied).ok()?;
        let earlier_tokens = self.model.str_to_token(&text, add_bos).ok()?;
        Some(prefix_reuse::shared_len(&earlier_tokens, tokens))
    }

    /// Drops what the engine keeps beyond the live context (snapshots, saved partial states,
    /// the last call's tokens) — under memory pressure (app `localllm`). The next call starts fresh.
    pub fn drop_saved(&self) {
        if let Ok(mut slot) = self.session.try_lock() {
            if let Some(session) = slot.as_mut() {
                session.memory = Memory::default();
                session.ctx.clear_kv_cache();
            }
        }
    }

    fn new_context(&self, key: &SessionKey) -> Result<LlamaContext<'_>, String> {
        let mut ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(self.n_ctx))
            .with_n_batch(CHUNK as u32);
        if let Some(q) = key.kv_quantization.as_deref() {
            let kv_type = match q {
                "Q8_0" => KvCacheType::Q8_0,
                "Q4_0" => KvCacheType::Q4_0,
                _ => KvCacheType::F16,
            };
            ctx_params = ctx_params.with_type_k(kv_type).with_type_v(kv_type);
        }
        if let Some(t) = key.cpu_threads {
            ctx_params = ctx_params.with_n_threads(t as i32).with_n_threads_batch(t as i32);
        }
        self.model.new_context(get_backend()?, ctx_params).map_err(|e| format!("context create: {e}"))
    }

    /// Prepares `ctx` for `tokens` along `prefix_reuse::plan`, or from a snapshot when that keeps
    /// more; returns how many are kept and where to save a snapshot while reading the rest.
    fn reuse(&self, ctx: &mut LlamaContext<'_>, memory: &mut Memory, tokens: &[LlamaToken]) -> (usize, Option<usize>) {
        let shared = prefix_reuse::shared_len(&memory.tokens, tokens);
        let at: Vec<usize> = memory.checkpoints.iter().map(|c| c.0).collect();
        let reuse = prefix_reuse::plan(&memory.tokens, tokens, self.partial_memory, &at);
        let planned = match reuse {
            Reuse::Fresh => 0,
            Reuse::Truncate { keep } | Reuse::Restore { keep } => keep,
        };
        let starts: Vec<&[LlamaToken]> = memory.snapshots.iter().map(|s| s.tokens.as_slice()).collect();
        let keep = if let Some(i) = prefix_reuse::best_snapshot(&starts, tokens, planned) {
            let snapshot = memory.snapshots.remove(i);
            ctx.clear_kv_cache();
            // SAFETY: the bytes were written by `state_seq_get_data_ext` of this context.
            if unsafe { ctx.state_seq_set_data_ext(&snapshot.state, 0, LlamaStateSeqFlags::empty()) } {
                let keep = snapshot.tokens.len();
                // Saved partial states up to what both share describe the same tokens.
                memory.checkpoints.retain(|c| c.0 <= shared.min(keep));
                memory.tokens.clone_from(&snapshot.tokens);
                memory.snapshots.push(snapshot); // now the most recently used
                keep
            } else {
                0
            }
        } else {
            match reuse {
                Reuse::Fresh => 0,
                Reuse::Truncate { keep } => {
                    if ctx.clear_kv_cache_seq(Some(0), Some(keep as u32), None).unwrap_or(false) { keep } else { 0 }
                }
                Reuse::Restore { keep } => {
                    let state = memory.checkpoints.iter().find(|c| c.0 == keep).map(|c| c.1.as_slice()).unwrap_or_default();
                    // SAFETY: the bytes were written by `state_seq_get_data_ext` of this context.
                    let restored = unsafe { ctx.state_seq_set_data_ext(state, 0, LlamaStateSeqFlags::PARTIAL_ONLY) };
                    if restored && ctx.clear_kv_cache_seq(Some(0), Some(keep as u32), None).unwrap_or(false) { keep } else { 0 }
                }
            }
        };
        if keep == 0 {
            ctx.clear_kv_cache();
        }
        memory.tokens.truncate(keep);
        memory.checkpoints.retain(|c| c.0 <= keep);
        (keep, prefix_reuse::snapshot_at(shared, keep, tokens.len()))
    }

    /// Copies the whole memory (`tokens` read so far) into a snapshot, within the size bounds.
    fn save_snapshot(ctx: &LlamaContext<'_>, memory: &mut Memory, tokens: &[LlamaToken]) {
        let size = ctx.state_seq_get_size_ext(0, LlamaStateSeqFlags::empty());
        if size > prefix_reuse::SNAPSHOT_BYTES {
            return;
        }
        let mut state = vec![0u8; size];
        // SAFETY: `state` holds the size llama.cpp asked for.
        let written = unsafe { ctx.state_seq_get_data_ext(state.as_mut_ptr(), 0, LlamaStateSeqFlags::empty()) };
        state.truncate(written);
        memory.snapshots.push(Snapshot { tokens: tokens.to_vec(), state });
        let sizes: Vec<usize> = memory.snapshots.iter().map(|s| s.state.len()).collect();
        memory.snapshots.drain(..prefix_reuse::snapshots_to_drop(&sizes));
    }

    /// Reads the prompt (from what `reuse` kept on) and samples the answer into `ctx`.
    fn complete(&self, ctx: &mut LlamaContext<'_>, memory: &mut Memory, tokens: &[LlamaToken], turn_start: Option<usize>, max_tokens: u32, stop: &AtomicBool) -> Result<String, String> {
        let stopped = || stop.load(Ordering::Relaxed);
        let started = std::time::Instant::now();
        let (keep, snapshot_at) = self.reuse(ctx, memory, tokens);

        // 3. Eval the rest of the prompt in chunks; save the partial state at the checkpoints.
        let checkpoints = if self.partial_memory { prefix_reuse::checkpoints_for(keep, tokens.len(), turn_start) } else { Vec::new() };
        let mut batch = llama_cpp_2::llama_batch::LlamaBatch::new(CHUNK, 1);
        let mut pos = keep;
        while pos < tokens.len() {
            if stopped() {
                return Err("cancelled".to_string());
            }
            let next_checkpoint = checkpoints.iter().copied().find(|&c| c > pos).unwrap_or(usize::MAX);
            let next_snapshot = snapshot_at.filter(|&at| at > pos).unwrap_or(usize::MAX);
            let end = (pos + CHUNK).min(tokens.len()).min(next_checkpoint).min(next_snapshot);
            batch.clear();
            for (i, &tok) in tokens[pos..end].iter().enumerate() {
                let p = pos + i;
                batch.add(tok, p as i32, &[0], p == tokens.len() - 1).map_err(|e| format!("batch: {e}"))?;
            }
            ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
            memory.tokens.extend_from_slice(&tokens[pos..end]);
            pos = end;
            if pos == next_checkpoint {
                let size = ctx.state_seq_get_size_ext(0, LlamaStateSeqFlags::PARTIAL_ONLY);
                let mut state = vec![0u8; size];
                // SAFETY: `state` holds the size llama.cpp asked for.
                let written = unsafe { ctx.state_seq_get_data_ext(state.as_mut_ptr(), 0, LlamaStateSeqFlags::PARTIAL_ONLY) };
                state.truncate(written);
                memory.checkpoints.push((pos, state));
                let keep: Vec<usize> = prefix_reuse::retain(memory.checkpoints.iter().map(|c| c.0).collect(), turn_start);
                memory.checkpoints.retain(|c| keep.contains(&c.0));
            }
            if pos == next_snapshot {
                Self::save_snapshot(ctx, memory, &tokens[..pos]);
            }
        }
        let mut n_past = tokens.len() as i32;
        let read = started.elapsed().as_secs_f64();

        let samplers = vec![
            llama_cpp_2::sampling::LlamaSampler::penalties(64, 1.1, 0.0, 0.0),
            llama_cpp_2::sampling::LlamaSampler::top_k(40),
            llama_cpp_2::sampling::LlamaSampler::top_p(0.9, 1),
            llama_cpp_2::sampling::LlamaSampler::temp(0.7),
            llama_cpp_2::sampling::LlamaSampler::dist(1337),
        ];
        let mut chain = llama_cpp_2::sampling::LlamaSampler::chain_simple(samplers);

        // Accept prompt tokens into the chain to avoid repeating the prompt
        for &t in tokens {
            chain.accept(t);
        }

        // 4. Sampling loop
        // Raw bytes, decoded once at the end: a multi-byte UTF-8 character (e.g. the narrow
        // no-break space in "z. B.") can be split across two tokens, and decoding each
        // piece on its own turned it into U+FFFD pairs.
        let mut output: Vec<u8> = Vec::new();
        for _ in 0..max_tokens {
            if stopped() {
                return Err("cancelled".to_string());
            }
            let next_token = chain.sample(ctx, batch.n_tokens() - 1);
            
            // Check EOS/EOG (handles ChatML <|im_end|>, Llama <|eot_id|>, etc automatically)
            if self.model.is_eog_token(next_token) {
                break;
            }

            chain.accept(next_token);

            if let Ok(token_bytes) = self.model.token_to_piece_bytes(next_token, 128, true, None) {
                output.extend_from_slice(&token_bytes);
            }

            batch.clear();
            batch.add(next_token, n_past, &[0], true).map_err(|e| format!("batch: {e}"))?;
            n_past += 1;

            ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
            memory.tokens.push(next_token);
        }

        eprintln!(
            "gguf: prompt {} tokens ({keep} kept from earlier calls) read in {read:.2} s, answer {} tokens in {:.2} s",
            tokens.len(),
            n_past as usize - tokens.len(),
            started.elapsed().as_secs_f64() - read
        );
        // gpt-oss: only the Harmony `final` channel is the answer.
        super::chat_text::harmony_final(&String::from_utf8_lossy(&output))
    }
}
