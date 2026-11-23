//! Tests of the place index with a few made-up rows in the GeoNames layout.

use std::collections::HashSet;

use super::*;

/// A `cities1000.txt` row.
#[allow(clippy::too_many_arguments)]
fn row(id: i64, name: &str, alt: &str, lat: f64, lon: f64, code: &str, cc: &str, pop: u32, zone: &str) -> String {
    let ascii: String = name.chars().filter(char::is_ascii).collect();
    format!("{id}\t{name}\t{ascii}\t{alt}\t{lat}\t{lon}\tP\t{code}\t{cc}\t\t09\t\t\t\t{pop}\t\t171\t{zone}\t2024-01-01")
}

fn sample() -> String {
    [
        row(2761369, "Vienna", "Bec,VIE,Viena,Wien,Wenen", 48.20849, 16.37208, "PPLC", "AT", 1_691_468, "Europe/Vienna"),
        row(2775260, "Innere Stadt", "", 48.20906, 16.37135, "PPLX", "AT", 16_450, "Europe/Vienna"),
        row(2778690, "Gerasdorf bei Wien", "", 48.29447, 16.46765, "PPLA3", "AT", 9_442, "Europe/Vienna"),
        row(5128581, "New York City", "NYC,New York,Nueva York", 40.71427, -74.00597, "PPL", "US", 8_804_190, "America/New_York"),
        row(2147714, "Sydney", "Sidney", -33.86785, 151.20732, "PPLA", "AU", 4_627_345, "Australia/Sydney"),
        row(4030939, "Fale old settlement", "", -9.38, -171.24, "PPL", "TK", 1_000, "Pacific/Fakaofo"),
        "broken\trow".into(),
    ]
    .join("\n")
}

fn index() -> PlaceIndex {
    PlaceIndex::parse(sample().as_bytes()).unwrap()
}

#[test]
fn rows_are_read_and_sections_skipped() {
    let ix = index();
    assert_eq!(ix.len(), 5, "PPLX and the broken row are left out");
    let v = ix.get(2761369).unwrap();
    assert_eq!((&*v.name, &*v.country, v.population, ix.zone(v)), ("Vienna", "AT", 1_691_468, "Europe/Vienna"));
    assert!(ix.get(2775260).is_none());
}

#[test]
fn the_nearest_place_is_found_across_cells_and_the_date_line() {
    let ix = index();
    // St. Stephen's: the city, not its section.
    let (p, d) = ix.nearest(48.2085, 16.3731).unwrap();
    assert_eq!((p.id, d < 1.0), (2761369, true));
    // North of Vienna, nearer to Gerasdorf.
    assert_eq!(ix.nearest(48.29, 16.46).unwrap().0.id, 2778690);
    // Manhattan from a neighbouring cell.
    assert_eq!(ix.nearest(41.02, -73.7).unwrap().0.id, 5128581);
    // Over the date line: 179.9° E is next to 171° W.
    assert_eq!(ix.nearest(-9.4, 179.9).unwrap().0.id, 4030939);
    // Far out at sea, still a place (with its distance).
    let (p, d) = ix.nearest(-40.0, 160.0).unwrap();
    assert_eq!(p.id, 2147714);
    assert!(d > 500.0);
    assert!(ix.nearest(91.0, 0.0).is_none());
    assert!(PlaceIndex::default().nearest(0.0, 0.0).is_none());
}

#[test]
fn distance_is_great_circle() {
    let d = distance_km(48.20849, 16.37208, 52.52437, 13.41053); // Vienna – Berlin
    assert!((520.0..530.0).contains(&d), "{d}");
}

#[test]
fn names_in_a_question_find_their_place() {
    let ids: HashSet<i64> = [2761369, 5128581].into();
    let names = alternate_names(sample().as_bytes(), &ids).unwrap();
    assert_eq!(names.len(), 2);
    assert!(!names[&2761369].iter().any(|n| n == "VIE"), "codes in capitals are left out");
    assert_eq!(mentioned("Wo war ich in Wien am Wasser?", &names), vec![2761369]);
    assert_eq!(mentioned("photos from new york and vienna", &names), vec![2761369, 5128581]);
    assert!(mentioned("La vie à Paris", &names).is_empty(), "no code, no part of a word");
    assert!(mentioned("York", &names).is_empty(), "a name needs all its words");
    assert!(mentioned("Wienerwald", &names).is_empty(), "whole words only");
}

#[test]
fn a_point_inside_a_large_city_belongs_to_it_not_to_the_nearer_small_place() {
    let rows = [
        row(1, "Big City", "", 40.0, -74.0, "PPL", "US", 8_000_000, "America/New_York"),
        row(2, "Small Town", "", 40.04, -73.95, "PPL", "US", 10_000, "America/New_York"),
    ]
    .join("\n");
    let ix = PlaceIndex::parse(rows.as_bytes()).unwrap();
    // 4.9 km from the city centre, 2.3 km from the town centre: inside the city's area (r ≈ 17.8 km), outside the town's.
    let p = (40.03, -73.975);
    assert_eq!(ix.nearest(p.0, p.1).unwrap().0.id, 2);
    assert_eq!(ix.locate(p.0, p.1).unwrap().0.id, 1);
    // At the town's own centre the town wins (it lies more centrally there).
    assert_eq!(ix.locate(40.04, -73.95).unwrap().0.id, 2);
    // Far from both: the nearest.
    assert_eq!(ix.locate(38.0, -74.1).unwrap().0.id, 1);
}
