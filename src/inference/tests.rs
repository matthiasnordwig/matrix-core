//! Tests for the llama.cpp-free parts of `inference/` (`vlm_prompt`).
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
