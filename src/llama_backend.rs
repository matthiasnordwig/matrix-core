//! Process-global llama.cpp backend singleton, shared by every GGUF consumer
//! (LLM engine `inference/gguf.rs`, embedder `embedding/gguf_embed.rs`,
//! reranker `embedding/gguf_rerank.rs`).
//!
//! llama.cpp allows exactly one backend init per process. Three module-local
//! `OnceLock` singletons each calling `LlamaBackend::init()` meant whichever
//! module initialized *second* failed with `BackendAlreadyInitialized` (seen
//! in the wild: local-LLM grid run first, GGUF query embedding afterwards).
//! All modules must go through this one accessor.
//!
//! The same goes for where a model's layers run ([`model_params_on`]): every GGUF consumer
//! loads with these parameters; which unit (graphics or CPU) is `crate::compute`'s rule
//! ([`rule_unit`] for the small models, `inference/gguf_device.rs` for the chat model).
//!
//! How large a context fits on the graphics unit next to the whole model ([`context_on_gpu`]) is
//! llama.cpp's own projection (`llama_params_fit` with the context left open): Metal counts what
//! the system lets the GPU take minus what is allocated, Vulkan only device-local heaps (budget −
//! usage; shared graphics memory never counts). The services choose the chat context with it
//! (`local_context.rs`).

use std::sync::{Mutex, OnceLock};

use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::LlamaBackendDeviceType;

use crate::compute::{Fit, Gpu, Unit};

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
    #[cfg(feature = "gguf-vulkan")]
    load_backend_dlls();
    let b = LlamaBackend::init().map_err(|e| format!("llama backend init: {e}"))?;
    let _ = BACKEND.set(b);
    BACKEND
        .get()
        .ok_or_else(|| "failed to set llama backend".to_string())
}

/// `gguf-vulkan`: llama.cpp's backends are DLLs (`ggml-cpu-<variant>.dll`, `ggml-vulkan.dll`);
/// llama.cpp loads the best CPU variant and every GPU backend whose driver loads. Shipped
/// beside the exe; a development build (`cargo run`, examples) finds them in the build
/// folder llama-cpp-sys-2 wrote them to.
#[cfg(feature = "gguf-vulkan")]
fn load_backend_dlls() {
    use llama_cpp_2::llama_backend::{load_backends, load_backends_from_path};
    let beside_exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let has_cpu = |dir: &std::path::Path| {
        std::fs::read_dir(dir).is_ok_and(|entries| {
            entries.flatten().any(|e| e.file_name().to_string_lossy().starts_with("ggml-cpu"))
        })
    };
    match beside_exe {
        Some(dir) if has_cpu(&dir) => load_backends_from_path(&dir),
        _ => load_backends(),
    }
}

/// Parameters every GGUF model loads with on `unit`: weights memory-mapped. Graphics unit:
/// Metal all layers; Vulkan (`gguf-vulkan`) llama.cpp's default, all layers on the dedicated
/// cards it found (an integrated unit only without one). CPU: no graphics device at all (an
/// empty device list, else llama.cpp still hands single large operations to the card). The
/// iOS Simulator is always CPU: its Metal runs llama.cpp's kernels wrongly (garbage, no error).
pub fn model_params_on(unit: Unit) -> LlamaModelParams {
    let params = LlamaModelParams::default().with_use_mmap(true);
    if unit == Unit::Cpu || cfg!(all(target_os = "ios", target_abi = "sim")) {
        let params = params.with_n_gpu_layers(0);
        return params.with_devices(&[]).unwrap_or_else(|_| LlamaModelParams::default().with_use_mmap(true).with_n_gpu_layers(0));
    }
    if cfg!(feature = "gguf-metal") {
        params.with_n_gpu_layers(999)
    } else {
        params
    }
}

