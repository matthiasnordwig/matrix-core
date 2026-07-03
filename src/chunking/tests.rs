use super::*;

// --- sentence segmentation (still used inside segment splitting) ------------

#[test]
fn splits_basic_sentences() {
    let s = split_sentences("Hello world. How are you? I am fine!");
    assert_eq!(s.len(), 3);
}

#[test]
fn abbreviation_guard_does_not_oversplit() {
    let s = split_sentences("Dr. Smith met Mr. Lee at 5 p.m. Then they left.");
    assert_eq!(s.len(), 2);
}

#[test]
fn version_numbers_do_not_split() {
    let s = split_sentences("Use nomic v1.5 now. Done.");
    assert_eq!(s.len(), 2);
}

// --- segments: line OR sentence, whichever is finer -------------------------

#[test]
fn segments_break_on_lines_and_sentences() {
    let raw = "Überschrift\n- Punkt eins\n- Punkt zwei\nFließtext. Zweiter Satz.";
    let segs = split_segments(raw);
    assert_eq!(segs.len(), 5, "got: {:?}", segs.iter().map(|s| &s.text).collect::<Vec<_>>());
    assert_eq!(segs[0].text, "Überschrift");
    assert_eq!(segs[1].text, "- Punkt eins");
    assert_eq!(segs[3].text, "Fließtext.");
    assert_eq!(&raw[segs[1].byte_start..segs[1].byte_end], "- Punkt eins");
    assert_eq!(&raw[segs[4].byte_start..segs[4].byte_end], "Zweiter Satz.");
}

// --- token windows over segments, indexed render ----------------------------

#[test]
fn windows_render_indexed_segments() {
    let raw = "Eins. Zwei. Drei. Vier.";
    let segs = split_segments(raw);
    assert_eq!(segs.len(), 4);
    let big = build_windows(&segs, 100_000, 0.1);
    assert_eq!(big.len(), 1);
    assert!(big[0].text.starts_with("[0] Eins."));
    assert!(big[0].text.contains("[3] Vier."));
}

// --- index-based assembly ---------------------------------------------------

fn resp(starts: Vec<StartItem>, leave_out: Vec<usize>) -> LlmChunkResponse {
    LlmChunkResponse { starts, leave_out }
}

const RAW5: &str = "A eins. B zwei. C drei. D vier. E fuenf.";

#[test]
fn assembles_chunks_by_start_indices() {
    let chunks = assemble(RAW5, 1, 1, &[resp(vec![StartItem::Index(0), StartItem::Index(2)], vec![])]);
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].chunk_index, 0);
    assert!(chunks[0].text.contains("A eins.") && chunks[0].text.contains("B zwei."));
    assert!(chunks[1].text.contains("C drei.") && chunks[1].text.contains("E fuenf."));
}

#[test]
fn no_starts_yields_single_chunk() {
    // segment 0 is always forced → whole document as one chunk.
    let chunks = assemble(RAW5, 1, 1, &[resp(vec![], vec![])]);
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].text.contains("A eins.") && chunks[0].text.contains("E fuenf."));
}

#[test]
fn leave_out_drops_segments() {
    let chunks = assemble(RAW5, 1, 1, &[resp(vec![StartItem::Index(0)], vec![1])]);
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].text.contains("A eins."));
    assert!(!chunks[0].text.contains("B zwei."), "segment 1 should be dropped");
}

#[test]
fn metadata_inherits_last_heading() {
    let r = resp(
        vec![
            StartItem::Detailed { i: 0, section: Some("§1".into()), title: Some("Titel".into()) },
            StartItem::Index(2),
        ],
        vec![],
    );
    let chunks = assemble(RAW5, 1, 1, &[r]);
    assert_eq!(chunks.len(), 2);
    assert!(chunks[0].metadata.contains("§1"));
    assert!(chunks[1].metadata.contains("§1"), "second chunk inherits the heading");
}

#[test]
fn response_json_is_lenient() {
    // bare ints and detailed objects mixed
    let r: LlmChunkResponse =
        serde_json::from_str(r#"{"starts":[0,{"i":2,"title":"X"}],"leave_out":[1]}"#).unwrap();
    assert_eq!(r.starts.len(), 2);
    assert_eq!(r.leave_out, vec![1]);
}
