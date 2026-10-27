//! Tests for `complete_section_chunks` — section-continuation lookup against
//! chunk fragmentation (multi-part §§/Artikel split across staging chunks by
//! the structural chunker): along the outline read from the texts where it knows
//! headings, else along `metadata.section` (the `mk_section_chunk` cases, whose
//! texts have none). Mirrors `chunk_refs_tests.rs`'s seed pattern.

use super::{ChunkSlot, SectionContinuation};
use crate::db::models::*;
use crate::db::Database;

fn db() -> Database {
    Database::open_in_memory().expect("open in-memory db")
}

/// Minimal model→context→document chain (mirrors `chunk_refs_tests.rs::seed`).
pub(crate) fn seed(db: &Database) -> (i64, i64) {
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
            query_prefix: None,
            document_prefix: None,
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
            image_llm_id: None,
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

/// A legal act whose chunker `section` is one coarse "Abschnitt 2" for every chunk (as seen
/// in the EU acts of the search bench): the continuation follows the articles of the outline.
fn mk_act(db: &Database, ctx: i64, doc: i64, texts: &[(&str, bool)]) -> Vec<i64> {
    texts
        .iter()
        .enumerate()
        .map(|(i, (text, omitted))| {
            db.create_chunk(&NewChunk {
                context_id: ctx,
                document_id: doc,
                chunk_index: i as i64,
                char_start: 0,
                char_end: 1,
                text: (*text).into(),
                signature: None,
                is_omitted: *omitted,
                metadata: serde_json::json!({ "section": "Abschnitt 2" }).to_string(),
            })
            .unwrap()
            .id
        })
        .collect()
}

#[test]
fn continuation_ends_at_the_next_article_of_the_outline() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let ids = mk_act(
        &db,
        ctx,
        doc,
        &[
            ("Artikel 1\nGegenstand\n(1) Diese Verordnung regelt die Aufbewahrung der Unterlagen.", false),
            ("(2) Sie gilt auch für Unterlagen, die vor ihrem Inkrafttreten entstanden sind.\nArtikel 2", false),
            ("Begriffe\n(1) Im Sinne dieser Verordnung bezeichnet der Ausdruck Unterlage jedes Schriftstück.", false),
            ("(2) Eine Kopie steht dem Original gleich, wenn sie beglaubigt ist.", false),
            ("Artikel 3\nInkrafttreten\nDiese Verordnung tritt am zwanzigsten Tag nach ihrer Veröffentlichung in Kraft.", false),
        ],
    );
    let r = db.complete_section_chunks(ids[0], 5).unwrap();
    assert_eq!(chunk_ids(&r), vec![ids[1]], "Artikel 1 ends with the chunk that holds the heading of Artikel 2");
    assert_eq!(r.continues_at, None);
    let r = db.complete_section_chunks(ids[2], 5).unwrap();
    assert_eq!(chunk_ids(&r), vec![ids[3]], "Artikel 2 does not run into Artikel 3");
    let r = db.complete_section_chunks(ids[2], 0).unwrap();
    assert_eq!(r.continues_at, Some(ids[3]));
    let r = db.complete_section_chunks(ids[3], 5).unwrap();
    assert!(r.chunks.is_empty() && r.continues_at.is_none(), "last chunk of Artikel 2: no marker");
}

#[test]
fn outline_continuation_stops_at_an_omitted_chunk_and_outside_every_section() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let ids = mk_act(
        &db,
        ctx,
        doc,
        &[
            ("Erwägungsgrund ohne Überschrift, der vor dem ersten Artikel steht und keinen Pfad hat.", false),
            ("Artikel 1\nGegenstand\n(1) Diese Verordnung regelt die Aufbewahrung der Unterlagen.", false),
            ("(2) Ausgelassener Text des Artikels, etwa eine Kopfzeile der Seite.", true),
            ("(3) Sie gilt auch für Unterlagen, die vor ihrem Inkrafttreten entstanden sind.", false),
            ("Artikel 2\nBegriffe\nIm Sinne dieser Verordnung bezeichnet der Ausdruck Unterlage jedes Schriftstück.", false),
            ("Artikel 3\nInkrafttreten\nDiese Verordnung tritt in Kraft.", false),
        ],
    );
    let r = db.complete_section_chunks(ids[0], 5).unwrap();
    assert!(r.chunks.is_empty() && r.continues_at.is_none(), "the chunker section is not followed in an act with an outline");
    let r = db.complete_section_chunks(ids[1], 5).unwrap();
    assert!(r.chunks.is_empty() && r.continues_at.is_none(), "an omitted chunk ends the run");
}

#[test]
fn chunk_counts_per_document_and_active() {
    let db = db();
    let (ctx, doc) = seed(&db);
    assert_eq!(db.count_active_chunks(ctx).unwrap(), 0);
    assert!(db.count_chunks_per_document(ctx).unwrap().is_empty());
    mk_section_chunk(&db, ctx, doc, 0, None, false);
    mk_section_chunk(&db, ctx, doc, 1, None, false);
    mk_section_chunk(&db, ctx, doc, 2, None, true);
    assert_eq!(db.count_chunks(ctx).unwrap(), 3);
    assert_eq!(db.count_active_chunks(ctx).unwrap(), 2, "omitted chunk not counted");
    assert_eq!(db.count_active_chunks(ctx).unwrap() as usize, db.list_chunks(ctx, false).unwrap().len());
    let per_doc = db.count_chunks_per_document(ctx).unwrap();
    assert_eq!(per_doc.len(), 1);
    assert_eq!(per_doc[&doc], 3, "per-document count includes omitted chunks");
}