/// The graphics units llama.cpp found (dedicated and integrated), for the rule and the key of a
/// stored test (`crate::compute`). Initializes the backend.
pub fn gpus() -> Vec<Gpu> {
    if backend().is_err() {
        return Vec::new();
    }
    llama_cpp_2::list_llama_ggml_backend_devices()
        .into_iter()
        .filter(|d| matches!(d.device_type, LlamaBackendDeviceType::Gpu | LlamaBackendDeviceType::IntegratedGpu))
        .map(|d| Gpu {
            driver: crate::compute::driver_version(&d.description),
            integrated: d.device_type == LlamaBackendDeviceType::IntegratedGpu,
            memory_total: d.memory_total as u64,
            name: d.description,
        })
        .collect()
}

/// The unit for the small GGUF models (embedder, reranker, picture model), which get no quick
/// test of their own: graphics with Metal or a dedicated card, else the CPU.
pub fn rule_unit() -> Unit {
    if cfg!(feature = "gguf-metal") || gpus().iter().any(|g| !g.integrated) {
        Unit::Gpu
    } else {
        Unit::Cpu
    }
}

/// Graphics memory kept free for the desktop and other programs when a model is placed on the
/// graphics unit ([`fit_to_gpu_memory`], [`context_on_gpu`]); llama.cpp's own tools keep 1 GiB.
/// Measured against the real use (`handbuch/chat-messungen.md` "Speichergrenze").
pub const GPU_MARGIN: u64 = 1 << 30;

/// Vulkan: fewer layers on the graphics unit where the model plus a context of `n_ctx` tokens
/// (F16 memory, the largest the chat engine makes) would not fit its free memory, keeping
/// [`GPU_MARGIN`] (llama.cpp's `llama_params_fit`). Fills `params` in place and says how much fits; a hard error (unreadable file) leaves
/// `params` for the load to report. Other builds: nothing to fit.
pub fn fit_to_gpu_memory(
    #[allow(unused_mut)] mut params: std::pin::Pin<&mut LlamaModelParams>,
    model_path: &str,
    n_ctx: u32,
    n_batch: u32,
) -> Option<Fit> {
    #[cfg(feature = "gguf-vulkan")]
    {
        use llama_cpp_2::context::params::LlamaContextParams;
        use llama_cpp_2::model::params::FitError;
        const GGML_LOG_LEVEL_WARN: u32 = 3;
        let path = std::ffi::CString::new(model_path).ok()?;
        let mut cparams = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(n_ctx))
            .with_n_batch(n_batch);
        let mut margins = vec![gpu_margin() as usize; llama_cpp_2::max_devices()];
        match params.as_mut().fit_params(&path, &mut cparams, &mut margins, n_ctx, GGML_LOG_LEVEL_WARN as _) {
            Err(FitError::Failure) => {
                params.set(model_params_on(Unit::Cpu));
                Some(Fit::Nothing)
            }
            Err(FitError::Error) => None,
            // Left at llama.cpp's default (-1 = every layer) where all of it fits.
            Ok(_) => Some(match params.n_gpu_layers() {
                n if n < 0 => Fit::All,
                0 => Fit::Nothing,
                _ => Fit::Part,
            }),
        }
    }
    #[cfg(not(feature = "gguf-vulkan"))]
    {
        let _ = (params, model_path, n_ctx, n_batch);
        None
    }
}

