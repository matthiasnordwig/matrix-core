//! Pure rules of carrying graph evidence onto new chunks (`evidence_carry.rs`):
//! placement tiers, unplaced pieces, the extracted rule, changed document text,
//! the last rules for a new cut across the old one (quote over two chunks, shared words).
use crate::db::ontology::evidence_carry::*;

fn old(id: i64, s: i64, e: i64, text: &str, extracted: bool) -> OldChunk {
    OldChunk { id, char_start: s, char_end: e, text: text.into(), extracted }
}

fn ev(edge: i64, chunk: i64, quote: Option<&str>, a: &str, b: &str) -> OldEvidence {
    OldEvidence { kind: EvidenceKind::Edge, edge_id: edge, chunk_id: chunk, evidence: quote.map(Into::into), labels: [a.into(), b.into()] }
}

fn new(id: i64, s: i64, e: i64, text: &str) -> NewChunkSpan {
    NewChunkSpan { id, char_start: s, char_end: e, text: text.into(), is_omitted: false }
}

fn graph(chunks: Vec<OldChunk>, evidence: Vec<OldEvidence>) -> DocumentGraph {
    DocumentGraph { context_id: 1, document_id: 1, text_fingerprint: "x".into(), chunks, evidence }
}

const A: &str = "Die KfW fördert den Mittelstand in Deutschland.";
const B: &str = "Der Bund garantiert die Verbindlichkeiten der Bank.";

#[test]
fn single_overlapping_chunk_takes_the_evidence() {
    let g = graph(vec![old(10, 0, 50, A, true)], vec![ev(1, 10, None, "Foo", "Bar")]);
    let p = plan_carry(&g, true, &[new(20, 0, 50, A)]);
    assert_eq!(p.placed, vec![(0, 20, MatchTier::SinglePlace)]);
    assert!(p.unplaced.is_empty());
    assert_eq!(p.extracted, vec![20]);
}

#[test]
fn split_chunk_quote_decides() {
    let both = format!("{A} {B}");
    let g = graph(vec![old(10, 0, 100, &both, true)], vec![ev(1, 10, Some("garantiert die\nVerbindlichkeiten"), "x", "y")]);
    let p = plan_carry(&g, true, &[new(20, 0, 48, A), new(21, 48, 100, B)]);
    assert_eq!(p.placed, vec![(0, 21, MatchTier::EvidenceText)]);
    assert_eq!(p.extracted, vec![20, 21]);
}

#[test]
fn split_chunk_without_quote_goes_by_labels_then_overlap() {
    let both = format!("{A} {B}");
    let g = graph(
        vec![old(10, 0, 100, &both, true)],
        vec![ev(1, 10, None, "KFW", "mittelstand"), ev(2, 10, None, "Nirgends", "Niemand")],
    );
    // The second new chunk overlaps more (52 vs 48 bytes).
    let p = plan_carry(&g, true, &[new(20, 0, 48, A), new(21, 48, 100, B)]);
    assert_eq!(p.placed, vec![(0, 20, MatchTier::Labels), (1, 21, MatchTier::Overlap)]);
}

#[test]
fn evidence_without_any_place_is_reported() {
    let g = graph(vec![old(10, 500, 600, A, true)], vec![ev(1, 10, None, "a", "b"), ev(2, 99, None, "a", "b")]);
    let p = plan_carry(&g, true, &[new(20, 0, 100, A)]);
    assert!(p.placed.is_empty());
    assert_eq!(p.unplaced, vec![0, 1]);
    // The new chunk is not covered by an old extracted chunk.
    assert!(p.extracted.is_empty());
}

#[test]
fn extracted_only_when_fully_covered_by_extracted_chunks() {
    let g = graph(vec![old(10, 0, 50, A, true), old(11, 50, 100, B, true), old(12, 100, 150, A, false)], vec![]);
    let p = plan_carry(
        &g,
        true,
        &[
            new(20, 0, 100, "covered by two extracted"),
            new(21, 90, 120, "reaches into the unextracted one"),
            new(22, 140, 200, "reaches beyond every old chunk"),
        ],
    );
    assert_eq!(p.extracted, vec![20]);
}

