//! Where an ONNX session runs — one place for every ONNX model Matrix loads (search model and its
//! image encoder; the app's PaddleOCR builds its sessions here too; the ONNX reranker keeps its own Apple EP choice in `rerank.rs`). Compiled only
//! under the `onnx` feature.
//!
//! [`session_on`] turns a [`Unit`] into a session builder: the graphics unit is DirectML on Windows
//! (the ONNX Runtime `ort` downloads for Windows carries it; DirectML wants no memory pattern and
//! sequential execution) and CoreML (`All`) on Apple; Linux has no graphics path here and stays on
//! the CPU. A graphics provider that cannot start (no DirectX 12 device, missing `DirectML.dll`)
//! leaves the session on the CPU: ONNX Runtime falls back by itself, so a load never fails for it.
//! Every model loads on the CPU (measured: the graphics unit was slower or failed, `handbuch/core.md`
//! "Rechenweg"); the graphics unit is asked for only by the measurements (`examples/onnx_unit_bench.rs`,
//! services `paddle_measure.rs`), which repeat that comparison.

use ort::session::builder::{GraphOptimizationLevel, SessionBuilder};
use ort::session::Session;

use crate::compute::Unit;
use crate::Result;

/// Session builder for `unit`, graph optimisation at the highest level.
#[cfg_attr(not(any(windows, target_vendor = "apple")), allow(unused_mut))]
pub fn session_on(unit: Unit) -> Result<SessionBuilder> {
    let mut builder = Session::builder()?;
    if unit == Unit::Gpu {
        #[cfg(windows)]
        {
            builder = builder
                .with_execution_providers([ort::ep::DirectML::default().build()])?
                .with_memory_pattern(false)?
                .with_parallel_execution(false)?;
        }
        #[cfg(target_vendor = "apple")]
        {
            use ort::ep::coreml::{ComputeUnits, CoreML};
            builder = builder.with_execution_providers([CoreML::default().with_compute_units(ComputeUnits::All).build()])?;
        }
    }
    Ok(builder.with_optimization_level(GraphOptimizationLevel::Level3)?)
}