/// The largest context (tokens) with which the whole model at `model_path` fits the graphics unit,
/// keeping `margin` bytes free per device: llama.cpp projects weights, KV cache (`kv`: `Q8_0`,
/// `Q4_0`, else F16 — as the chat engine sets it) and compute buffers for a batch of `n_batch`,
/// and shrinks the context from the model's trained length down to `n_ctx_min` until all of it
/// fits. `Some(0)`: the model's whole trained length fits. `None` when not even `n_ctx_min` fits
/// next to every layer, without a graphics unit, or when the file cannot be read.
pub fn context_on_gpu(model_path: &str, n_batch: u32, kv: Option<&str>, n_ctx_min: u32, margin: u64) -> Option<u32> {
    use llama_cpp_2::context::params::{KvCacheType, LlamaContextParams};
    const GGML_LOG_LEVEL_WARN: u32 = 3;
    let metal = cfg!(feature = "gguf-metal") && !cfg!(all(target_os = "ios", target_abi = "sim"));
    if !metal && gpus().is_empty() {
        return None; // CPU build: llama.cpp would report "no device" as a fit.
    }
    let path = std::ffi::CString::new(model_path).ok()?;
    let mut cparams = LlamaContextParams::default().with_n_ctx(None).with_n_batch(n_batch);
    if let Some(t) = kv.and_then(|q| match q {
        "Q8_0" => Some(KvCacheType::Q8_0),
        "Q4_0" => Some(KvCacheType::Q4_0),
        _ => None,
    }) {
        cparams = cparams.with_type_k(t).with_type_v(t);
    }
    // llama.cpp's defaults (every layer on the graphics unit): fit refuses parameters it did not set.
    let mut params = std::pin::pin!(LlamaModelParams::default().with_use_mmap(true));
    let mut margins = vec![usize::try_from(margin).unwrap_or(usize::MAX); llama_cpp_2::max_devices()];
    let fit = params.as_mut().fit_params(&path, &mut cparams, &mut margins, n_ctx_min, GGML_LOG_LEVEL_WARN as _).ok()?;
    // Fewer layers on the unit = the model does not fit whole even at `n_ctx_min`.
    (params.n_gpu_layers() < 0).then_some(fit.n_ctx)
}

/// Whether the graphics unit takes its memory from main memory: Apple's GPU, or only integrated
/// units (a dedicated card has its own). Also true without any graphics unit.
pub fn gpu_shares_ram() -> bool {
    cfg!(feature = "gguf-metal") || gpus().iter().all(|g| g.integrated)
}

/// Free memory of the dedicated graphics units now (bytes; integrated units and Apple's GPU share
/// main memory and do not count), less what other programs hold that the backend does not see
/// ([`hidden_gpu_use`]). 0 without one.
pub fn dedicated_free_bytes() -> u64 {
    if cfg!(feature = "gguf-metal") || backend().is_err() {
        return 0;
    }
    let free: u64 = dedicated_cards().iter().map(|&(free, _)| free).sum();
    free.saturating_sub(hidden_gpu_use())
}

/// (free, total) of every dedicated graphics unit as llama.cpp reports it.
fn dedicated_cards() -> Vec<(u64, u64)> {
    llama_cpp_2::list_llama_ggml_backend_devices()
        .into_iter()
        .filter(|d| d.device_type == LlamaBackendDeviceType::Gpu)
        .map(|d| (d.memory_free as u64, d.memory_total as u64))
        .collect()
}

/// Graphics memory to keep free when a model is placed: [`GPU_MARGIN`] plus what other programs hold
/// that the backend's free count misses ([`hidden_gpu_use`]). Used by [`fit_to_gpu_memory`] and the
/// services' context choice (`context_on_gpu`).
pub fn gpu_margin() -> u64 {
    GPU_MARGIN + hidden_gpu_use()
}

/// Memory of the largest dedicated card that other programs hold but llama.cpp counts as free
/// (`device_memory::foreign_gpu_use` over the system counter, `gpu_dedicated_use`): never this
/// process's own, so a model just freed for the next one does not count against it. Only Windows
/// reports the counter; 0 elsewhere and without a dedicated card.
fn hidden_gpu_use() -> u64 {
    let Some(used) = crate::device_memory::gpu_dedicated_use() else { return 0 };
    if cfg!(feature = "gguf-metal") || backend().is_err() {
        return 0;
    }
    dedicated_cards()
        .into_iter()
        .max_by_key(|&(_, total)| total)
        .map_or(0, |(free, total)| crate::device_memory::foreign_gpu_use(free, total, used))
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
