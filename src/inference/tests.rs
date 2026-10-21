//! Tests for the llama.cpp-free parts of `inference/` (`vlm_prompt`, `chat_text`).
use super::vlm_prompt::{context_for, fits_context, user_turn_with_image, DEFAULT_MEDIA_MARKER};

#[test]
fn picture_marker_comes_before_the_instruction() {
    let t = user_turn_with_image("  Describe this picture.  ", DEFAULT_MEDIA_MARKER);
    assert_eq!(t, "<__media__>\nDescribe this picture.");
}

#[test]
fn a_marker_inside_the_instruction_is_removed() {
    // A second marker would make mtmd expect a second picture.
    let t = user_turn_with_image("What is <__media__> showing?", DEFAULT_MEDIA_MARKER);
    assert_eq!(t.matches(DEFAULT_MEDIA_MARKER).count(), 1);
    assert!(t.ends_with("What is  showing?"));
}

#[test]
fn context_fit_counts_prompt_and_answer() {
    assert!(fits_context(3000, 1024, 4096));
    assert!(fits_context(3072, 1024, 4096));
    assert!(!fits_context(3073, 1024, 4096));
    assert!(!fits_context(usize::MAX, u32::MAX, u32::MAX));
}

#[test]
fn context_is_sized_to_the_call_not_the_endpoint() {
    assert_eq!(context_for(300, 512, 8192), 1024, "812 rounded up to 1024");
    assert_eq!(context_for(256, 256, 8192), 512, "exact multiple stays");
    assert_eq!(context_for(0, 0, 8192), 256, "never zero");
    assert_eq!(context_for(7000, 2048, 8192), 8192, "capped at the endpoint's context");
}

// ── chat_text: template route + Harmony answer ──────────────────────────────
use super::chat_text::{harmony_final, route_for, TemplateRoute, HARMONY_KWARGS, HARMONY_TRUNCATED};

#[test]
fn plain_chatml_template_stays_on_the_builtin_list() {
    // Qwen3 30B-A3B Instruct 2507: ChatML, no thinking switch.
    let t = "{%- for message in messages %}{{- '<|im_start|>' + message.role + '\\n' }}{%- endfor %}";
    assert_eq!(route_for(t), TemplateRoute::Builtin);
}

#[test]
fn a_thinking_switch_goes_through_jinja_without_kwargs() {
    // Qwen3.5/3.6, Gemma 4: thinking is turned off through `enable_thinking`.
    let t = "{%- if enable_thinking is defined and enable_thinking is false %}<think>\n\n</think>{%- endif %}";
    assert_eq!(route_for(t), TemplateRoute::Jinja { kwargs: None });
}

#[test]
fn harmony_template_goes_through_jinja_with_low_reasoning() {
    let t = "{{- '<|start|>assistant<|channel|>final<|message|>' }}{%- if enable_thinking %}{% endif %}";
    assert_eq!(route_for(t), TemplateRoute::Jinja { kwargs: Some(HARMONY_KWARGS) });
    assert!(serde_json::from_str::<serde_json::Value>(HARMONY_KWARGS).unwrap().is_object());
}

#[test]
fn text_without_harmony_markers_is_unchanged() {
    assert_eq!(harmony_final("  Plain answer [1].\n").unwrap(), "  Plain answer [1].\n");
    assert_eq!(harmony_final("<think>x</think>answer").unwrap(), "<think>x</think>answer");
}

#[test]
fn harmony_reply_is_cut_to_the_final_channel() {
    let raw = "<|channel|>analysis<|message|>The user asks about X. Source [2] says…<|end|>\
               <|start|>assistant<|channel|>final<|message|>X is 42 [2].";
    assert_eq!(harmony_final(raw).unwrap(), "X is 42 [2].");
}

#[test]
fn harmony_final_stops_at_the_end_marker_and_takes_the_last_final() {
    let raw = "<|channel|>final<|message|>draft<|end|><|start|>assistant<|channel|>analysis<|message|>hm<|end|>\
               <|start|>assistant<|channel|>final <|constrain|>text<|message|> Answer. <|return|>";
    assert_eq!(harmony_final(raw).unwrap(), "Answer.");
}

#[test]
fn harmony_reply_cut_off_while_reasoning_is_an_error() {
    let raw = "<|channel|>analysis<|message|>Still thinking about the sources and";
    assert_eq!(harmony_final(raw).unwrap_err(), HARMONY_TRUNCATED);
    // A commentary (tool) channel is no answer either.
    let raw = "<|channel|>commentary to=functions.search<|message|>{\"q\":1}<|call|>";
    assert!(harmony_final(raw).is_err());
}
