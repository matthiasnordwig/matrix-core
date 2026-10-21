//! On-device generation through llama.cpp: `gguf` (chat), `gguf_vlm` (picture
//! + instruction via mtmd) and their shared prompt builder `chat_template` —
//! behind the `gguf` feature — plus the llama.cpp-free text helpers
//! (`chat_text`: template route + Harmony answer, `prefix_reuse`: what of the last
//! call the chat engine keeps, `vlm_prompt`).
#[cfg(feature = "gguf")]
mod chat_template;
pub mod chat_text;
pub mod prefix_reuse;
#[cfg(feature = "gguf")]
pub mod gguf;
#[cfg(feature = "gguf")]
pub mod gguf_vlm;
pub mod vlm_prompt;

#[cfg(test)]
mod tests;
