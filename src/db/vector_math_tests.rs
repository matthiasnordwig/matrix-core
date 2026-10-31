//! `dot` against the plain sequential sum: same value within rounding at every length around the
//! lane width, and the behaviour for different lengths stays "up to the shorter one".

use super::embeddings::{cosine, dot};

fn sequence(n: usize, seed: f32) -> Vec<f32> {
    (0..n).map(|i| ((i as f32 + seed) * 0.7).sin()).collect()
}

fn sequential(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[test]
fn dot_matches_the_sequential_sum_at_every_length() {
    for n in (0..=33).chain([767, 768, 769, 1536]) {
        let (a, b) = (sequence(n, 1.0), sequence(n, 2.5));
        let (fast, plain) = (dot(&a, &b), sequential(&a, &b));
        assert!((fast - plain).abs() <= 1e-3 * plain.abs().max(1.0), "length {n}: {fast} vs {plain}");
    }
}

#[test]
fn dot_stops_at_the_shorter_input() {
    let (a, b) = (sequence(20, 1.0), sequence(11, 2.0));
    assert_eq!(dot(&a, &b), dot(&a[..11], &b));
    assert_eq!(dot(&b, &a), dot(&b, &a[..11]));
    assert_eq!(dot(&[], &a), 0.0);
}

#[test]
fn identical_vectors_score_exactly_alike() {
    let v = sequence(768, 3.0);
    assert_eq!(cosine(&v, &v.clone()), cosine(&v, &v));
    assert!((cosine(&v, &v) - 1.0).abs() < 1e-5);
}
