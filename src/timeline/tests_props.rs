//! Property tests (proptest) for the offline place index. Case count from `crate::props`
//! (`PROPTEST_CASES`, stage 2: 32), see `handbuch/arbeitsablauf.md` "Teststufen".
//! Counterexamples: `core/proptest-regressions/timeline/` (checked in) plus a named example test.
//!
//! Rules (from the doc comments in `place.rs`):
//! (24) `PlaceIndex::nearest` (grid searched "ring by ring until the next ring cannot hold
//!      anything nearer") finds the same distance as a search over every place — also across the
//!      date line, near the poles and in high latitudes, where a 1° cell is narrower than 111 km.
//! (25) `words`: twice is once; framed by spaces; never two spaces in a row.

use proptest::prelude::*;

use super::place::{distance_km, words, PlaceIndex};

/// A GeoNames row (18 tab-separated fields) with the given id and position.
fn row(id: usize, lat: f64, lon: f64) -> String {
    format!("{id}\tP{id}\tP{id}\t\t{lat}\t{lon}\tP\tPPL\tDE\t\t\t\t\t\t1000\t\t\tEurope/Berlin")
}

fn position() -> impl Strategy<Value = (f64, f64)> {
    prop_oneof![
        2 => (-89.9f64..89.9, -180.0f64..180.0),
        2 => (60.0f64..89.9, -180.0f64..180.0),
        1 => (-10.0f64..10.0, prop_oneof![170.0f64..180.0, -180.0f64..-170.0]),
    ]
}

/// Rule (24), shared by the property and the regression tests.
fn check_nearest(places: &[(f64, f64)], q: (f64, f64)) -> Result<(), TestCaseError> {
    let rows: Vec<String> = places.iter().enumerate().map(|(i, &(lat, lon))| row(i + 1, lat, lon)).collect();
    let index = PlaceIndex::parse(rows.join("\n").as_bytes()).unwrap();
    let want = places
        .iter()
        .map(|&(lat, lon)| distance_km(q.0, q.1, f64::from(lat as f32), f64::from(lon as f32)))
        .fold(f64::INFINITY, f64::min);
    match index.nearest(q.0, q.1) {
        Some((_, d)) => prop_assert!((d - want).abs() < 1e-6, "found {} km, nearest is {} km, query {:?} in {:?}", d, want, q, places),
        None => prop_assert!(places.is_empty()),
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::props::config())]

    /// Rule (24).
    #[test]
    fn rule_24_nearest_matches_brute_force(
        places in prop::collection::vec(position(), 0..12),
        q in position(),
        near in prop::collection::vec((-4.0f64..4.0, -12.0f64..12.0), 0..6),
    ) {
        // Places near the query (a few degrees off), where the ring order matters most.
        let mut all = places.clone();
        all.extend(near.iter().map(|&(dl, dn)| ((q.0 + dl).clamp(-89.9, 89.9), q.1 + dn)));
        check_nearest(&all, q)?;
    }

    /// Rule (25).
    #[test]
    fn rule_25_words_idempotent_and_framed(t in prop_oneof![".{0,30}", "[A-Za-zÄä0-9 ,.\\-]{0,30}"]) {
        let w = words(&t);
        prop_assert_eq!(words(&w), w.clone());
        prop_assert!(w.starts_with(' ') && w.ends_with(' '));
        prop_assert!(!w.contains("  "), "{:?} → {:?}", t, w);
    }
}

// Counterexamples proptest found, replayed verbatim (seeds in
// `core/proptest-regressions/timeline/tests_props.txt`).

/// Rule (24): at 84.5° N a place 11.8° of longitude away (126 km) lies in ring 12, but the
/// search stopped after ring 5 because it bounded a ring by 111 km per ring (true only for
/// latitude), and returned a place 400 km away.
#[test]
fn regression_rule_24_high_latitude_neighbour_east() {
    check_nearest(&[(84.48283828696555, 11.76612947534052), (80.88326819784105, 0.0)], (84.48283828696555, 0.0)).unwrap();
}
