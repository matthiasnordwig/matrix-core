//! Tests of `scenes.rs` on made-up stills: what counts as a change, cuts between shots, a panning camera
//! without cuts, a single transition still, which still stands for a shot, and pages held into a shaking
//! phone with a pan between.

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

/// `b` moved `dx` pixels to the left, the last column repeated at the right edge: a hand shaking the camera.
fn shaken(b: &RgbBuf, dx: u32) -> RgbBuf {
    let mut out = b.clone();
    for y in 0..b.height {
        for x in 0..b.width {
            let (src, dst) = ((y * b.width + (x + dx).min(b.width - 1)) as usize * 3, (y * b.width + x) as usize * 3);
            out.rgb[dst..dst + 3].copy_from_slice(&b.rgb[src..src + 3]);
        }
    }
    out
}

/// Held `page` for `n` stills with a shaking hand.
fn held(page: &RgbBuf, n: usize) -> Vec<RgbBuf> {
    [0, 7, 2, 9, 4].iter().cycle().take(n).map(|dx| shaken(page, *dx)).collect()
}

fn split(stills: &[RgbBuf]) -> Vec<Shot<usize>> {
    let mut s = ShotSplitter::new();
    let mut out = Vec::new();
    for (i, b) in stills.iter().enumerate() {
        out.extend(s.push(i as u64 * 1000, sig(b), i));
    }
    out.extend(s.finish(stills.len() as u64 * 1000));
    out
}

#[test]
fn a_shaking_camera_on_one_page_differs_like_a_pan_but_keeps_its_colours() {
    let page = slide(W, H, &[0.8, 0.6, 0.7, 0.5], 1);
    let (a, b) = (sig(&page), sig(&shaken(&page, 7)));
    assert!(distance(&a, &b) >= MIN_SCORE, "the grid sees the shake: {}", distance(&a, &b));
    assert!(colours(&a, &b) < HOLD_COLOURS, "{}", colours(&a, &b));
}

/// User finding: page, pan, page — shorter than a piece of a moving shot (`PIECE_MS`), so only the held
/// views part them. Two white pages look alike in colour: a further piece, told apart by their text later.
#[test]
fn two_held_pages_with_a_pan_between_are_two_shots() {
    let (a, b) = (slide(W, H, &[0.8, 0.6, 0.7, 0.5], 1), slide(W, H, &[0.4, 0.9, 0.3, 0.8, 0.6], 2));
    let mut stills = held(&a, 4);
    stills.extend([scene(W, H, 11, 0), scene(W, H, 11, 90)]);
    stills.extend(held(&b, 3));
    let got = split(&stills);
    let spans: Vec<(u64, u64, usize, bool)> = got.iter().map(|s| (s.t_start_ms, s.t_end_ms, s.still, s.continues)).collect();
    assert_eq!(got.len(), 2, "{spans:?}");
    assert_eq!((got[1].t_start_ms, got[1].t_end_ms), (6000, 9000), "{spans:?}");
    assert!(got[0].still < 4, "the page, not the pan away from it: {spans:?}");
    assert!(got[1].still >= 6, "{spans:?}");
    assert!(!got[1].continues, "a new held view is a shot of its own, never a piece for its colours: {spans:?}");
}

#[test]
fn a_held_view_in_other_colours_is_a_new_shot() {
    // A leaflet (many colours) held, the same pan, then a white letter: a new shot, not a further piece.
    let mut stills: Vec<RgbBuf> = [0, 7, 2, 9].iter().map(|dx| scene(W, H, 4, *dx)).collect();
    stills.extend([scene(W, H, 11, 0), scene(W, H, 11, 90)]);
    stills.extend(held(&slide(W, H, &[0.8, 0.6, 0.7, 0.5], 1), 3));
    let got = split(&stills);
    let spans: Vec<(u64, u64, usize, bool)> = got.iter().map(|s| (s.t_start_ms, s.t_end_ms, s.still, s.continues)).collect();
    assert_eq!(spans, vec![(0, 6000, spans[0].2, false), (6000, 9000, spans[1].2, false)]);
    assert!(spans[0].2 < 4 && spans[1].2 >= 6, "{spans:?}");
}

#[test]
fn a_short_pause_on_the_desk_does_not_stand_for_the_page_after_it() {
    // Letter, a plain desk for two stills (standing: nothing moves on it), the next letter.
    let desk = plain(W, H, [120, 78, 45]);
    let mut stills = held(&slide(W, H, &[0.8, 0.6, 0.7, 0.5], 1), 3);
    stills.extend([desk.clone(), desk]);
    stills.extend(held(&slide(W, H, &[0.4, 0.9, 0.3, 0.8, 0.6], 2), 3));
    let chosen: Vec<usize> = split(&stills).iter().map(|s| s.still).collect();
    assert!(chosen.first().is_some_and(|c| *c < 3) && chosen.last().is_some_and(|c| *c >= 5), "{chosen:?}");
    assert!(!chosen.contains(&3) && !chosen.contains(&4), "{chosen:?}");
}

#[test]
fn a_page_held_on_and_on_stays_one_shot() {
    let page = slide(W, H, &[0.8, 0.6, 0.7, 0.5], 1);
    let got = split(&held(&page, 8));
    assert_eq!(got.iter().map(|s| (s.t_start_ms, s.t_end_ms)).collect::<Vec<_>>(), vec![(0, 8000)]);
}

#[test]
fn the_same_view_after_a_shake_is_the_same_two_white_pages_are_not() {
    let view = |b: &RgbBuf| View::of(&b.picture());
    let (a, b) = (slide(W, H, &[0.8, 0.6, 0.7, 0.5], 1), slide(W, H, &[0.4, 0.9, 0.3, 0.8, 0.6], 2));
    let leaflet = scene(W, H, 4, 0);
    // Moved by whole cells of the view (320 px / 96 cells): nothing else changed.
    assert!(same_view(&view(&a), &view(&shaken(&a, 10))), "{}", view_difference(&view(&a), &view(&shaken(&a, 10))));
    assert!(same_view(&view(&leaflet), &view(&shaken(&leaflet, 20))));
    // Alike in colour, but another page: never the same view.
    assert!(colours(&sig(&a), &sig(&b)) < HOLD_COLOURS);
    assert!(!same_view(&view(&a), &view(&shaken(&b, 4))), "{}", view_difference(&view(&a), &view(&shaken(&b, 4))));
    assert!(!same_view(&view(&leaflet), &view(&a)));
}
