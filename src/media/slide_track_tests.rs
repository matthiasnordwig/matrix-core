//! Tests of `slide_track.rs`: the same slide by its words or (with little text) its picture, a slide built
//! up point by point, slides interrupted by a scene, and scenes never joined.

use super::*;
use crate::media::test_pictures::{scene, slide};
use crate::media::RgbBuf;

fn key(b: &RgbBuf, text: &str) -> SlideKey {
    SlideKey::new(&b.picture(), text)
}

fn shot(t: u64, kind: FrameKind, b: &RgbBuf, text: &str, id: u32) -> ShotStill<u32> {
    let key = (kind == FrameKind::Slide).then(|| key(b, text));
    ShotStill { t_start_ms: t * 1000, t_end_ms: (t + 1) * 1000, kind, key, text: text.into(), payload: id, continues: false, look: None }
}

fn run(shots: Vec<ShotStill<u32>>) -> Vec<(u64, u64, FrameKind, u32, usize)> {
    let mut t = Tracker::new();
    let mut out: Vec<Unit<u32>> = shots.into_iter().flat_map(|s| t.push(s)).collect();
    out.extend(t.finish());
    out.into_iter().map(|u| (u.t_start_ms, u.t_end_ms, u.kind, u.payload, u.shots)).collect()
}

#[test]
fn same_words_are_the_same_slide_even_when_read_slightly_differently() {
    let p = slide(320, 180, &[0.7, 0.5], 1);
    let a = key(&p, "Results of the survey\nMost readers use the app daily");
    let b = key(&p, "Results of the surey\nMost readers use the app daily");
    assert!(same_slide(&a, &b));
    let other = key(&p, "Methods\nWe asked 200 readers in three cities");
    assert!(!same_slide(&a, &other));
}

#[test]
fn a_shared_header_and_footer_do_not_make_two_slides_one() {
    let p = slide(320, 180, &[0.7, 0.5], 1);
    let a = key(&p, "Wikilokal | Story\nInspiration from location based discovery\nConference 2026");
    let b = key(&p, "Wikilokal | Advantages\nWorks on web and Android in many languages\nConference 2026");
    assert!(!same_slide(&a, &b));
}

#[test]
fn with_little_text_the_picture_decides() {
    let p = slide(320, 180, &[0.7, 0.5, 0.8], 1);
    let q = slide(320, 180, &[0.2, 0.9], 5);
    assert!(same_slide(&key(&p, "Thank"), &key(&p, "")));
    assert!(!same_slide(&key(&p, ""), &key(&q, "")));
}

#[test]
fn a_slide_built_up_point_by_point_is_one_unit_with_the_full_text() {
    let p = slide(320, 180, &[0.7], 1);
    let texts = ["Advantages\nIntuitive interface", "Advantages\nIntuitive interface\nMultilingual", "Advantages\nIntuitive interface\nMultilingual\nFree access without login"];
    let shots = texts.iter().enumerate().map(|(i, t)| shot(i as u64, FrameKind::Slide, &p, t, i as u32)).collect();
    assert_eq!(run(shots), vec![(0, 3000, FrameKind::Slide, 2, 3)]);
}

#[test]
fn a_scene_ends_the_slide_and_only_pieces_of_one_moving_shot_join() {
    let p = slide(320, 180, &[0.7], 1);
    let s = scene(320, 180, 2, 0);
    let a = "Story of Wikilokal\nContent within a radius of the user";
    let shots = vec![
        shot(0, FrameKind::Slide, &p, a, 0),
        shot(1, FrameKind::Slide, &p, a, 1),
        shot(2, FrameKind::Scene, &s, "", 2),
        shot(3, FrameKind::Scene, &s, "", 3),
        ShotStill { continues: true, ..shot(4, FrameKind::Scene, &s, "", 4) },
        shot(5, FrameKind::Slide, &p, a, 5),
    ];
    assert_eq!(
        run(shots),
        vec![
            (0, 2000, FrameKind::Slide, 1, 2),
            (2000, 3000, FrameKind::Scene, 2, 1),
            (3000, 5000, FrameKind::Scene, 3, 2),
            (5000, 6000, FrameKind::Slide, 5, 1),
        ]
    );
}

#[test]
fn a_poorly_read_still_does_not_chain_two_slides_together() {
    let p = slide(320, 180, &[0.7, 0.5], 1);
    let a = "Types of lists\nCrowd sourced from missing articles";
    let b = "Considerations\nKeep lists short and sourced";
    // The middle still shows the same screen but was read with one word only.
    let shots = vec![
        shot(0, FrameKind::Slide, &p, a, 0),
        shot(1, FrameKind::Slide, &p, "Types", 1),
        shot(2, FrameKind::Slide, &p, b, 2),
    ];
    let got = run(shots);
    assert_eq!(got.iter().map(|u| (u.0, u.1)).collect::<Vec<_>>(), vec![(0, 2000), (2000, 3000)]);
}

fn seen(t: u64, b: &RgbBuf, id: u32) -> ShotStill<u32> {
    ShotStill { look: Some(crate::media::scenes::View::of(&b.picture())), ..shot(t, FrameKind::Scene, b, "", id) }
}

/// Without text recognition every page is a scene: two pages alike in colour stay two units.
#[test]
fn two_scenes_alike_in_colour_but_not_the_same_view_stay_two() {
    let (a, b) = (slide(320, 180, &[0.8, 0.6, 0.7, 0.5], 1), slide(320, 180, &[0.4, 0.9, 0.3, 0.8, 0.6], 2));
    let got = run(vec![seen(0, &a, 0), seen(1, &b, 1)]);
    assert_eq!(got.len(), 2, "{got:?}");
}

#[test]
fn a_scene_held_again_after_a_shake_is_one_unit() {
    // Moved by three cells of the view (320 px / 96 cells).
    let got = run(vec![seen(0, &scene(320, 180, 4, 0), 0), seen(1, &scene(320, 180, 4, 10), 1)]);
    assert_eq!(got.iter().map(|u| (u.0, u.1, u.4)).collect::<Vec<_>>(), vec![(0, 2000, 2)]);
}
