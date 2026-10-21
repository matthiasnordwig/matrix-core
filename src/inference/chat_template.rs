//! Chat messages → prompt text for the local engines (`gguf.rs` text,
//! `gguf_vlm.rs` picture + instruction): the one place that applies a GGUF
//! model's chat template. Which path a template takes is decided by the pure
//! `chat_text::route_for` — llama.cpp's built-in template list, or the model's
//! own Jinja template with thinking off (Qwen3.5/3.6 and Gemma 4 thinking
//! switch, gpt-oss Harmony with low reasoning effort); a template the built-in
//! list does not know falls back to Jinja too.

use llama_cpp_2::model::{LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::openai::OpenAIChatTemplateParams;

use super::chat_text::{route_for, TemplateRoute};

/// Prompt text for `messages` (`(role, content)` pairs), ending with the
/// assistant turn's opening.
pub fn render(model: &LlamaModel, messages: &[(&str, &str)]) -> Result<String, String> {
    let tmpl = model.chat_template(None).map_err(|e| format!("model has no default chat template or invalid: {e}"))?;
    match route_for(tmpl.to_str().unwrap_or("")) {
        TemplateRoute::Jinja { kwargs } => jinja(model, &tmpl, messages, kwargs),
        TemplateRoute::Builtin => {
            let chat = messages
                .iter()
                .map(|(role, content)| LlamaChatMessage::new(role.to_string(), content.to_string()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("invalid chat message: {e}"))?;
            match model.apply_chat_template(&tmpl, &chat, true) {
                Ok(prompt) => Ok(prompt),
                // Not in llama.cpp's list (e.g. Gemma 4's new turn tokens).
                Err(_) => jinja(model, &tmpl, messages, None),
            }
        }
    }
}

/// Whether `prompt` already starts with the model's BOS token text (Jinja
/// templates such as Gemma's write `{{ bos_token }}` themselves) — then the
/// tokenizer must not add a second one.
pub fn starts_with_bos(model: &LlamaModel, prompt: &str) -> bool {
    model
        .token_to_piece_bytes(model.token_bos(), 32, true, None)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .is_some_and(|bos| !bos.is_empty() && prompt.starts_with(&bos))
}

fn jinja(model: &LlamaModel, tmpl: &LlamaChatTemplate, messages: &[(&str, &str)], kwargs: Option<&str>) -> Result<String, String> {
    let json = serde_json::Value::Array(
        messages.iter().map(|(role, content)| serde_json::json!({ "role": role, "content": content })).collect(),
    )
    .to_string();
    let params = OpenAIChatTemplateParams {
        messages_json: &json,
        tools_json: None,
        tool_choice: None,
        json_schema: None,
        grammar: None,
        reasoning_format: None,
        chat_template_kwargs: kwargs,
        add_generation_prompt: true,
        use_jinja: true,
        parallel_tool_calls: false,
        // A thinking block would eat the answer budget and delay the first token.
        enable_thinking: false,
        add_bos: false,
        add_eos: false,
        parse_tool_calls: false,
    };
    model.apply_chat_template_oaicompat(tmpl, &params).map(|r| r.prompt).map_err(|e| format!("failed to apply chat template: {e}"))
}
