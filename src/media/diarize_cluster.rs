//! Agglomerative clustering of voice vectors into speakers, without a fixed number of speakers.
//!
//! - **What is clustered:** only vectors from enough clean speech (the caller filters, like pyannote's and sherpa-onnx
//!   `master`'s `min_active_ratio`); every other vector later goes to the nearest centroid ([`nearest`]).
//! - **Bounded memory:** [`Subset`] keeps at most `capacity` vectors spread over the file's time (every vector until
//!   full, then every second one is dropped and only every second further one taken, and so on), so the
//!   distance matrix of [`cluster`] does not grow with the length of the file. [`Subset::capacity_for`] turns a
//!   byte budget into that capacity.
//! - **Where to stop:** [`cluster`] merges by complete linkage over the cosine distance (nearest-neighbour chain,
//!   O(n²) time) and picks the number of speakers from the data: the cut of the dendrogram (2 to [`MAX_SPEAKERS`]
//!   groups) with the highest mean silhouette (how much closer each vector is to its own group than to the next one).
//!   Groups smaller than one moment's worth of vectors ([`Subset::min_group`]: the overlapping windows of one instant
//!   give near-copies, which would make any split by time look good) join the nearest larger group before scoring.
//!   When even the best cut has a silhouette below [`MIN_SILHOUETTE`] — Kaufman and Rousseeuw's line under which a
//!   grouping is weak or artificial — the file has one speaker. No fixed distance threshold: sherpa-onnx 2.0.2's
//!   found 10–70 speakers for 4, and the right threshold differed per model and meeting.
//!
//! Reference values (DER on the AMI meetings) and the rule's reasons: `handbuch/medien.md` „Sprecher“.

/// Vectors kept for clustering, spread evenly over the order they were offered in.
#[derive(Debug, Clone)]
pub struct Subset {
    capacity: usize,
    stride: u64,
    seen: u64,
    items: Vec<Vec<f32>>,
}

impl Subset {
    /// At most `capacity` vectors (at least 2).
    pub fn new(capacity: usize) -> Self {
        Subset { capacity: capacity.max(2), stride: 1, seen: 0, items: Vec::new() }
    }

    /// How many vectors of `dim` floats fit `bytes` together with [`cluster`]'s working memory.
    pub fn capacity_for(bytes: u64, dim: usize) -> usize {
        // n·dim·4 (vectors) + 2 · n²/2·4 (distances, and their copy the merging overwrites)
        // + n·(16 + MAX_SPEAKERS·8) (bookkeeping, sums per group) ≤ bytes
        let (a, b, c) = (4.0, (dim * 4 + 16 + MAX_SPEAKERS * 8) as f64, -(bytes as f64));
        ((-b + (b * b - 4.0 * a * c).sqrt()) / (2.0 * a)).floor().max(2.0) as usize
    }

    /// Offers the next vector in time order.
    pub fn offer(&mut self, v: &[f32]) {
        let take = self.seen.is_multiple_of(self.stride);
        self.seen += 1;
        if !take {
            return;
        }
        self.items.push(v.to_vec());
        if self.items.len() > self.capacity {
            // Keep every second one (the even positions of the time order) and take half as often from now on.
            let mut i = 0;
            self.items.retain(|_| {
                i += 1;
                i % 2 == 1
            });
            self.stride *= 2;
        }
    }

