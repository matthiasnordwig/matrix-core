//! Where a local model runs — graphics unit or CPU — for every model kind (chat model through
//! llama.cpp, search model and reranker through ONNX): the one rule, pure and tested here; the
//! measuring lives with the engine (`inference/gguf_device.rs`), the stored results and the
//! progress line in the app (`services/compute_choice.rs`).
//!
//! 1. **Rule, free:** [`gguf_verdict`] from the devices llama.cpp reports. No graphics unit, or a
//!    model that does not fit: CPU. Apple's Metal and a dedicated card the whole model plus its
//!    context fits on: graphics unit. Everything else (an integrated graphics unit, part of the
//!    layers only): a quick test. The ONNX models (search model, ONNX reranker) stay on the CPU:
//!    DirectML was slower there at the chunk sizes Matrix cuts (`handbuch/release-windows.md`).
//! 2. **Quick test, once:** the engine measures both ways ([`Timing`]) and [`judge`] decides. The
//!    result is kept under [`probe_key`] (device, driver, model file, build), so it runs again only
//!    when one of them changes.
//!
//! [`judge`] errs towards the CPU: a failed or deviating graphics run, or a gain below
//! [`MIN_SPEEDUP`] (noise, other load, power saving), keeps the CPU.

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

/// Where a model runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Gpu,
    Cpu,
}

/// A graphics unit as a backend reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gpu {
    pub name: String,
    /// Built into the processor (shares its memory) rather than a card of its own.
    pub integrated: bool,
    pub memory_total: u64,
    /// Version of the graphics driver where the platform tells it (part of [`probe_key`]).
    pub driver: String,
}

/// What the free query decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// No test needed; the reason goes to the log.
    Use(Unit, &'static str),
    /// Unclear: measure once ([`judge`]).
    Probe(&'static str),
}

/// How much of a model plus its context fits the free graphics memory (llama.cpp's fitting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    All,
    Part,
    Nothing,
}

/// Chat model through llama.cpp. `metal`: Apple's graphics unit (always faster there, measured in
/// `handbuch/chat.md`). `fit`: `None` where nothing was fitted (no fitting in this build, or an
/// unreadable file the load will report).
pub fn gguf_verdict(metal: bool, gpus: &[Gpu], fit: Option<Fit>) -> Verdict {
    if metal {
        return Verdict::Use(Unit::Gpu, "Metal");
    }
    if gpus.is_empty() {
        return Verdict::Use(Unit::Cpu, "no graphics unit");
    }
    match fit {
        Some(Fit::Nothing) => Verdict::Use(Unit::Cpu, "model does not fit graphics memory"),
        Some(Fit::Part) => Verdict::Probe("only part of the model fits graphics memory"),
        _ if gpus.iter().all(|g| g.integrated) => Verdict::Probe("integrated graphics unit"),
        _ => Verdict::Use(Unit::Gpu, "dedicated graphics card, model fits"),
    }
}

/// Smallest gain (CPU time / graphics time) for which the graphics unit is chosen.
pub const MIN_SPEEDUP: f64 = 1.3;

/// One way's time for the test's fixed work, in milliseconds. `None` for the CPU: stopped early
/// because it was already [`CLEARLY_SLOWER`] than the graphics unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Timing {
    pub ms: f64,
}

/// CPU time from which the test stops measuring it (the graphics unit is clearly faster).
pub const CLEARLY_SLOWER: f64 = 3.0;

/// A quick test's measurements; `gpu: None` = the graphics run failed. `agree`: both ways
/// continue the same text with the same first tokens (a driver fault shows as other tokens).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    pub gpu: Option<Timing>,
    pub cpu: Option<Timing>,
    pub agree: bool,
}

/// The test's decision and its reason (for the log).
pub fn judge(p: &Probe) -> (Unit, String) {
    let Some(gpu) = p.gpu else {
        return (Unit::Cpu, "graphics run failed".into());
    };
    if !p.agree {
        return (Unit::Cpu, "graphics results deviate from the CPU's".into());
    }
    let Some(cpu) = p.cpu else {
        return (Unit::Gpu, format!("CPU over {CLEARLY_SLOWER}x slower, stopped"));
    };
    let speedup = cpu.ms / gpu.ms.max(0.001);
    let unit = if speedup >= MIN_SPEEDUP { Unit::Gpu } else { Unit::Cpu };
    (unit, format!("graphics {:.0} ms, CPU {:.0} ms ({speedup:.2}x)", gpu.ms, cpu.ms))
}

/// Whether the CPU run may stop: after `done` of `total` steps its projected time is already
/// [`CLEARLY_SLOWER`] than the graphics unit's whole run.
pub fn cpu_clearly_slower(cpu_ms_so_far: f64, done: usize, total: usize, gpu_ms: f64) -> bool {
    done > 0 && cpu_ms_so_far * total as f64 / done as f64 > CLEARLY_SLOWER * gpu_ms
}

/// Key of a stored test result: model kind and file (name and size), every graphics unit with
/// its driver, and the build (`version`, a new llama.cpp/ONNX Runtime can change the outcome).
pub fn probe_key(kind: &str, model_file: &str, model_bytes: u64, gpus: &[Gpu], version: &str) -> String {
    let name = model_file.rsplit(['/', '\\']).next().unwrap_or(model_file);
    let devices: Vec<String> = gpus.iter().map(|g| format!("{}@{}", g.name, g.driver)).collect();
    format!("{kind}|{name}|{model_bytes}|{}|{version}", devices.join("+"))
}

/// Version of the graphics driver for the device llama.cpp names `description` (Windows: the
/// display adapter's `DriverVersion` in the registry); empty where unknown.
pub fn driver_version(description: &str) -> String {
    #[cfg(all(windows, feature = "gguf-vulkan"))]
    {
        windows_driver::version(description).unwrap_or_default()
    }
    #[cfg(not(all(windows, feature = "gguf-vulkan")))]
    {
        let _ = description;
        String::new()
    }
}

#[cfg(all(windows, feature = "gguf-vulkan"))]
mod windows_driver {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

    const DISPLAY_CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn value(subkey: &str, name: &str) -> Option<String> {
        let mut buf = [0u16; 512];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: wide strings are NUL-terminated, `len` is the buffer's size in bytes.
        let rc = unsafe {
            RegGetValueW(HKEY_LOCAL_MACHINE, wide(subkey).as_ptr(), wide(name).as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len)
        };
        (rc == 0).then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
    }

    /// The adapter whose `DriverDesc` is `description` (numbered subkeys 0000, 0001, …).
    pub fn version(description: &str) -> Option<String> {
        (0..32).find_map(|i| {
            let key = format!(r"{DISPLAY_CLASS}\{i:04}");
            (value(&key, "DriverDesc")?.trim() == description.trim()).then(|| value(&key, "DriverVersion")).flatten()
        })
    }
}
