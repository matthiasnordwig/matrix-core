//! On-device generation through llama.cpp: `gguf` (chat), `gguf_vlm` (picture
//! + instruction via mtmd) — both behind the `gguf` feature — and the
//! llama.cpp-free text helpers of the vision engine (`vlm_prompt`).
#[cfg(feature = "gguf")]
pub mod gguf;
#[cfg(feature = "gguf")]
pub mod gguf_vlm;
pub mod vlm_prompt;

#[cfg(test)]
mod tests;
