//! The offline place index: GeoNames `cities1000` (every place with at least 1000 inhabitants or a seat of
//! an administrative division; CC BY 4.0, named in the app's licences) parsed from its tab-separated rows.
//! A position becomes its place ([`PlaceIndex::locate`]: the place whose area holds it, else the nearest;
//! grid of 1° cells searched ring by ring, great-circle distance), a place id becomes a name for the answer context ([`PlaceIndex::get`]), and a
//! question or filter text names a place when one of the place's names stands in it as whole words
//! ([`mentioned`]). No online geocoder: positions never leave the device.
//!
//! **Kept in memory** per place: id, position, name, country, time zone (an index into the zones list,
//! for the timeline's local time), population — about 7 MB for the 170 000 rows. The alternate names (13
//! MB, every language, no language tags) are read only for the few places a collection holds
//! ([`alternate_names`]), when a question is matched against them. Sections of a city (`PPLX`: "Innere
//! Stadt") and historical, abandoned or destroyed places are skipped, so a photo in a city centre names
//! the city.
//!
//! Row layout (GeoNames `readme.txt`): 0 id, 1 name, 2 ASCII name, 3 alternate names (comma-separated),
//! 4 latitude, 5 longitude, 6 feature class, 7 feature code, 8 country code, …, 14 population, …, 17 time zone.

use std::collections::{HashMap, HashSet};
use std::io::BufRead;

/// One place of the index.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub id: i64,
    pub lat: f32,
    pub lon: f32,
    pub name: Box<str>,
    pub ascii_name: Box<str>,
    /// ISO 3166 alpha-2 country code.
    pub country: Box<str>,
    pub population: u32,
    zone: u16,
}

/// Feature codes left out: sections of a city and places that no longer exist.
const SKIPPED: [&str; 4] = ["PPLX", "PPLH", "PPLQ", "PPLW"];

/// Mean earth radius (km) for the great-circle distance.
const EARTH_KM: f64 = 6371.0;

/// The parsed index.
#[derive(Debug, Default)]
pub struct PlaceIndex {
    places: Vec<Place>,
    zones: Vec<Box<str>>,
    by_id: HashMap<i64, u32>,
    cells: HashMap<(i16, i16), Vec<u32>>,
    max_population: u32,
}

/// People per km² over which a place's population is spread to give it an area ([`PlaceIndex::locate`]).
/// Measured on 14 positions (city sights, suburb centres; services `places_tests.rs::real_*`): 2000 and 4000
/// gave a suburb centre (Schwechat) to Vienna, 8000 to 32000 all right; the smallest of those (city
/// cores are denser than that, so a large city's area is rather too small than too large).
pub const DENSITY: f64 = 8000.0;

fn cell(lat: f64, lon: f64) -> (i16, i16) {
    (lat.floor().clamp(-90.0, 89.0) as i16, lon.floor().rem_euclid(360.0) as i16)
}

/// Great-circle distance between two positions in km.
pub fn distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = p2 - p1;
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_KM * a.sqrt().min(1.0).asin()
}

impl PlaceIndex {
    /// Parses `cities1000.txt` rows; malformed rows are skipped. Errors only when reading fails.
    pub fn parse(reader: impl BufRead) -> std::io::Result<PlaceIndex> {
        let mut index = PlaceIndex::default();
        let mut zone_ids: HashMap<String, u16> = HashMap::new();
        for line in reader.lines() {
            let line = line?;
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 18 || SKIPPED.contains(&f[7]) {
                continue;
            }
            let (Ok(id), Ok(lat), Ok(lon)) = (f[0].parse::<i64>(), f[4].parse::<f64>(), f[5].parse::<f64>()) else { continue };
            let next = zone_ids.len() as u16;
            let zone = *zone_ids.entry(f[17].to_string()).or_insert_with(|| {
                index.zones.push(f[17].into());
                next
            });
            let i = index.places.len() as u32;
            index.places.push(Place {
                id,
                lat: lat as f32,
                lon: lon as f32,
                name: f[1].into(),
                ascii_name: f[2].into(),
                country: f[8].into(),
                population: f[14].parse().unwrap_or(0),
                zone,
            });
            index.by_id.insert(id, i);
            index.max_population = index.max_population.max(index.places[i as usize].population);
            index.cells.entry(cell(lat, lon)).or_default().push(i);
        }
        Ok(index)
    }

