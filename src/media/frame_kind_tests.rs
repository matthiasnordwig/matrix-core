//! Tests of `frame_kind.rs`: slides on plain colour, text inside a found outline, dense text, and scenes
//! with or without a little text.

use super::*;
use crate::media::test_pictures::{scene, slide};

fn facts(chars: usize, share: f32, in_outline: bool, flat: f32) -> StillFacts {
    StillFacts { text_chars: chars, text_share: share, in_outline, colours: Colours { flat_share: flat } }
}

#[test]
fn slides_have_plain_colours_and_scenes_do_not() {
    let s = slide(320, 180, &[0.8, 0.6, 0.7], 1);
    assert!(Colours::of(&s.picture()).flat_share > 0.9);
    let n = scene(320, 180, 4, 0);
    assert!(Colours::of(&n.picture()).flat_share < MIN_FLAT_SHARE / 2.0);
}

#[test]
fn text_on_plain_colour_is_a_slide() {
    assert_eq!(classify(&facts(120, 0.08, false, 0.8)), FrameKind::Slide);
    // The same text over a busy picture: a poster in a room.
    assert_eq!(classify(&facts(120, 0.08, false, 0.2)), FrameKind::Scene);
    // A logo on a lectern before a light wall: plain, but too little text.
    assert_eq!(classify(&facts(40, 0.03, false, 0.8)), FrameKind::Scene);
}

#[test]
fn text_inside_an_outline_is_a_slide_whatever_is_around() {
    assert_eq!(classify(&facts(80, 0.05, true, 0.1)), FrameKind::Slide);
    assert_eq!(classify(&facts(80, 0.01, true, 0.1)), FrameKind::Scene);
}

#[test]
fn dense_text_is_a_slide_and_too_little_text_never() {
    assert_eq!(classify(&facts(900, 0.3, false, 0.1)), FrameKind::Slide);
    assert_eq!(classify(&facts(MIN_CHARS - 1, 0.5, true, 1.0)), FrameKind::Scene);
}

#[test]
fn readable_chars_count_letters_and_digits_of_any_script() {
    assert_eq!(readable_chars("Ab 12 — ü!"), 5);
    assert_eq!(readable_chars("中文 テキスト"), 6);
}
