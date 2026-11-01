//! Paragraphs and topics of long texts.

use super::topics::{mean_vector, paragraph_units, topic_groups};

fn sentence(word: &str) -> String {
    format!("{word} ").repeat(20).trim_end().to_string()
}

#[test]
fn paragraphs_are_cut_at_blank_lines_only() {
    let t = format!("{}\n\n{}\nsame paragraph\n\n\n{}\n", sentence("a"), sentence("b"), sentence("c"));
    let u = paragraph_units(&t);
    assert_eq!(u.len(), 3);
    assert!(t[u[1].0..u[1].1].ends_with("same paragraph"));
}

#[test]
fn a_greeting_and_a_signature_go_with_their_neighbours() {
    let t = format!("Hi John,\n\n{}\n\n{}\n\nBest,\nJeff\n", sentence("a"), sentence("b"));
    let u = paragraph_units(&t);
    assert_eq!(u.len(), 2);
    assert!(t[u[0].0..u[0].1].starts_with("Hi John"));
    assert!(t[u[1].0..u[1].1].ends_with("Jeff"));
}

#[test]
fn a_text_of_short_lines_stays_one_unit() {
    assert_eq!(paragraph_units("Thanks.\n\nJeff\n").len(), 1);
}

#[test]
fn a_giant_paragraph_is_read_by_sentences() {
    let t = "Prices rose again today. ".repeat(1000);
    let u = paragraph_units(&t);
    assert!(u.len() > 1);
    assert!(u.iter().all(|&(s, e)| t[s..e].trim_end().ends_with("today.")));
}

#[test]
fn neighbours_on_one_topic_stay_together_and_a_new_topic_starts_a_group() {
    let a = vec![1.0, 0.0];
    let b = vec![0.9, 0.1];
    let c = vec![0.0, 1.0];
    assert_eq!(topic_groups(&[a.clone(), b, c], 0.8), vec![0..2, 2..3]);
    assert_eq!(topic_groups(&[a.clone()], 0.8), vec![0..1]);
    assert!(topic_groups(&[], 0.8).is_empty());
    let m = mean_vector(&[a, vec![0.0, 1.0]]);
    assert!((m[0] - m[1]).abs() < 1e-6 && (m.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-6);
}