    pub fn len(&self) -> usize {
        self.places.len()
    }

    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }

    /// The place with GeoNames id `id`.
    pub fn get(&self, id: i64) -> Option<&Place> {
        self.by_id.get(&id).map(|&i| &self.places[i as usize])
    }

    /// The IANA time zone of `place` (`Europe/Vienna`).
    pub fn zone(&self, place: &Place) -> &str {
        self.zones.get(place.zone as usize).map_or("", |z| z)
    }

    /// The place nearest to (`lat`, `lon`) and its distance in km; `None` for an empty index or a position
    /// off the globe. Searches the cells around the position ring by ring until the next ring cannot hold
    /// anything nearer (a ring of cells lies at least its radius in degrees of latitude away).
    pub fn nearest(&self, lat: f64, lon: f64) -> Option<(&Place, f64)> {
        self.nearest_index(lat, lon).map(|(i, d)| (&self.places[i as usize], d))
    }

    /// The place a position belongs to: among the places whose area holds it (a circle of the place's
    /// population at [`DENSITY`]) the one it lies most centrally in (smallest distance / radius), else the
    /// nearest. A point in Manhattan is New York City although Weehawken's centre across the river is nearer;
    /// a suburb's own centre stays the suburb.
    pub fn locate(&self, lat: f64, lon: f64) -> Option<(&Place, f64)> {
        self.locate_with(lat, lon, DENSITY)
    }

    /// [`PlaceIndex::locate`] with another density (measurement).
    pub fn locate_with(&self, lat: f64, lon: f64, density: f64) -> Option<(&Place, f64)> {
        let (near, d) = self.nearest_index(lat, lon)?;
        let radius = |pop: u32| (f64::from(pop) / (std::f64::consts::PI * density)).sqrt();
        let reach = radius(self.max_population);
        let rings_lat = (reach / 111.0).ceil() as i16 + 1;
        let rings_lon = ((reach / (111.0 * lat.to_radians().cos().max(0.05))).ceil() as i16 + 1).min(180);
        let (cy, cx) = cell(lat, lon);
        let mut best: Option<(u32, f64, f64)> = None; // index, distance, distance / radius
        for dy in -rings_lat..=rings_lat {
            let y = cy + dy;
            if !(-90..90).contains(&y) {
                continue;
            }
            for dx in -rings_lon..=rings_lon {
                for &i in self.cells.get(&(y, (cx + dx).rem_euclid(360))).into_iter().flatten() {
                    let p = &self.places[i as usize];
                    let di = distance_km(lat, lon, f64::from(p.lat), f64::from(p.lon));
                    let r = radius(p.population);
                    if di <= r && best.is_none_or(|(_, _, s)| di / r < s) {
                        best = Some((i, di, di / r));
                    }
                }
            }
        }
        let (i, d) = best.map_or((near, d), |(i, d, _)| (i, d));
        Some((&self.places[i as usize], d))
    }

    fn nearest_index(&self, lat: f64, lon: f64) -> Option<(u32, f64)> {
        if self.places.is_empty() || !(-90.0..=90.0).contains(&lat) || !(-180.0..=360.0).contains(&lon) {
            return None;
        }
        let (cy, cx) = cell(lat, lon);
        let mut best: Option<(u32, f64)> = None;
        for ring in 0i16..=180 {
            // Anything in ring `r` lies at least (r - 1) degrees of latitude ≈ 111 km per degree away.
            if let Some((_, d)) = best && f64::from(ring - 1) * 111.0 > d {
                break;
            }
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    if dy.abs() != ring && dx.abs() != ring {
                        continue;
                    }
                    let y = cy + dy;
                    if !(-90..90).contains(&y) {
                        continue;
                    }
                    let x = (cx + dx).rem_euclid(360);
                    for &i in self.cells.get(&(y, x)).into_iter().flatten() {
                        let p = &self.places[i as usize];
                        let d = distance_km(lat, lon, f64::from(p.lat), f64::from(p.lon));
                        if best.is_none_or(|(_, b)| d < b) {
                            best = Some((i, d));
                        }
                    }
                }
            }
        }
        best
    }
}

/// Name, ASCII name and alternate names of the places `ids` from `cities1000.txt` rows (a second read of
/// the file, only for the places a collection holds). Codes in capitals (`VIE`) and names shorter than
/// three letters are left out: they stand in ordinary text too often.
pub fn alternate_names(reader: impl BufRead, ids: &HashSet<i64>) -> std::io::Result<HashMap<i64, Vec<String>>> {
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    for line in reader.lines() {
        let line = line?;
        let mut f = line.splitn(5, '\t');
        let Some(id) = f.next().and_then(|v| v.parse::<i64>().ok()).filter(|id| ids.contains(id)) else { continue };
        let (name, ascii, alt) = (f.next().unwrap_or(""), f.next().unwrap_or(""), f.next().unwrap_or(""));
        let mut names: Vec<String> = Vec::new();
        for n in [name, ascii].into_iter().chain(alt.split(',')) {
            let n = n.trim();
            let letters = n.chars().filter(|c| c.is_alphabetic()).count();
            let code = n.chars().all(|c| !c.is_lowercase()) && n.chars().any(char::is_uppercase);
            if letters >= 3 && !code && !names.iter().any(|m| m.eq_ignore_ascii_case(n)) {
                names.push(n.to_string());
            }
        }
        out.insert(id, names);
    }
    Ok(out)
}

/// Lower case, every run of non-alphanumeric characters one space, padded with spaces: whole-word search
/// by plain `contains` (place names here, label words in `media/scene_words.rs`).
pub fn words(text: &str) -> String {
    let mut out = String::from(" ");
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

/// The places (ids ascending) one of whose `names` stands in `text` as whole words, ignoring case ("in
/// Wien" names Vienna through its alternate name, "New York" needs both words).
pub fn mentioned(text: &str, names: &HashMap<i64, Vec<String>>) -> Vec<i64> {
    let hay = words(text);
    let mut ids: Vec<i64> = names
        .iter()
        .filter(|(_, ns)| ns.iter().any(|n| {
            let w = words(n);
            w.trim() != "" && hay.contains(&w)
        }))
        .map(|(&id, _)| id)
        .collect();
    ids.sort_unstable();
    ids
}

#[cfg(test)]
#[path = "place_tests.rs"]
mod tests;
