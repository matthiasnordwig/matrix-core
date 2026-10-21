//! Tests for the llama.cpp-free parts of `inference/` (`vlm_prompt`, `chat_text`, `prefix_reuse`).
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

// --- prefix_reuse -------------------------------------------------------------

use super::prefix_reuse::{
    best_snapshot, checkpoints_for, plan, retain, shared_len, snapshot_at, snapshots_to_drop, Reuse, CHECKPOINT_BACK,
    CHECKPOINT_EVERY, MAX_CHECKPOINTS, MAX_SNAPSHOTS, SNAPSHOT_BYTES, SNAPSHOT_MIN_GAIN,
};

#[test]
fn attention_memory_is_cut_at_the_shared_prefix() {
    // Last call: prompt 1..=6 + answer 7, 8. Next round re-renders the answer differently.
    let cached = [1, 2, 3, 4, 5, 6, 7, 8];
    assert_eq!(plan(&cached, &[1, 2, 3, 4, 5, 6, 9, 10], false, &[]), Reuse::Truncate { keep: 6 });
    assert_eq!(plan(&cached, &[9, 1, 2], false, &[]), Reuse::Fresh);
    assert_eq!(plan::<i32>(&[], &[1, 2], false, &[]), Reuse::Fresh);
}

#[test]
fn one_prompt_token_is_always_read_again() {
    assert_eq!(plan(&[1, 2, 3], &[1, 2, 3], false, &[]), Reuse::Truncate { keep: 2 });
    assert_eq!(plan(&[1], &[1], false, &[]), Reuse::Fresh);
}

#[test]
fn partial_memory_continues_or_restores_the_latest_shared_checkpoint() {
    let cached = [1, 2, 3, 4, 5, 6, 7, 8];
    // The new prompt continues everything in memory: nothing to cut back.
    assert_eq!(plan(&cached, &[1, 2, 3, 4, 5, 6, 7, 8, 9], true, &[]), Reuse::Truncate { keep: 8 });
    // Diverges at 6: the latest state saved inside the shared part wins.
    assert_eq!(plan(&cached, &[1, 2, 3, 4, 5, 6, 0], true, &[2, 4, 7]), Reuse::Restore { keep: 4 });
    // Saved only after the divergence, or never: start over.
    assert_eq!(plan(&cached, &[1, 2, 3, 0], true, &[4]), Reuse::Fresh);
    assert_eq!(plan(&cached, &[1, 2, 3, 4, 5, 6, 0], true, &[]), Reuse::Fresh);
}

#[test]
fn checkpoints_every_few_thousand_tokens_and_just_before_the_end() {
    let e = CHECKPOINT_EVERY;
    assert_eq!(checkpoints_for(0, 10_000, None), vec![e, 2 * e, 10_000 - CHECKPOINT_BACK]);
    // Only beyond what was kept; nothing for a prompt too short to matter.
    assert_eq!(checkpoints_for(e, 10_000, None), vec![2 * e, 10_000 - CHECKPOINT_BACK]);
    assert_eq!(checkpoints_for(9_950, 10_000, None), Vec::<usize>::new());
    assert_eq!(checkpoints_for(0, CHECKPOINT_BACK, None), Vec::<usize>::new());
}

#[test]
fn a_chat_turn_also_saves_where_its_question_begins() {
    let e = CHECKPOINT_EVERY;
    // System prompt + earlier turns end at 700; this turn's sources and question follow.
    assert_eq!(checkpoints_for(0, 5_000, Some(700)), vec![700, e, 5_000 - CHECKPOINT_BACK]);
    // Already kept, or no earlier message: nothing extra; also for a prompt too short otherwise.
    assert_eq!(checkpoints_for(700, 5_000, Some(700)), vec![e, 5_000 - CHECKPOINT_BACK]);
    assert_eq!(checkpoints_for(0, 100, Some(40)), vec![40]);
    assert_eq!(checkpoints_for(0, 100, Some(100)), Vec::<usize>::new());
}

#[test]
fn retained_checkpoints_keep_the_earliest_and_the_latest() {
    let kept = retain(vec![9, 1, 5, 3, 7, 11], None);
    assert_eq!(kept.len(), MAX_CHECKPOINTS);
    assert_eq!(kept.first(), Some(&1));
    assert_eq!(kept.last(), Some(&11));
    // The turn start survives even where an unpinned one would go first.
    let kept = retain(vec![9, 1, 5, 3, 7, 11], Some(3));
    assert_eq!(kept.len(), MAX_CHECKPOINTS);
    assert!(kept.contains(&3) && kept.contains(&1) && kept.contains(&11));
}

#[test]
fn a_snapshot_is_saved_where_the_prompt_stops_sharing_the_last_call() {
    let g = SNAPSHOT_MIN_GAIN;
    // Grid row 2 after row 1: nothing kept (recurrent model), the instructions are shared.
    assert_eq!(snapshot_at(2 * g, 0, 4 * g), Some(2 * g));
    // Row 3 restored the snapshot and shares only a few tokens more: no second one.
    assert_eq!(snapshot_at(2 * g + 10, 2 * g, 4 * g), None);
    // Too little shared, or the whole prompt (a repeated call): nothing to save.
    assert_eq!(snapshot_at(g - 1, 0, 4 * g), None);
    assert_eq!(snapshot_at(4 * g, 0, 4 * g), None);
    assert_eq!(shared_len(&[1, 2, 3, 4], &[1, 2, 9]), 2);
}

#[test]
fn the_longest_snapshot_the_prompt_starts_with_is_restored() {
    let grid: &[i32] = &[1, 2, 3];
    let grid_longer: &[i32] = &[1, 2, 3, 4, 5];
    let chat: &[i32] = &[7, 8];
    let snaps = [grid, chat, grid_longer];
    assert_eq!(best_snapshot(&snaps, &[1, 2, 3, 4, 5, 6], 0), Some(2));
    assert_eq!(best_snapshot(&snaps, &[1, 2, 3, 9], 0), Some(0));
    assert_eq!(best_snapshot(&snaps, &[7, 8, 1], 0), Some(1));
    // Not longer than what the context keeps anyway, or nothing left to read: no restore.
    assert_eq!(best_snapshot(&snaps, &[1, 2, 3, 9], 3), None);
    assert_eq!(best_snapshot(&snaps, &[7, 8], 0), None);
    assert_eq!(best_snapshot(&snaps, &[9], 0), None);
}

#[test]
fn snapshots_are_bounded_by_count_and_bytes_oldest_first() {
    assert_eq!(snapshots_to_drop(&[]), 0);
    assert_eq!(snapshots_to_drop(&[1; MAX_SNAPSHOTS]), 0);
    assert_eq!(snapshots_to_drop(&[1; MAX_SNAPSHOTS + 2]), 2);
    let half = SNAPSHOT_BYTES / 2;
    assert_eq!(snapshots_to_drop(&[half, half, 1]), 1, "the oldest goes first");
    assert_eq!(snapshots_to_drop(&[1, SNAPSHOT_BYTES + 1]), 2, "one that alone is too big is not kept");
}