    /// Fewest vectors a speaker group of this subset needs: `per_point` vectors describe the same moment (the
    /// overlapping windows each instant lies in), thinned by the subset's stride — a group smaller than that is one
    /// stretch of time heard through overlapping windows, not a voice of its own.
    pub fn min_group(&self, per_point: usize) -> usize {
        (per_point as u64 / self.stride).max(2) as usize
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[Vec<f32>] {
        &self.items
    }
}

fn normalized(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter().map(|x| x / norm).collect()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Index into the condensed upper triangle of an n×n matrix (i < j).
fn tri(n: usize, i: usize, j: usize) -> usize {
    let (i, j) = if i < j { (i, j) } else { (j, i) };
    i * (2 * n - i - 1) / 2 + (j - i - 1)
}

/// Most speakers a file is split into.
pub const MAX_SPEAKERS: usize = 20;
/// Below this mean silhouette of the best cut, the vectors form no groups: one speaker (Kaufman & Rousseeuw,
/// "Finding Groups in Data", 1990: 0.51–0.70 reasonable structure, ≤ 0.50 weak or none).
pub const MIN_SILHOUETTE: f32 = 0.5;

/// One merge of the dendrogram: the two cluster ids (the smaller survives) and their distance.
#[derive(Debug, Clone, Copy)]
struct Merge {
    a: usize,
    b: usize,
    distance: f32,
}

/// Cosine distances of the normalised `vectors` (condensed upper triangle).
fn distances(vectors: &[Vec<f32>]) -> Vec<f32> {
    let n = vectors.len();
    let unit: Vec<Vec<f32>> = vectors.iter().map(|v| normalized(v)).collect();
    let mut d = vec![0f32; n * n.saturating_sub(1) / 2];
    for i in 0..n {
        for j in i + 1..n {
            d[tri(n, i, j)] = (1.0 - dot(&unit[i], &unit[j])).max(0.0);
        }
    }
    d
}

/// Complete-linkage merges over the distances `d` of `n` items (overwritten), in the order made.
fn merges(n: usize, mut d: Vec<f32>) -> Vec<Merge> {
    let mut active = vec![true; n];
    let mut out = Vec::with_capacity(n.saturating_sub(1));
    let mut chain: Vec<usize> = Vec::new();
    for _ in 1..n {
        if chain.is_empty() {
            chain.push((0..n).find(|&i| active[i]).unwrap_or(0));
        }
        loop {
            let a = *chain.last().unwrap_or(&0);
            let prev = (chain.len() >= 2).then(|| chain[chain.len() - 2]);
            // Nearest active neighbour of `a`; the previous chain element wins ties (that ends the chain).
            let (mut b, mut best) = match prev {
                Some(p) => (p, d[tri(n, a, p)]),
                None => (usize::MAX, f32::INFINITY),
            };
            for c in (0..n).filter(|&c| active[c] && c != a) {
                let dc = d[tri(n, a, c)];
                if dc < best {
                    best = dc;
                    b = c;
                }
            }
            if Some(b) == prev {
                chain.truncate(chain.len() - 2);
                let (x, y) = (a.min(b), a.max(b));
                out.push(Merge { a: x, b: y, distance: best });
                // Lance–Williams update for complete linkage.
                for c in (0..n).filter(|&c| active[c] && c != x && c != y) {
                    d[tri(n, x, c)] = d[tri(n, x, c)].max(d[tri(n, y, c)]);
                }
                active[y] = false;
                break;
            }
            chain.push(b);
        }
    }
    out
}

/// Cluster label per vector (0, 1, … in order of first appearance). `min_group`: fewest vectors a speaker needs
/// ([`Subset::min_group`]); smaller groups of a cut join the nearest larger one before the cut is scored.
pub fn cluster(vectors: &[Vec<f32>], min_group: usize) -> Vec<usize> {
    let n = vectors.len();
    if n < 3 {
        return vec![0; n];
    }
    let d = distances(vectors);
    let mut ms = merges(n, d.clone());
    ms.sort_by(|x, y| x.distance.total_cmp(&y.distance));
    let finest = MAX_SPEAKERS.min(n - 1);
    // Labels at the finest cut; every coarser cut joins whole groups of it.
    let fine = labels_after(n, &ms[..n - finest]);
    // Sum of distances from each vector to each finest group, and between finest groups.
    let mut sums = vec![0f32; n * finest];
    for i in 0..n {
        for j in i + 1..n {
            let x = d[tri(n, i, j)];
            sums[i * finest + fine[j]] += x;
            sums[j * finest + fine[i]] += x;
        }
    }
    let mut between = vec![0f32; finest * finest];
    for i in 0..n {
        for f in 0..finest {
            between[fine[i] * finest + f] += sums[i * finest + f];
        }
    }
    let mut fine_size = vec![0usize; finest];
    fine.iter().for_each(|&f| fine_size[f] += 1);
    let min_group = min_group.max(2);
    let mut best: Option<(f32, Vec<usize>)> = None;
    for k in 2..=finest {
        let labels = labels_after(n, &ms[..n - k]);
        let mut group = vec![0usize; finest];
        for (i, &f) in fine.iter().enumerate() {
            group[f] = labels[i];
        }
        let Some(group) = join_small_groups(&group, k, &fine_size, &between, finest, min_group) else { continue };
        let final_labels: Vec<usize> = fine.iter().map(|&f| group[f]).collect();
        let s = silhouette(n, k, &final_labels, &group, &sums, finest);
        if best.as_ref().is_none_or(|b| s > b.0) {
            best = Some((s, final_labels));
        }
    }
    match best {
        Some((s, labels)) if s >= MIN_SILHOUETTE => renumber(&labels),
        _ => vec![0; n],
    }
}

/// `group` (finest group → group of a cut into `k`) with every group under `min_group` vectors moved as a whole to
/// the larger group it is closest to on average; `None` when fewer than two groups are large enough.
fn join_small_groups(group: &[usize], k: usize, fine_size: &[usize], between: &[f32], finest: usize, min_group: usize) -> Option<Vec<usize>> {
    let mut size = vec![0usize; k];
    for f in 0..finest {
        size[group[f]] += fine_size[f];
    }
    let large: Vec<usize> = (0..k).filter(|&g| size[g] >= min_group).collect();
    if large.len() < 2 {
        return None;
    }
    let sum_between = |a: usize, b: usize| -> f32 {
        let mut s = 0.0;
        for fa in (0..finest).filter(|&f| group[f] == a) {
            for fb in (0..finest).filter(|&f| group[f] == b) {
                s += between[fa * finest + fb];
            }
        }
        s
    };
    let mut target: Vec<usize> = (0..k).collect();
    for g in (0..k).filter(|g| size[*g] < min_group && size[*g] > 0) {
        target[g] = *large
            .iter()
            .min_by(|&&a, &&b| (sum_between(g, a) / size[a] as f32).total_cmp(&(sum_between(g, b) / size[b] as f32)))
            .unwrap_or(&large[0]);
    }
    Some(group.iter().map(|&g| target[g]).collect())
}

/// Labels numbered 0, 1, … in order of first appearance.
fn renumber(labels: &[usize]) -> Vec<usize> {
    let mut ids: Vec<usize> = Vec::new();
    labels
        .iter()
        .map(|l| match ids.iter().position(|x| x == l) {
            Some(p) => p,
            None => {
                ids.push(*l);
                ids.len() - 1
            }
        })
        .collect()
}

/// Mean silhouette of the labeling `labels` (groups numbered below `k`, empty ones allowed), from the per-vector
/// distance sums to the finest groups and the group of each finest group.
fn silhouette(n: usize, k: usize, labels: &[usize], group: &[usize], sums: &[f32], finest: usize) -> f32 {
    let mut size = vec![0usize; k];
    labels.iter().for_each(|&l| size[l] += 1);
    let mut total = 0f32;
    let mut to = vec![0f32; k];
    for i in 0..n {
        to.iter_mut().for_each(|x| *x = 0.0);
        for f in 0..finest {
            to[group[f]] += sums[i * finest + f];
        }
        let own = labels[i];
        if size[own] < 2 {
            continue; // a group of one scores 0
        }
        let a = to[own] / (size[own] - 1) as f32;
        let b = (0..k).filter(|&c| c != own && size[c] > 0).map(|c| to[c] / size[c] as f32).fold(f32::INFINITY, f32::min);
        total += (b - a) / a.max(b).max(1e-12);
    }
    total / n as f32
}

/// Labels after applying `merges` (each joins the clusters of its two members).
fn labels_after(n: usize, merges: &[Merge]) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for m in merges {
        let (a, b) = (root(&mut parent, m.a), root(&mut parent, m.b));
        parent[b.max(a)] = a.min(b);
    }
    let mut ids: Vec<(usize, usize)> = Vec::new();
    (0..n)
        .map(|i| {
            let r = root(&mut parent, i);
            match ids.iter().find(|(root, _)| *root == r) {
                Some(&(_, id)) => id,
                None => {
                    ids.push((r, ids.len()));
                    ids.len() - 1
                }
            }
        })
        .collect()
}

/// Normalised mean vector of each cluster (`labels` from [`cluster`]).
pub fn centroids(vectors: &[Vec<f32>], labels: &[usize]) -> Vec<Vec<f32>> {
    let k = labels.iter().max().map_or(0, |m| m + 1);
    let dim = vectors.first().map_or(0, Vec::len);
    let mut sums = vec![vec![0f32; dim]; k];
    for (v, &l) in vectors.iter().zip(labels) {
        for (s, x) in sums[l].iter_mut().zip(normalized(v)) {
            *s += x;
        }
    }
    sums.iter().map(|s| normalized(s)).collect()
}

/// The centroid closest to `v` (cosine); 0 without centroids.
pub fn nearest(centroids: &[Vec<f32>], v: &[f32]) -> usize {
    let u = normalized(v);
    centroids.iter().enumerate().max_by(|a, b| dot(a.1, &u).total_cmp(&dot(b.1, &u))).map_or(0, |(i, _)| i)
}

#[cfg(test)]
#[path = "diarize_cluster_tests.rs"]
mod tests;
