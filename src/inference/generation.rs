//! What one call of the local chat engine (`gguf.rs`) returns: the answer text and what it cost —
//! prompt tokens (all of them, and how many the engine kept from its previous call instead of
//! reading them again, `prefix_reuse.rs`), answer tokens and the time for each part, and whether
//! the answer budget ran out before the model ended its turn (`hit_limit`, decided once for both
//! local engines by [`write_until_limit`]). The app puts the counts into the tool loop's per-round
//! usage (`llm_tools_gguf`), the same field hosted providers fill from their response, and turns
//! `hit_limit` into the "cut off" notice under a chat answer. llama.cpp-free, so it exists without
//! the `gguf` feature.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Generation {
    pub text: String,
    /// Tokens of the whole prompt (chat template applied).
    pub prompt_tokens: usize,
    /// Of those, the tokens kept from the engine's previous call (not read again).
    pub kept_tokens: usize,
    /// Time to read the rest of the prompt (restoring a saved beginning included).
    pub prompt_secs: f64,
    /// Tokens the model wrote (the end-of-turn token not counted).
    pub answer_tokens: usize,
    /// Time to write them.
    pub answer_secs: f64,
    /// The model was still writing when `max_tokens` (the endpoint's Output Reserve) ran out:
    /// the answer is cut off, not finished.
    pub hit_limit: bool,
}

impl Generation {
    /// Prompt tokens actually read in this call.
    pub fn read_tokens(&self) -> usize {
        self.prompt_tokens.saturating_sub(self.kept_tokens)
    }
}

/// Tokens per second, `None` without measurable time or without tokens.
pub fn per_second(tokens: f64, secs: f64) -> Option<f64> {
    (secs > 0.0 && tokens > 0.0).then(|| tokens / secs)
}

/// Writes at most `max_tokens` tokens: `step` writes one and answers `Ok(false)` once the model
/// ended its turn (end-of-generation token). Returns whether the budget ran out first, i.e. the
/// answer is cut off. The chat engine (`gguf.rs`) and the picture engine (`gguf_vlm.rs`) both
/// generate through this, so "stopped at the limit" means the same in both.
pub fn write_until_limit(max_tokens: u32, mut step: impl FnMut() -> Result<bool, String>) -> Result<bool, String> {
    for _ in 0..max_tokens {
        if !step()? {
            return Ok(false);
        }
    }
    Ok(true)
}
