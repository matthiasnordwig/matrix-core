//! Pure text parts of the local vision engine (`gguf_vlm.rs`), kept free of
//! llama.cpp so they build and test without the `gguf` feature.
//!
//! llama.cpp's multimodal library (mtmd) finds the picture by a marker string
//! inside the prompt text (default `<__media__>`) and swaps the image tokens in
//! there. The marker goes into the *user* turn before the instruction (the
//! order the models were trained on: picture first, then the question); the
//! chat template of the model wraps that turn afterwards.

/// The marker mtmd looks for when the caller does not set its own
/// (`mtmd_default_marker()` in llama.cpp; same value).
pub const DEFAULT_MEDIA_MARKER: &str = "<__media__>";

/// User turn text for one picture: `marker`, newline, `prompt` (trimmed). A
/// marker already inside the prompt would make mtmd expect a second picture
/// (bitmap count mismatch), so it is removed from the instruction first.
pub fn user_turn_with_image(prompt: &str, marker: &str) -> String {
    let instruction = if marker.is_empty() { prompt.to_string() } else { prompt.replace(marker, "") };
    format!("{marker}\n{}", instruction.trim())
}

/// Context size for one call: prompt + answer rounded up to a multiple of 256,
/// never more than the endpoint's `n_ctx` (a full-size context would reserve
/// KV memory the single picture never uses — costly on the iPhone).
pub fn context_for(prompt_tokens: usize, max_tokens: u32, n_ctx: u32) -> u32 {
    let need = (prompt_tokens as u64).saturating_add(max_tokens as u64);
    let rounded = need.div_ceil(256).saturating_mul(256).max(256);
    rounded.min(n_ctx as u64) as u32
}

/// Whether a prompt of `prompt_tokens` (text + image tokens) leaves room for
/// `max_tokens` of answer in a context of `n_ctx` tokens.
pub fn fits_context(prompt_tokens: usize, max_tokens: u32, n_ctx: u32) -> bool {
    (prompt_tokens as u64).saturating_add(max_tokens as u64) <= n_ctx as u64
}
