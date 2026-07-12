//! Tests for `complete_section_chunks` — section-continuation lookup against
//! chunk fragmentation (multi-part §§/Artikel split across staging chunks by
//! the structural chunker). Mirrors `chunk_refs_tests.rs`'s seed pattern.

use super::SectionContinuation;
use crate::db::models::*;
use crate::db::Database;

fn db() -> Database {
    Database::open_in_memory().expect("open in-memory db")
}

/// Minimal model→context→document chain (mirrors `chunk_refs_tests.rs::seed`).
fn seed(db: &Database) -> (i64, i64) {
    let model = db
        .create_embedding_model(&NewEmbeddingModel {
            identifier: "test-embed".into(),
            kind: ModelKind::LocalOnnx,
            model_path: Some("/m.onnx".into()),
            tokenizer_path: Some("/t.json".into()),
            api_config: None,
            execution_provider: Some(ExecutionProvider::Ane),
            is_matryoshka: false,
            native_dim: 4,
            default_dim: 4,
            normalize: true,
            tpm_limit: None,
            rpm_limit: None,
            max_concurrency: 1,
        })
        .unwrap();
    let ctx = db
        .create_context(&NewContext {
            name: "Ctx".into(),
            description: None,
            chunking_profile_id: None,
            embedding_model_id: Some(model.id),
            embedding_dim: Some(4),
            llm_id: None,
            fallback_llm_id: None,
            ontology_profile_id: None,
            ontology_pool_id: None,
            ontology_extract_llm_id: None,
            ontology_extract_pool_id: None,
            ontology_extract_reasoning_effort: None,
            extract_title_llm: false,
            auto_merge_ontology: false,
            chunking_strategy: "Semantic".into(),
            structural_profile_id: None,
        })
        .unwrap();
    let doc = db
        .create_document(&NewDocument {
            context_id: ctx.id,
            name: "d.pdf".into(),
            zip_entry: None,
            byte_size: None,
            page_count: None,
            content_hash: None,
            extracted_text: None,
        })
        .unwrap();
    (ctx.id, doc.id)
}

/// Create a chunk with a `metadata.section` JSON field (the structural
/// chunker's continuation marker) and given `is_omitted`.
fn mk_section_chunk(
    db: &Database,
    ctx: i64,
    doc: i64,
    idx: i64,
    section: Option<&str>,
    omitted: bool,
) -> i64 {
    let metadata = match section {
        Some(s) => serde_json::json!({ "section": s }).to_string(),
        None => "{}".into(),
    };
    db.create_chunk(&NewChunk {
        context_id: ctx,
        document_id: doc,
        chunk_index: idx,
        char_start: 0,
        char_end: 1,
        text: format!("chunk {idx}"),
        signature: None,
        is_omitted: omitted,
        metadata,
    })
    .unwrap()
    .id
}

fn chunk_ids(sc: &SectionContinuation) -> Vec<i64> {
    sc.chunks.iter().map(|c| c.id).collect()
}

#[test]
fn continuation_over_multiple_chunks_in_index_order() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let a = mk_section_chunk(&db, ctx, doc, 0, Some("Artikel 395 (1)"), false);
    let b = mk_section_chunk(&db, ctx, doc, 1, Some("Artikel 395 (1)"), false);
    let c = mk_section_chunk(&db, ctx, doc, 2, Some("Artikel 395 (1)"), false);

    let result = db.complete_section_chunks(a, 5).unwrap();
    assert_eq!(chunk_ids(&result), vec![b, c]);
    assert_eq!(result.continues_at, None);
}

#[test]
fn cap_is_enforced_and_continues_at_reports_the_next_id() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let a = mk_section_chunk(&db, ctx, doc, 0, Some("Artikel 395 (1)"), false);
    let b = mk_section_chunk(&db, ctx, doc, 1, Some("Artikel 395 (1)"), false);
    let c = mk_section_chunk(&db, ctx, doc, 2, Some("Artikel 395 (1)"), false);
    let _d = mk_section_chunk(&db, ctx, doc, 3, Some("Artikel 395 (1)"), false);

    let result = db.complete_section_chunks(a, 2).unwrap();
    assert_eq!(chunk_ids(&result), vec![b, c]);
    assert_eq!(result.continues_at, Some(_d));
}

#[test]
fn section_change_stops_the_run() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let a = mk_section_chunk(&db, ctx, doc, 0, Some("Artikel 395 (1)"), false);
    let b = mk_section_chunk(&db, ctx, doc, 1, Some("Artikel 395 (1)"), false);
    let _c = mk_section_chunk(&db, ctx, doc, 2, Some("Artikel 395 (2)"), false);

    let result = db.complete_section_chunks(a, 5).unwrap();
    assert_eq!(chunk_ids(&result), vec![b]);
    assert_eq!(result.continues_at, None);
}

#[test]
fn empty_or_missing_section_yields_empty_result() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let no_section = mk_section_chunk(&db, ctx, doc, 0, None, false);
    let empty_section = mk_section_chunk(&db, ctx, doc, 1, Some(""), false);
    mk_section_chunk(&db, ctx, doc, 2, Some("Artikel 1"), false);

    let r1 = db.complete_section_chunks(no_section, 5).unwrap();
    assert!(r1.chunks.is_empty());
    assert_eq!(r1.continues_at, None);

    let r2 = db.complete_section_chunks(empty_section, 5).unwrap();
    assert!(r2.chunks.is_empty());
    assert_eq!(r2.continues_at, None);
}

#[test]
fn omitted_chunk_in_the_chain_stops_continuation() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let a = mk_section_chunk(&db, ctx, doc, 0, Some("Artikel 395 (1)"), false);
    let _b_omitted = mk_section_chunk(&db, ctx, doc, 1, Some("Artikel 395 (1)"), true);
    mk_section_chunk(&db, ctx, doc, 2, Some("Artikel 395 (1)"), false);

    let result = db.complete_section_chunks(a, 5).unwrap();
    assert!(result.chunks.is_empty(), "omitted chunk must end the run immediately");
    assert_eq!(result.continues_at, None);
}

#[test]
fn max_extra_zero_still_reports_continues_at() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let a = mk_section_chunk(&db, ctx, doc, 0, Some("Artikel 395 (1)"), false);
    let b = mk_section_chunk(&db, ctx, doc, 1, Some("Artikel 395 (1)"), false);

    let result = db.complete_section_chunks(a, 0).unwrap();
    assert!(result.chunks.is_empty());
    assert_eq!(result.continues_at, Some(b));

    // No continuation at all → None even with max_extra = 0.
    let solo = mk_section_chunk(&db, ctx, doc, 5, Some("Artikel 400"), false);
    let solo_result = db.complete_section_chunks(solo, 0).unwrap();
    assert!(solo_result.chunks.is_empty());
    assert_eq!(solo_result.continues_at, None);
}