#[test]
fn chunks_go_in_at_their_place_and_the_order_is_renumbered() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let a = mk_section_chunk(&db, ctx, doc, 0, None, false);
    let b = mk_section_chunk(&db, ctx, doc, 1, None, false);
    let new = |text: &str, meta: serde_json::Value| NewChunk {
        context_id: ctx,
        document_id: 0, // set by the insert
        chunk_index: 99,
        char_start: 0,
        char_end: 0,
        text: text.into(),
        signature: None,
        is_omitted: false,
        metadata: meta.to_string(),
    };
    let x = new("between", serde_json::json!({ "described_page": 2 }));
    let y = new("declined", serde_json::json!({ "described_page": 5 }));
    let inserted = db
        .insert_chunks_in_order(doc, &[ChunkSlot::Existing(a), ChunkSlot::New(&x), ChunkSlot::Existing(b), ChunkSlot::New(&y)])
        .unwrap();
    assert_eq!(inserted.len(), 2);
    let order: Vec<(String, i64)> = db.list_chunks(ctx, true).unwrap().into_iter().map(|c| (c.text, c.chunk_index)).collect();
    assert_eq!(
        order,
        vec![("chunk 0".into(), 0), ("between".into(), 1), ("chunk 1".into(), 2), ("declined".into(), 3)]
    );
    let spans: Vec<i64> = db.chunk_spans(doc).unwrap().into_iter().map(|s| s.chunk_id).collect();
    assert_eq!(spans, vec![a, inserted[0].id, b, inserted[1].id], "spans in the new order");
    let marks = db.page_description_marks(ctx, None).unwrap();
    assert_eq!(marks.len(), 2);
    assert_eq!((marks[0].document_id, marks[0].page), (doc, 2));
    assert_eq!((marks[1].page, marks[1].chunk_id), (5, inserted[1].id));
    assert_eq!(db.page_description_marks(ctx, Some(doc)).unwrap(), marks);
    assert!(db.page_description_marks(ctx, Some(doc + 1)).unwrap().is_empty());
}

#[test]
fn an_order_that_misses_or_invents_a_chunk_changes_nothing() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let a = mk_section_chunk(&db, ctx, doc, 0, None, false);
    let b = mk_section_chunk(&db, ctx, doc, 1, None, false);
    assert!(db.insert_chunks_in_order(doc, &[ChunkSlot::Existing(a)]).is_err(), "b missing");
    assert!(db.insert_chunks_in_order(doc, &[ChunkSlot::Existing(a), ChunkSlot::Existing(b), ChunkSlot::Existing(b + 100)]).is_err());
    let idx: Vec<i64> = db.list_chunks(ctx, true).unwrap().into_iter().map(|c| c.chunk_index).collect();
    assert_eq!(idx, vec![0, 1]);
}

#[test]
fn next_chunk_skips_omitted_and_ends_with_the_document() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let a = mk_section_chunk(&db, ctx, doc, 0, None, false);
    let _b = mk_section_chunk(&db, ctx, doc, 1, None, true);
    let c = mk_section_chunk(&db, ctx, doc, 2, None, false);
    let next = |id: i64| db.next_chunk(id).unwrap().map(|c| c.id);
    assert_eq!(next(a), Some(c), "omitted chunk is skipped");
    assert_eq!(next(c), None, "end of the document");
    assert_eq!(next(c + 100), None, "unknown id");
}

#[test]
fn document_outline_reads_chunks_in_index_order_without_omitted_and_ocr_headings() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let mk = |idx: i64, text: &str, metadata: serde_json::Value, omitted: bool| {
        db.create_chunk(&NewChunk {
            context_id: ctx,
            document_id: doc,
            chunk_index: idx,
            char_start: 0,
            char_end: 1,
            text: text.into(),
            signature: None,
            is_omitted: omitted,
            metadata: metadata.to_string(),
        })
        .unwrap()
        .id
    };
    let md = serde_json::json!({ "chunk_type": "markdown_section" });
    // Created out of order: the outline follows chunk_index, not the id.
    let usage = mk(3, "## Usage\nUse it.", md.clone(), false);
    let ocr = mk(2, "## Text on page 2 (OCR)\nScanned", serde_json::json!({ "chunk_type": "markdown_section", "ocr": true }), false);
    let setup = mk(1, "## Setup\nRun it.", md.clone(), false);
    let _gone = mk(0, "# Omitted\nNot read.", md.clone(), true);
    let o = db.document_outline(doc).unwrap();
    assert_eq!(o.path(setup).as_deref(), Some("Setup"));
    assert_eq!(o.path(ocr).as_deref(), Some("Setup"), "the OCR section heading is no heading");
    assert_eq!(o.path(usage).as_deref(), Some("Usage"));
    assert_eq!(o.section(setup).unwrap().chunk_ids, vec![setup, ocr]);
}
