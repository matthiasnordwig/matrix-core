//! Agglomerative clustering of voice vectors into speakers, without a fixed number of speakers: the
//! stop comes from the distribution of merge distances. Memory stays bounded: a subset drawn across
//! the file's time (size from the memory budget) is clustered, every other vector goes to the nearest
//! centroid. Clustering rule and reference values: `handbuch/medien.md` „Sprecher“.
//! Scaffold: filled by its work package (`PLAN-ton-video-aufnahme.md` AP4).
