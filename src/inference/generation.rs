//! What one call of the local chat engine (`gguf.rs`) returns: the answer text and what it cost —
//! prompt tokens (all of them, and how many the engine kept from its previous call instead of
//! reading them again, `prefix_reuse.rs`), answer tokens and the time for each part. The app puts
//! the counts into the tool loop's per-round usage (`llm_tools_gguf`), the same field hosted
//! providers fill from their response. llama.cpp-free, so it exists without the `gguf` feature.

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
