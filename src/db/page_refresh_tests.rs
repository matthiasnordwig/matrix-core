//! Tests for `page_refresh.rs`: which PDF documents count as read before page
//! changes were recorded (rule pure, then applied to a collection).

use super::page_refresh::{is_outdated_pdf, PdfFacts};
use crate::db::models::*;
use crate::db::Database;

fn facts(file_name: &str) -> PdfFacts<'_> {
    PdfFacts { file_name, chunks: 4, has_page_breaks: false, structural: true }
}

#[test]
fn rule_needs_a_chunked_pdf_without_the_page_breaks_key_in_a_structural_collection() {
    assert!(is_outdated_pdf(&facts("Bericht.PDF")));
    assert!(!is_outdated_pdf(&PdfFacts { has_page_breaks: true, ..facts("a.pdf") }), "read with page changes = never again");
    assert!(!is_outdated_pdf(&PdfFacts { chunks: 0, ..facts("a.pdf") }), "no chunks, nothing to gain");
    assert!(!is_outdated_pdf(&PdfFacts { structural: false, ..facts("a.pdf") }), "LLM chunks never get page changes");
    assert!(!is_outdated_pdf(&facts("notes.docx")));
}

fn document(db: &Database, ctx: i64, name: &str, zip_entry: Option<&str>) -> i64 {
    db.create_document(&NewDocument {
        context_id: ctx,
        name: name.into(),
        zip_entry: zip_entry.map(Into::into),
        byte_size: None,
        page_count: Some(3),
        content_hash: None,
        extracted_text: Some(format!("text of {name}")),
    })
    .unwrap()
    .id
}

fn chunk(db: &Database, ctx: i64, doc: i64, index: i64, metadata: &str) {
    db.create_chunk(&NewChunk {
        context_id: ctx,
        document_id: doc,
        chunk_index: index,
        char_start: 0,
        char_end: 4,
        text: "text".into(),
        signature: None,
        is_omitted: false,
        metadata: metadata.into(),
    })
    .unwrap();
}

#[test]
fn collection_lists_only_its_outdated_pdfs() {
    let db = Database::open_in_memory().unwrap();
    let ctx = db.create_context(&NewContext { name: "s".into(), chunking_strategy: "structural".into(), ..Default::default() }).unwrap().id;
    let llm = db.create_context(&NewContext { name: "p".into(), chunking_strategy: "prompt".into(), ..Default::default() }).unwrap().id;

    // Read by an older Matrix (also after 2026-10-23): pages, no key.
    let a = document(&db, ctx, "Titel aus dem PDF", Some("bericht.pdf"));
    chunk(&db, ctx, a, 0, r#"{"page":1}"#);
    chunk(&db, ctx, a, 1, r#"{"page":2}"#);
    // Read now: every chunk has the key, one page or more.
    let over = document(&db, ctx, "neu.pdf", None);
    chunk(&db, ctx, over, 0, r#"{"page":1,"page_breaks":[[40,2]]}"#);
    let slides = document(&db, ctx, "folien.pdf", None);
    chunk(&db, ctx, slides, 0, r#"{"page":1,"page_breaks":[]}"#);
    chunk(&db, ctx, slides, 1, r#"{"page":2,"page_breaks":[]}"#);
    let note = document(&db, ctx, "notiz.md", None);
    chunk(&db, ctx, note, 0, r#"{"section":"x"}"#);
    document(&db, ctx, "leer.pdf", None);
    let in_llm = document(&db, llm, "bericht.pdf", None);
    chunk(&db, llm, in_llm, 0, r#"{"page":1}"#);

    assert_eq!(db.outdated_pdf_documents(ctx).unwrap(), vec![(a, "bericht.pdf".to_string())]);
    assert!(db.outdated_pdf_documents(llm).unwrap().is_empty());
}
