//! Tests of `diarize_cluster.rs` on made-up voice vectors: separated groups of any number, one voice alone, the
//! bounded subset (10 000 sections) and the nearest centroid.

use super::*;

/// Deterministic pseudo-random numbers in [-1, 1).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

const DIM: usize = 192;

/// `per` vectors around each of `speakers` random directions; `noise` relative to the centre's length.
fn voices(speakers: usize, per: usize, noise: f32, seed: u64) -> (Vec<Vec<f32>>, Vec<usize>) {
    let mut rng = Lcg(seed);
    let centres: Vec<Vec<f32>> = (0..speakers).map(|_| (0..DIM).map(|_| rng.next()).collect()).collect();
    let (mut v, mut truth) = (Vec::new(), Vec::new());
    for i in 0..per {
        for (s, c) in centres.iter().enumerate() {
            let _ = i;
            v.push(c.iter().map(|x| x + noise * rng.next()).collect());
            truth.push(s);
        }
    }
    (v, truth)
}

/// Whether two labelings split the items the same way.
fn same_partition(a: &[usize], b: &[usize]) -> bool {
    (0..a.len()).all(|i| (0..a.len()).all(|j| (a[i] == a[j]) == (b[i] == b[j])))
}

#[test]
fn separated_voices_are_found_whatever_their_number() {
    for speakers in [2, 3, 4, 6] {
        let (v, truth) = voices(speakers, 30, 0.8, speakers as u64);
        let labels = cluster(&v, 2);
        assert!(same_partition(&labels, &truth), "{speakers} speakers: {:?}", labels.iter().max());
    }
}

#[test]
fn one_voice_alone_stays_one_speaker() {
    let (v, _) = voices(1, 80, 0.8, 7);
    assert!(cluster(&v, 2).iter().all(|&l| l == 0));
}

#[test]
fn labels_count_from_zero_in_order_of_appearance() {
    let (v, _) = voices(3, 10, 0.5, 3);
    let labels = cluster(&v, 2);
    assert_eq!(&labels[..3], &[0, 1, 2]);
    assert!(cluster(&[], 2).is_empty());
    assert_eq!(cluster(&v[..1], 2), vec![0]);
}

#[test]
fn every_vector_goes_to_the_nearest_centroid_of_its_voice() {
    let (v, truth) = voices(4, 25, 0.8, 11);
    let labels = cluster(&v, 2);
    let c = centroids(&v, &labels);
    assert_eq!(c.len(), 4);
    let (more, more_truth) = voices(4, 5, 0.8, 11);
    let _ = more_truth;
    // Same centres (same seed), new noise: each new vector lands where its voice's training vectors went.
    for (x, t) in more.iter().zip(truth.iter()) {
        let expect = labels[truth.iter().position(|y| y == t).unwrap()];
        assert_eq!(nearest(&c, x), expect);
    }
}

#[test]
fn the_subset_stays_within_capacity_and_spread_over_time() {
    let mut s = Subset::new(1000);
    for i in 0..10_000u32 {
        s.offer(&[i as f32]);
    }
    assert!(s.len() <= 1000 && s.len() >= 500, "{}", s.len());
    let first = s.items()[0][0];
    let last = s.items()[s.len() - 1][0];
    assert_eq!(first, 0.0);
    assert!(last > 9_000.0, "the end of the file is represented: {last}");
    // Evenly spaced: every gap the same.
    let gaps: Vec<f32> = s.items().windows(2).map(|w| w[1][0] - w[0][0]).collect();
    assert!(gaps.iter().all(|g| *g == gaps[0]));
}

#[test]
fn ten_thousand_sections_cluster_within_the_memory_budget() {
    // A budget that allows ~1300 vectors: the matrices of 10 000 would need 400 MB.
    let budget = 8 << 20;
    let cap = Subset::capacity_for(budget, DIM);
    let matrix_and_vectors = |n: usize| (n * n * 4 + n * (DIM * 4 + 16 + MAX_SPEAKERS * 8)) as u64;
    assert!(matrix_and_vectors(cap) <= budget && matrix_and_vectors(cap + 50) > budget, "{cap}");
    let (all, truth) = voices(5, 2_000, 0.8, 5);
    let mut s = Subset::new(cap);
    all.iter().for_each(|v| s.offer(v));
    assert!(s.len() <= cap);
    let labels = cluster(s.items(), s.min_group(2));
    let c = centroids(s.items(), &labels);
    assert_eq!(c.len(), 5);
    // All 10 000 vectors (also those not clustered) land with their voice.
    let mut seen = std::collections::HashMap::new();
    for (v, t) in all.iter().zip(&truth) {
        let got = nearest(&c, v);
        assert_eq!(*seen.entry(*t).or_insert(got), got);
    }
}

#[test]
fn groups_too_small_for_a_voice_join_the_nearest_one() {
    // Two voices of 40 vectors each, plus 8 vectors of a third direction: with a minimum of 10 per voice the
    // eight are no speaker of their own.
    let (mut v, _) = voices(2, 40, 0.8, 21);
    let (odd, _) = voices(1, 8, 0.8, 99);
    v.extend(odd);
    let labels = cluster(&v, 10);
    assert_eq!(labels.iter().max(), Some(&1));
    let free = cluster(&v, 2);
    assert_eq!(free.iter().max(), Some(&2), "without the minimum they are a group");
}

#[test]
fn the_minimum_group_shrinks_with_the_subset_stride() {
    let mut s = Subset::new(4);
    assert_eq!(s.min_group(10), 10);
    for i in 0..9 {
        s.offer(&[i as f32]);
    }
    // 9 offered into 4 places: stride 4 now.
    assert_eq!(s.min_group(10), 2);
}

#[test]
fn capacity_grows_with_the_budget_but_not_linearly() {
    let small = Subset::capacity_for(16 << 20, DIM);
    let large = Subset::capacity_for(64 << 20, DIM);
    assert!(large > small && large < 3 * small, "{small} {large}");
    assert_eq!(Subset::capacity_for(0, DIM), 2);
}
