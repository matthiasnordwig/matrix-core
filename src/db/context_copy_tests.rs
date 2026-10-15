//! Tests for `copy_context_chunks` — verbatim document/chunk/page-image duplication into
//! an empty target context (same texts/metadata, new ids, FTS indexed), with a
//! guard against copying into a non-empty or the same context.

use crate::db::models::*;
use crate::db::{CoreError, Database};

fn ctx(db: &Database, name: &str) -> i64 {
    db.create_context(&NewContext {
        name: name.into(),
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
        image_llm_id: None,
        chunking_strategy: "Semantic".into(),
        structural_profile_id: None,
    })
    .unwrap()
    .id
}

fn doc(db: &Database, ctx: i64, name: &str) -> i64 {
    db.create_document(&NewDocument {
        context_id: ctx,
        name: name.into(),
        zip_entry: None,
        byte_size: Some(42),
        page_count: Some(3),
        content_hash: Some("h".into()),
        extracted_text: Some("full text".into()),
    })
    .unwrap()
    .id
}

fn chunk(db: &Database, ctx: i64, doc: i64, idx: i64, text: &str, omitted: bool) {
    db.create_chunk(&NewChunk {
        context_id: ctx,
        document_id: doc,
        chunk_index: idx,
        char_start: idx * 10,
        char_end: idx * 10 + 9,
        text: text.into(),
        signature: Some(format!("sig{idx}")),
        is_omitted: omitted,
        metadata: format!(r#"{{"document_title":"T{idx}"}}"#),
    })
    .unwrap();
}

fn seeded() -> (Database, i64) {
    let db = Database::open_in_memory().unwrap();
    let src = ctx(&db, "Src");
    let d1 = doc(&db, src, "a.pdf");
    let d2 = doc(&db, src, "b.pdf");
    chunk(&db, src, d1, 0, "Rechnungen sind fällig", false);
    chunk(&db, src, d1, 1, "Kündigungsfrist drei Monate", false);
    chunk(&db, src, d2, 0, "ausgelassen", true);
    db.insert_page_image(&NewPageImage {
        context_id: src, document_id: d1, page: 2, char_start: Some(0), char_end: Some(10),
        width: 8, height: 6, image: vec![0xFF, 0xD8, 1, 2],
    })
    .unwrap();
    (db, src)
}

#[test]
fn copies_documents_and_chunks_verbatim() {
    let (db, src) = seeded();
    let dst = ctx(&db, "Src [copy]");
    assert_eq!(db.copy_context_chunks(src, dst).unwrap(), 3);

    let src_docs = db.list_documents(src).unwrap();
    let dst_docs = db.list_documents(dst).unwrap();
    assert_eq!(dst_docs.len(), 2);
    for (s, d) in src_docs.iter().zip(dst_docs.iter()) {
        assert_ne!(s.id, d.id);
        assert_eq!((&s.name, s.byte_size, s.page_count), (&d.name, d.byte_size, d.page_count));
        assert_eq!(s.extracted_text, d.extracted_text);
    }

    let key = |c: &Chunk| (c.chunk_index, c.char_start, c.char_end, c.text.clone(), c.signature.clone(), c.is_omitted, c.metadata.clone());
    let s: Vec<_> = db.list_chunks(src, true).unwrap().iter().map(key).collect();
    let d_chunks = db.list_chunks(dst, true).unwrap();
    let d: Vec<_> = d_chunks.iter().map(key).collect();
    assert_eq!(s, d);
    // Chunks point at the COPIED documents, never the source ones.
    let dst_doc_ids: Vec<i64> = dst_docs.iter().map(|x| x.id).collect();
    assert!(d_chunks.iter().all(|c| dst_doc_ids.contains(&c.document_id) && c.context_id == dst));
    // Source is untouched.
    assert_eq!(db.list_chunks(src, true).unwrap().len(), 3);
    // Page images follow their (copied) document, bytes included.
    let imgs = db.list_page_images_for_context(dst).unwrap();
    assert_eq!(imgs.len(), 1);
    assert!(dst_doc_ids.contains(&imgs[0].document_id));
    assert_eq!((imgs[0].page, imgs[0].char_end), (2, Some(10)));
    assert_eq!(db.page_image_bytes(imgs[0].id).unwrap(), Some(vec![0xFF, 0xD8, 1, 2]));
    // FTS triggers indexed the copies.
    assert_eq!(db.keyword_search_context(dst, "Kündigungsfrist", 5, None).unwrap().len(), 1);
}

#[test]
fn refuses_non_empty_or_same_target() {
    let (db, src) = seeded();
    let dst = ctx(&db, "Dst");
    db.copy_context_chunks(src, dst).unwrap();
    assert!(matches!(db.copy_context_chunks(src, dst), Err(CoreError::InvalidState(_))));
    assert!(matches!(db.copy_context_chunks(src, src), Err(CoreError::InvalidState(_))));
    // No duplicate rows from the refused second copy.
    assert_eq!(db.list_chunks(dst, true).unwrap().len(), 3);
}
