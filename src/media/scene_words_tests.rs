//! Tests of `scene_words.rs`: questions in the app's languages name their labels; document questions using the
//! same words in another sense name none.

use super::*;
use crate::media::scene_labels::{is_key, keys};

#[test]
fn every_label_has_words_and_every_term_is_written_as_matched() {
    let listed: Vec<&str> = WORDS.iter().map(|(k, _)| *k).collect();
    assert_eq!(listed, keys().collect::<Vec<_>>(), "one entry per label, in key order");
    for (key, terms) in WORDS {
        assert!(is_key(key));
        for t in terms {
            let stem = t.strip_suffix('*').unwrap_or(t);
            assert_eq!(stem, stem.to_lowercase(), "{key}: {t} not lower case");
            assert!(!stem.chars().any(|c| c.is_ascii_punctuation()) && stem.trim() == stem && !stem.contains("  "), "{key}: {t}");
            assert!(named(stem).contains(&key), "{key}: \"{t}\" does not find itself");
        }
    }
}

#[test]
fn questions_in_every_language_name_their_label() {
    let cases: &[(&str, &[&str])] = &[
        ("Zeig mir nur Fotos von draußen", &["outdoors"]),
        ("Wo war ich im Sommer am Wasser?", &["water"]),
        ("Bilder in der Bar", &["bar_restaurant"]),
        ("Videos vom Strand", &["beach"]),
        ("Which photos were taken at the beach?", &["beach"]),
        ("show me only outdoors pictures, by the lake", &["outdoors", "water"]),
        ("fotos en la playa", &["beach"]),
        ("photos au bord de l'eau", &["water"]),
        ("foto sulla neve in montagna", &["nature", "snow"]),
        ("foto's op het strand", &["beach"]),
        ("zdjęcia na plaży", &["beach"]),
        ("fotos no restaurante", &["bar_restaurant"]),
        ("фотографии на пляже", &["beach"]),
        ("plajda çekilen fotoğraflar", &["beach"]),
        ("ảnh ở bãi biển", &["beach"]),
        ("我在海滩上的照片", &["beach"]),
        ("我在海灘上的照片", &["beach"]),
        ("ビーチで撮った写真", &["beach"]),
        ("해변에서 찍은 사진", &["beach"]),
        ("صور على الشاطئ", &["beach"]),
        ("समुद्र तट की तस्वीरें", &["beach", "water"]),
        ("Fotos im Schnee und auf der Bühne", &["snow", "stage"]),
        ("Aufnahmen drinnen im Büro", &["indoors", "office"]),
        ("im Zug nach Wien", &["vehicle"]),
        ("beim Sport im Stadion", &["sport"]),
        ("Videos zu Hause", &["home"]),
        ("Fotos in der Innenstadt", &["city"]),
    ];
    for (q, want) in cases {
        assert_eq!(named(q), *want, "{q}");
    }
}

#[test]
fn words_used_in_another_sense_name_nothing() {
    for q in [
        "Show the bar chart of revenue",
        "What does the office lease say about rent?",
        "Payments outside the EU",
        "betalen binnen 30 dagen",
        "Is the home loan fixed?",
        "a sporadic error in the report",
        "a concerted effort by the board",
        "je suis en train de lire le contrat",
        "à ce stade du projet",
        "bu yılın karları",
        "in the open source licence",
        "accidents at work",
        "Was steht im Mietvertrag?",
        "",
    ] {
        assert!(named(q).is_empty(), "{q}: {:?}", named(q));
    }
}

#[test]
fn inflected_forms_count_through_the_open_end() {
    assert_eq!(named("Strände und Restaurants"), ["bar_restaurant", "beach"]);
    assert_eq!(named("na plażę"), ["beach"]);
    assert_eq!(named("snowy mountains"), ["snow"]);
    assert_eq!(named("stranded ships"), Vec::<&str>::new(), "a closed German word does not go on");
}
