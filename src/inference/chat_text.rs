//! Pure text parts of the local chat engines (`gguf.rs`, `gguf_vlm.rs`), kept
//! free of llama.cpp so they build and test without the `gguf` feature.
//!
//! Two decisions live here:
//! - **Which chat template path** (`route_for`): llama.cpp's built-in template
//!   list matches a model's template by its look (ChatML, Llama 3 …) and knows
//!   no switches. Templates with a thinking switch (Qwen3/3.5/3.6, Gemma 4:
//!   `enable_thinking`) or OpenAI's Harmony format (gpt-oss: `<|channel|>`) go
//!   through the model's own Jinja template instead — thinking off, gpt-oss
//!   with low reasoning effort (it cannot switch reasoning off). Everything
//!   else stays on the built-in list; a template the list does not know falls
//!   back to Jinja (`chat_template.rs`).
//! - **The answer out of a Harmony reply** (`harmony_final`): gpt-oss writes
//!   its reasoning into an `analysis` channel and the answer into a `final`
//!   channel; only the latter is the answer. Inline think tags of other models
//!   stay for the services' `llm::strip_reasoning_checked` (chunking needs them
//!   raw), Harmony markers are a local-model format and are resolved here, so
//!   every consumer of the local engine (chat, extraction, tool loop) gets
//!   plain text.

/// How to turn the chat messages into the prompt text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateRoute {
    /// llama.cpp's built-in template list (Jinja only if the list fails).
    Builtin,
    /// The model's Jinja template, thinking off, with these extra template
    /// variables (a JSON object) if any.
    Jinja { kwargs: Option<&'static str> },
}

/// gpt-oss cannot switch reasoning off; "low" keeps the analysis channel short.
pub const HARMONY_KWARGS: &str = r#"{"reasoning_effort":"low"}"#;

/// Route for the model's template source text (`tokenizer.chat_template`).
pub fn route_for(template: &str) -> TemplateRoute {
    if template.contains("<|channel|>") {
        TemplateRoute::Jinja { kwargs: Some(HARMONY_KWARGS) }
    } else if template.contains("enable_thinking") {
        TemplateRoute::Jinja { kwargs: None }
    } else {
        TemplateRoute::Builtin
    }
}

const CHANNEL: &str = "<|channel|>";
const MESSAGE: &str = "<|message|>";
const MESSAGE_ENDS: [&str; 4] = ["<|end|>", "<|return|>", "<|call|>", "<|start|>"];

/// Shown when a Harmony reply never reached its `final` channel.
pub const HARMONY_TRUNCATED: &str = "Response was truncated: the model hit the token limit before finishing its reasoning. Increase 'Output Reserve Tokens' and/or 'Context Window' in the endpoint settings.";

/// The answer of a Harmony reply: the text of its last `final` channel (up to
/// that message's end marker), trimmed. Text without Harmony markers is
/// returned unchanged. A reply with channels but no `final` one (cut off while
/// reasoning) is an error.
pub fn harmony_final(raw: &str) -> Result<String, String> {
    if !raw.contains(CHANNEL) {
        return Ok(raw.to_string());
    }
    let answer = raw.split(CHANNEL).skip(1).filter_map(|segment| {
        let (header, body) = segment.split_once(MESSAGE)?;
        // Header: channel name, optionally followed by ` to=…` / `<|constrain|>…`.
        let name = header.split(|c: char| c.is_whitespace() || c == '<').next().unwrap_or("");
        (name == "final").then_some(body)
    });
    let Some(body) = answer.last() else {
        return Err(HARMONY_TRUNCATED.to_string());
    };
    let end = MESSAGE_ENDS.iter().filter_map(|m| body.find(m)).min().unwrap_or(body.len());
    Ok(body[..end].trim().to_string())
}
