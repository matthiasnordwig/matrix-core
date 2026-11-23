//! Fixed scene labels of a picture (a video's scene still, a photo) from the image vector it already has:
//! no new model. The image vectors of a collection live in the text space of its embedding model
//! (`embedding/vision.rs`), so each label is a short English text embedded like a search question, and a
//! picture gets a label when its vector is clearly closer to that text than to the others (rule below).
//! Labels are keys (`outdoors`, `water`, …); the UI and the answer context name them.
//!
//! **Rule (measured on 135 hand-labelled photos, `handbuch/medien.md` „Szenen-Etiketten“):**
//! - *Setting:* `indoors` or `outdoors`, whichever text is closer.
//! - *Place and content* (all other labels, several may hold): a label holds when its similarity lies
//!   more than [`SPREADS`] standard deviations above the mean of the picture's similarities to all place
//!   texts. The yardstick comes from the picture and the model themselves: text-to-image similarities
//!   differ by model and by picture (a dark, busy or painted picture is less similar to every text), so a
//!   fixed cosine would hold for one model only. Measured against plain captions ("a photo") as the
//!   yardstick, which lost recall where labels go together (a beach is also water).
//!
//! Pure; the services crate embeds the texts ([`prompts`]) with the collection's model and stores the
//! labels (`services::commands::scene_labels`).

/// One label: its key (stored, filtered by) and the text its vector comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label {
    pub key: &'static str,
    pub text: &'static str,
}

const fn l(key: &'static str, text: &'static str) -> Label {
    Label { key, text }
}

/// The two settings; a picture gets the closer one.
pub const SETTING: [Label; 2] = [
    l("indoors", "a photo taken indoors, inside a room or hall"),
    l("outdoors", "a photo taken outdoors under the open sky"),
];

/// Places and content; each holds or not on its own.
pub const PLACES: [Label; 11] = [
    l("nature", "a natural landscape outdoors with plants, trees or mountains"),
    l("city", "a city: streets, buildings and a skyline"),
    l("home", "a room in a private home: living room, kitchen or bedroom"),
    l("office", "an office workplace with desks and computer screens"),
    l("bar_restaurant", "a bar, café or restaurant with tables"),
    l("beach", "a beach with sand by the sea"),
    l("water", "a view across water: sea, lake or river with its shore"),
    l("vehicle", "inside or in front of a vehicle: a car, train, bus, boat or airplane"),
    l("sport", "sports: athletes, a sports field, a stadium or a gym"),
    l("snow", "snow and ice in winter"),
    l("stage", "a stage with performers or a speaker in front of an audience"),
];

/// How far above the mean of the place similarities a label must lie, in standard deviations of them
/// (dimensionless; F1 0.89 at precision 0.89 on the measured set, 1.25 and 1.75 both lower).
pub const SPREADS: f32 = 1.5;

/// Every label key, setting first, in the order of [`SETTING`] and [`PLACES`].
pub fn keys() -> impl Iterator<Item = &'static str> {
    SETTING.iter().chain(PLACES.iter()).map(|l| l.key)
}

/// Whether `key` is a label.
pub fn is_key(key: &str) -> bool {
    keys().any(|k| k == key)
}

/// The texts to embed (as search questions, with the model's query prefix), in the order
/// [`LabelVectors::new`] takes their vectors: [`SETTING`], then [`PLACES`].
pub fn prompts() -> Vec<&'static str> {
    SETTING.iter().chain(PLACES.iter()).map(|l| l.text).collect()
}

/// The vectors of [`prompts`] for one model.
#[derive(Debug, Clone)]
pub struct LabelVectors {
    setting: Vec<Vec<f32>>,
    places: Vec<Vec<f32>>,
}

impl LabelVectors {
    /// `vectors` in the order of [`prompts`]; `None` when the count does not match.
    pub fn new(mut vectors: Vec<Vec<f32>>) -> Option<Self> {
        if vectors.len() != SETTING.len() + PLACES.len() {
            return None;
        }
        let places = vectors.split_off(SETTING.len());
        Some(LabelVectors { setting: vectors, places })
    }

    /// Length of the vectors (a picture vector of another length is from another model).
    pub fn dim(&self) -> usize {
        self.setting.first().map_or(0, Vec::len)
    }
}

/// Similarities of a picture vector to the label texts.
#[derive(Debug, Clone, PartialEq)]
pub struct Scores {
    pub setting: Vec<f32>,
    pub places: Vec<f32>,
}

/// The cosine similarities of `picture` to every text.
pub fn scores(picture: &[f32], v: &LabelVectors) -> Scores {
    let sim = |t: &Vec<f32>| crate::db::embeddings::cosine(picture, t);
    Scores { setting: v.setting.iter().map(sim).collect(), places: v.places.iter().map(sim).collect() }
}

/// The labels of a picture from its [`Scores`] (rule in the head comment), setting first, then places
/// in the order of [`PLACES`]. Empty for a vector of another length than the texts'.
pub fn labels(picture: &[f32], v: &LabelVectors) -> Vec<&'static str> {
    if picture.len() != v.dim() || v.dim() == 0 {
        return Vec::new();
    }
    labels_from(&scores(picture, v))
}

/// [`labels`] on given similarities.
pub fn labels_from(s: &Scores) -> Vec<&'static str> {
    let mut out = Vec::new();
    if let (Some(&i), Some(&o)) = (s.setting.first(), s.setting.get(1)) {
        out.push(if i >= o { SETTING[0].key } else { SETTING[1].key });
    }
    out.extend(places_above(&s.places, SPREADS));
    out
}

/// The place labels whose similarity lies more than `spreads` standard deviations above the mean of
/// `places` (the similarities in the order of [`PLACES`]); none when all are equal.
pub fn places_above(places: &[f32], spreads: f32) -> Vec<&'static str> {
    if places.len() < 2 {
        return Vec::new();
    }
    let mean = places.iter().sum::<f32>() / places.len() as f32;
    let sd = (places.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / places.len() as f32).sqrt();
    PLACES.iter().zip(places).filter(|&(_, &x)| sd > 0.0 && x - mean > spreads * sd).map(|(l, _)| l.key).collect()
}

#[cfg(test)]
#[path = "scene_labels_tests.rs"]
mod tests;
