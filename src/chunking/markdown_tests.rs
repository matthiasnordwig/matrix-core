use super::markdown::{chunk_markdown_structurally as chunk, DEFAULT_MAX_CHARS};
use crate::db::models::NewChunk;

fn meta(c: &NewChunk) -> serde_json::Value {
    serde_json::from_str(&c.metadata).unwrap()
}

fn path(c: &NewChunk) -> String {
    meta(c)["heading_path"].as_str().unwrap().to_string()
}

/// Core invariant: offsets are byte offsets and slice back to the exact text.
fn assert_offsets(text: &str, chunks: &[NewChunk]) {
    let mut prev_end = 0;
    for (i, c) in chunks.iter().enumerate() {
        assert_eq!(c.chunk_index, i as i64);
        assert_eq!(&text[c.char_start as usize..c.char_end as usize], c.text, "chunk {i}");
        assert!(c.char_start >= prev_end, "chunks overlap at {i}");
        prev_end = c.char_end;
    }
}

#[test]
fn hierarchy_builds_heading_path() {
    let t = "# A\nintro\n\n## B\nbody b\n\n### C\nbody c\n\n## D\nbody d\n\n# E\nbody e\n";
    let cs = chunk(t, 1, 2, 1000);
    assert_offsets(t, &cs);
    let paths: Vec<String> = cs.iter().map(path).collect();
    assert_eq!(paths, ["A", "A › B", "A › B › C", "A › D", "E"]);
    assert_eq!(meta(&cs[2])["level"], 3);
    assert_eq!(meta(&cs[2])["section"], "C");
    assert_eq!(meta(&cs[2])["chunk_type"], "markdown_section");
    assert_eq!((cs[0].context_id, cs[0].document_id), (1, 2));
    assert!(cs[2].text.starts_with("### C"));
}

#[test]
fn preamble_is_own_section_with_empty_path() {
    let t = "Intro text\n\n# A\nbody\n";
    let cs = chunk(t, 1, 1, 1000);
    assert_offsets(t, &cs);
    assert_eq!(cs.len(), 2);
    assert_eq!(path(&cs[0]), "");
    assert_eq!(meta(&cs[0])["level"], 0);
    assert_eq!(cs[0].text, "Intro text");
}

#[test]
fn code_fence_with_hash_is_not_cut() {
    let t = "# A\n```bash\n# comment\n## not a heading\n```\n\n~~~\n# tilde\n~~~\n\n## B\nx\n";
    let cs = chunk(t, 1, 1, 1000);
    assert_offsets(t, &cs);
    assert_eq!(cs.len(), 2);
    assert!(cs[0].text.contains("# comment") && cs[0].text.contains("# tilde"));
    assert_eq!(path(&cs[1]), "A › B");
}

#[test]
fn longer_fence_is_closed_only_by_equal_or_longer() {
    let t = "# A\n````\n```\n# inside\n```\n````\n# B\nz\n";
    let cs = chunk(t, 1, 1, 1000);
    assert_eq!(cs.len(), 2);
    assert!(cs[0].text.contains("# inside"));
}

#[test]
fn hash_without_space_or_indented_code_is_no_heading() {
    let t = "# A\n#hashtag\n    # indented code\n####### seven\n";
    let cs = chunk(t, 1, 1, 1000);
    assert_eq!(cs.len(), 1);
}

#[test]
fn closing_hashes_are_stripped() {
    let t = "## Title ##\nbody\n";
    let cs = chunk(t, 1, 1, 1000);
    assert_eq!(path(&cs[0]), "Title");
}

#[test]
fn long_section_splits_at_paragraphs_with_same_path() {
    let para = "word ".repeat(20); // 100 chars
    let body = (0..10).map(|_| para.trim().to_string()).collect::<Vec<_>>().join("\n\n");
    let t = format!("# A\n\n## B\n\n{body}\n");
    let cs = chunk(&t, 1, 1, 350);
    assert_offsets(&t, &cs);
    assert!(cs.len() >= 3, "got {}", cs.len());
    for c in &cs {
        assert_eq!(path(c), "A › B");
        assert!(c.text.chars().count() <= 350, "len {}", c.text.chars().count());
        assert_eq!(meta(c)["parts"], cs.len());
    }
    assert!(cs[0].text.starts_with("# A\n\n## B"));
    assert_eq!(meta(&cs[1])["part"], 2);
}

#[test]
fn heading_stays_with_first_paragraph_when_splitting() {
    let big = "x".repeat(500);
    let t = format!("# A\n\n{big}\n\n{big}\n");
    let cs = chunk(&t, 1, 1, 300);
    assert_offsets(&t, &cs);
    assert_eq!(cs.len(), 2); // oversized single paragraph stays whole
    assert!(cs[0].text.starts_with("# A") && cs[0].text.ends_with(&big));
}

#[test]
fn fence_with_blank_lines_is_never_split() {
    let code = format!("```\n{}\n\n{}\n```", "a".repeat(200), "b".repeat(200));
    let t = format!("# A\n\n{code}\n\ntail\n");
    let cs = chunk(&t, 1, 1, 250);
    assert_offsets(&t, &cs);
    assert!(cs.iter().any(|c| c.text.contains(&code)));
}

#[test]
fn heading_only_sections_merge_into_following() {
    let t = "# Manual\n## Install\nsteps\n";
    let cs = chunk(t, 1, 1, 1000);
    assert_offsets(t, &cs);
    assert_eq!(cs.len(), 1);
    assert!(cs[0].text.starts_with("# Manual\n## Install"));
    assert_eq!(path(&cs[0]), "Manual › Install");
}

#[test]
fn trailing_heading_only_attaches_to_previous() {
    let t = "# A\nbody\n\n# Empty\n";
    let cs = chunk(t, 1, 1, 1000);
    assert_offsets(t, &cs);
    assert_eq!(cs.len(), 1);
    assert!(cs[0].text.ends_with("# Empty"));
}

#[test]
fn only_a_heading_yields_one_chunk_and_blank_input_none() {
    assert_eq!(chunk("# Just\n", 1, 1, 1000).len(), 1);
    assert!(chunk("", 1, 1, 1000).is_empty());
    assert!(chunk("\n\n  \n", 1, 1, 1000).is_empty());
}

#[test]
fn multibyte_and_crlf_offsets_are_exact() {
    let t = "# Überblick äöü §\r\nGrüße — „Zitat“ €\r\n\r\n## Näher\r\n日本語 text\r\n";
    let cs = chunk(t, 1, 1, 1000);
    assert_offsets(t, &cs);
    assert_eq!(cs.len(), 2);
    assert_eq!(path(&cs[1]), "Überblick äöü § › Näher");
    assert!(cs[1].text.contains("日本語"));
}

#[test]
fn zero_max_uses_default_and_signature_is_prefix() {
    let t = format!("# A\n{}\n", "y".repeat(DEFAULT_MAX_CHARS - 10));
    let cs = chunk(&t, 1, 1, 0);
    assert_eq!(cs.len(), 1);
    assert_eq!(cs[0].signature.as_deref().unwrap().chars().count(), 80);
}
