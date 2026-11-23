//! Tests of `scene_labels.rs` with made-up vectors.

use super::*;

/// One-hot-ish vector of dimension `n` pointing at `i`, with `rest` on every other axis.
fn axis(n: usize, i: usize, rest: f32) -> Vec<f32> {
    (0..n).map(|j| if j == i { 1.0 } else { rest }).collect()
}

/// Label vectors where every text has its own axis (dimension = number of texts).
fn vectors() -> (LabelVectors, usize) {
    let n = prompts().len();
    (LabelVectors::new((0..n).map(|i| axis(n, i, 0.0)).collect()).unwrap(), n)
}

fn idx(key: &str) -> usize {
    keys().position(|k| k == key).unwrap()
}

#[test]
fn prompts_and_keys_line_up() {
    assert_eq!(prompts().len(), SETTING.len() + PLACES.len());
    assert_eq!(keys().count(), SETTING.len() + PLACES.len());
    assert!(is_key("water") && is_key("indoors") && !is_key("a photo"));
    assert!(LabelVectors::new(vec![vec![1.0]; 3]).is_none(), "wrong count");
}

#[test]
fn the_closer_setting_wins_and_strong_places_hold() {
    let (v, n) = vectors();
    // Outdoors, beach and water stand out; the plain captions are weak.
    let mut p = vec![0.05; n];
    p[idx("outdoors")] = 0.6;
    p[idx("beach")] = 0.7;
    p[idx("water")] = 0.65;
    assert_eq!(labels(&p, &v), vec!["outdoors", "beach", "water"]);
}

#[test]
fn a_place_must_stand_out_from_the_other_places() {
    let (v, n) = vectors();
    // Places at 0.10 and 0.20 by turns: none stands 1.5 standard deviations above the mean.
    let mut p = vec![0.0; n];
    p[idx("indoors")] = 0.3;
    for (i, l) in PLACES.iter().enumerate() {
        p[idx(l.key)] = if i % 2 == 0 { 0.10 } else { 0.20 };
    }
    assert_eq!(labels(&p, &v), vec!["indoors"]);
    // One clearly above the rest holds, whatever the absolute level (another model, a dark picture).
    for level in [0.02f32, 0.3] {
        let mut q = vec![level; n];
        q[idx("indoors")] = level + 0.1;
        q[idx("office")] = level * 2.0;
        assert_eq!(labels(&q, &v), vec!["indoors", "office"], "level {level}");
    }
    // All equal: no place, the tie goes to indoors.
    assert_eq!(labels(&vec![0.2; n], &v), vec!["indoors"]);
}

#[test]
fn vectors_of_another_model_give_nothing() {
    let (v, n) = vectors();
    assert!(labels(&vec![0.1; n + 1], &v).is_empty());
}
