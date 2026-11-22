//! Tests of `scenes.rs` on made-up stills: what counts as a change, cuts between shots, a panning camera
//! without cuts, a single transition still, and which still stands for a shot.

use super::*;
use crate::media::test_pictures::{plain, scene, slide};
use crate::media::RgbBuf;

const W: u32 = 320;
const H: u32 = 180;

fn sig(b: &RgbBuf) -> Signature {
    Signature::of(&b.picture())
}

/// Run stills (one per second) through the splitter; shots as (start, end, chosen index).
fn shots(stills: &[RgbBuf]) -> Vec<(u64, u64, usize, bool)> {
    let mut s = ShotSplitter::new();
    let mut out = Vec::new();
    for (i, b) in stills.iter().enumerate() {
        out.extend(s.push(i as u64 * 1000, sig(b), i));
    }
    out.extend(s.finish(stills.len() as u64 * 1000));
    out.into_iter().map(|s| (s.t_start_ms, s.t_end_ms, s.still, s.steady)).collect()
}

#[test]
fn the_same_still_has_no_distance_and_another_slide_counts() {
    let a = slide(W, H, &[0.8, 0.6, 0.7, 0.5], 1);
    assert_eq!(distance(&sig(&a), &sig(&a)), 0.0);
    // White slide after white slide: nearly the same colours, but the lines moved.
    let b = slide(W, H, &[0.4, 0.9, 0.3, 0.8, 0.6], 2);
    assert!(distance(&sig(&a), &sig(&b)) >= MIN_SCORE, "{}", distance(&sig(&a), &sig(&b)));
    // A camera cut changes the colours.
    assert!(distance(&sig(&a), &sig(&scene(W, H, 3, 0))) > 0.5);
}

#[test]
fn exposure_shift_of_the_whole_picture_is_not_a_content_change() {
    let a = plain(W, H, [100, 100, 100]);
    let b = plain(W, H, [108, 108, 108]);
    let (sa, sb) = (sig(&a), sig(&b));
    // Same histogram bin, every cell shifted alike.
    assert!(distance(&sa, &sb) < MIN_SCORE);
}

#[test]
fn standing_slides_split_at_each_change() {
    let a = slide(W, H, &[0.8, 0.6, 0.7], 1);
    let b = slide(W, H, &[0.3, 0.9, 0.5, 0.7, 0.4], 2);
    let c = scene(W, H, 5, 0);
    let stills: Vec<RgbBuf> = [&a, &a, &a, &a, &b, &b, &b, &c, &c, &c, &c].iter().map(|b| (*b).clone()).collect();
    let got = shots(&stills);
    assert_eq!(got.iter().map(|s| (s.0, s.1)).collect::<Vec<_>>(), vec![(0, 4000), (4000, 7000), (7000, 11_000)]);
    // The latest standing still of each shot.
    assert_eq!(got.iter().map(|s| s.2).collect::<Vec<_>>(), vec![3, 6, 10]);
    assert!(got.iter().all(|s| s.3));
}

#[test]
fn a_panning_camera_is_one_shot_until_a_real_cut() {
    let mut stills: Vec<RgbBuf> = (0..8).map(|i| scene(W, H, 7, i * 4)).collect();
    stills.extend((0..4).map(|_| slide(W, H, &[0.8, 0.5], 3)));
    let got = shots(&stills);
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!((got[0].0, got[0].1), (0, 8000));
    assert!(!got[0].3, "a moving shot has no standing still");
    assert!(got[1].3);
}

#[test]
fn a_long_moving_shot_is_read_in_pieces_a_standing_one_is_not() {
    let moving: Vec<RgbBuf> = (0..25).map(|i| scene(W, H, 7, i * 4)).collect();
    let mut s = ShotSplitter::new();
    let mut got = Vec::new();
    for (i, b) in moving.iter().enumerate() {
        got.extend(s.push(i as u64 * 1000, sig(b), i));
    }
    got.extend(s.finish(25_000));
    let pieces: Vec<(u64, u64, bool)> = got.iter().map(|s| (s.t_start_ms, s.t_end_ms, s.continues)).collect();
    assert_eq!(pieces, vec![(0, 10_000, false), (10_000, 20_000, true), (20_000, 25_000, true)]);
    let standing: Vec<RgbBuf> = (0..25).map(|_| slide(W, H, &[0.8, 0.5], 3)).collect();
    assert_eq!(shots(&standing).len(), 1);
}

#[test]
fn a_single_transition_still_joins_the_next_shot() {
    let a = slide(W, H, &[0.8, 0.6, 0.7], 1);
    let b = scene(W, H, 9, 0);
    let mut mid = a.clone();
    // Half of the picture already shows the next shot (a wipe).
    for y in 0..H {
        let row = (y * W * 3) as usize;
        mid.rgb[row..row + (W / 2 * 3) as usize].copy_from_slice(&b.rgb[row..row + (W / 2 * 3) as usize]);
    }
    let stills = vec![a.clone(), a.clone(), a.clone(), mid, b.clone(), b.clone(), b.clone()];
    let got = shots(&stills);
    assert_eq!(got.iter().map(|s| (s.0, s.1)).collect::<Vec<_>>(), vec![(0, 3000), (3000, 7000)], "{got:?}");
    assert_ne!(got[1].2, 3, "the transition still never stands for a shot");
}

#[test]
fn a_slide_built_up_line_by_line_keeps_its_fullest_still() {
    let lines = [0.8, 0.6, 0.7, 0.5];
    // Each step adds a short line: small against the slide, but grows; standing stills between.
    let steps: Vec<RgbBuf> = (2..=4).map(|n| slide(W, H, &lines[..n], 1)).collect();
    let stills = vec![steps[0].clone(), steps[0].clone(), steps[1].clone(), steps[1].clone(), steps[2].clone(), steps[2].clone()];
    let got = shots(&stills);
    let last = got.last().unwrap();
    assert_eq!(last.2, 5, "{got:?}");
    assert_eq!(last.1, 6000);
}

#[test]
fn one_still_alone_is_one_shot() {
    let got = shots(&[slide(W, H, &[0.5], 1)]);
    assert_eq!(got, vec![(0, 1000, 0, false)]);
}
