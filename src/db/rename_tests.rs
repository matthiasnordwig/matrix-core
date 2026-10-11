//! Tests for `rename_document` — name change plus `metadata.document_title`
//! of the document's chunks (repair path for a wrongly detected ingest title).

use crate::db::models::*;
use crate::db::Database;

fn seed(db: &Database, doc_name: &str) -> (i64, i64) {
    let ctx = db
        .create_context(&NewContext {
            name: "Ctx".into(),
            description: None,
            chunking_profile_id: None,
            embedding_model_id: None,
            embedding_dim: None,
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
            name: doc_name.into(),
            zip_entry: None,
            byte_size: None,
            page_count: None,
            content_hash: None,
            extracted_text: None,
        })
        .unwrap();
    (ctx.id, doc.id)
}

fn chunk(db: &Database, ctx: i64, doc: i64, idx: i64, metadata: &str) -> i64 {
    db.create_chunk(&NewChunk {
        context_id: ctx,
        document_id: doc,
        chunk_index: idx,
        char_start: 0,
        char_end: 1,
        text: format!("chunk {idx}"),
        signature: None,
        is_omitted: false,
        metadata: metadata.into(),
    })
    .unwrap()
    .id
}

fn title_of(db: &Database, chunk_id: i64) -> Option<String> {
    let c = db.chunk(chunk_id).unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&c.metadata).unwrap();
    v.get("document_title").and_then(|t| t.as_str()).map(str::to_string)
}

#[test]
fn rename_updates_document_and_chunk_titles_only_for_that_document() {
    let db = Database::open_in_memory().unwrap();
    let echo = "Titel des Dokuments: Regel 1: Antworte AUSSCHLIESSLICH mit dem Titel. Keine Erklärungen. Dokument: Geschäftsbericht des Uniper-Konzerns";
    let (ctx, doc) = seed(&db, echo);
    let other = db
        .create_document(&NewDocument {
            context_id: ctx,
            name: "Other".into(),
            zip_entry: None,
            byte_size: None,
            page_count: None,
            content_hash: None,
            extracted_text: None,
        })
        .unwrap()
        .id;
    let meta = serde_json::json!({"document_title": echo, "section": "§ 1"}).to_string();
    let a = chunk(&db, ctx, doc, 0, &meta);
    let b = chunk(&db, ctx, doc, 1, "{}");
    let o = chunk(&db, ctx, other, 0, r#"{"document_title":"Other"}"#);

    assert!(db.rename_document(doc, "  Geschäftsbericht Uniper  ").unwrap());
    assert_eq!(db.document(doc).unwrap().unwrap().name, "Geschäftsbericht Uniper");
    assert_eq!(title_of(&db, a).as_deref(), Some("Geschäftsbericht Uniper"));
    assert_eq!(title_of(&db, b).as_deref(), Some("Geschäftsbericht Uniper"));
    assert_eq!(title_of(&db, o).as_deref(), Some("Other"));
    // Other metadata keys survive.
    let kept: serde_json::Value = serde_json::from_str(&db.chunk(a).unwrap().unwrap().metadata).unwrap();
    assert_eq!(kept["section"], "§ 1");
}

#[test]
fn rename_rejects_blank_name_and_unknown_id() {
    let db = Database::open_in_memory().unwrap();
    let (ctx, doc) = seed(&db, "Old");
    let c = chunk(&db, ctx, doc, 0, r#"{"document_title":"Old"}"#);
    assert!(!db.rename_document(doc, "   ").unwrap());
    assert!(!db.rename_document(9999, "X").unwrap());
    assert_eq!(db.document(doc).unwrap().unwrap().name, "Old");
    assert_eq!(title_of(&db, c).as_deref(), Some("Old"));
}

#[test]
fn rename_tolerates_non_object_chunk_metadata() {
    let db = Database::open_in_memory().unwrap();
    let (ctx, doc) = seed(&db, "Old");
    let c = chunk(&db, ctx, doc, 0, "not json");
    assert!(db.rename_document(doc, "New").unwrap());
    assert_eq!(db.chunk(c).unwrap().unwrap().metadata, "not json");
}
