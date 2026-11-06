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

/// Vulkan: fewer layers on the graphics unit where the model plus a context of `n_ctx` tokens
/// (F16 memory, the largest the chat engine makes) would not fit its free memory, keeping 1 GiB
/// for the desktop and other programs (llama.cpp's `llama_params_fit`, the margin of its own
/// tools). Fills `params` in place and says how much fits; a hard error (unreadable file) leaves
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
        let mut margins = vec![1usize << 30; llama_cpp_2::max_devices()];
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