#[test]
fn omitted_new_chunks_get_nothing() {
    let g = graph(vec![old(10, 0, 50, A, true)], vec![ev(1, 10, None, "a", "b")]);
    let mut omitted = new(20, 0, 50, A);
    omitted.is_omitted = true;
    let p = plan_carry(&g, true, &[omitted, new(21, 0, 50, A)]);
    assert_eq!(p.placed, vec![(0, 21, MatchTier::SinglePlace)]);
    assert_eq!(p.extracted, vec![21]);
}

#[test]
fn changed_text_locates_by_content() {
    let g = graph(
        vec![old(10, 0, 48, A, true), old(11, 48, 100, B, false)],
        vec![ev(1, 10, None, "a", "b"), ev(2, 11, Some("Der Bund garantiert"), "a", "b")],
    );
    // New text: other offsets, A now inside a longer chunk, B gone except its quote.
    let p = plan_carry(
        &g,
        false,
        &[new(30, 0, 120, &format!("Neu: {A} Und mehr Text danach.")), new(31, 120, 160, "Der Bund garantiert nur noch wenig.")],
    );
    assert_eq!(p.placed, vec![(0, 30, MatchTier::SinglePlace), (1, 31, MatchTier::EvidenceText)]);
    // 30 holds more than the old extracted chunk; 31's text is in no extracted chunk.
    assert!(p.extracted.is_empty());
    let p = plan_carry(&g, false, &[new(32, 0, 30, "fördert den Mittelstand in Deutschland")]);
    assert_eq!(p.extracted, vec![32]);
}

#[test]
fn fingerprint_is_stable_and_length_aware() {
    assert_eq!(text_fingerprint("abc"), text_fingerprint("abc"));
    assert_ne!(text_fingerprint("abc"), text_fingerprint("abd"));
    assert!(text_fingerprint("").ends_with("-0"));
}

#[test]
fn report_summary_counts() {
    let r = EvidenceCarryReport { evidence: 3, by_text: 1, by_place: 1, unplaced: 1, extracted_before: 2, ..Default::default() };
    assert_eq!(r.placed(), 2);
    assert!(!r.is_empty());
    assert!(r.summary().starts_with("evidence 2/3 carried over"));
    assert!(EvidenceCarryReport::default().is_empty());
}

#[test]
fn a_cut_across_the_old_chunk_still_places_quote_and_labels() {
    // Old chunk A+B; the new reader cut in the middle of B, so no new chunk contains
    // the old text or lies inside it, and the quote runs over the cut.
    let ab = format!("{A} {B}");
    let g = graph(
        vec![old(10, 0, 100, &ab, true)],
        vec![
            ev(1, 10, Some("garantiert die Verbindlichkeiten"), "Bund", "Bank"),
            ev(2, 10, None, "KfW", "Mittelstand"),
            ev(3, 10, None, "Fremd", "Anders"),
        ],
    );
    let first = format!("Seite 1\n{A} Der Bund garantiert");
    let second = "die Verbindlichkeiten der Bank. Seite 2 folgt hier.";
    let p = plan_carry(&g, false, &[new(30, 0, 60, &first), new(31, 60, 120, second)]);
    // Rule 5: the chunk where the quote starts; rule 6: the chunk naming KfW with most shared words;
    // an edge whose ends no chunk names stays unplaced (counted).
    assert_eq!(p.placed, vec![(0, 30, MatchTier::EvidenceText), (1, 30, MatchTier::Labels)]);
    assert_eq!(p.unplaced, vec![2]);
    // Same text: the old range decides, nothing is guessed by words.
    let p = plan_carry(&g, true, &[new(30, 200, 260, &first), new(31, 260, 320, second)]);
    assert_eq!((p.placed, p.unplaced), (vec![(0, 30, MatchTier::EvidenceText)], vec![1, 2]));
}
