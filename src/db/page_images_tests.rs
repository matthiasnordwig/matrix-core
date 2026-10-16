//! `page_images.rs` CRUD: replace-on-insert, listing without bytes, vector
//! upsert per model, "needs embedding" after a model switch, counts, the
//! model-filtered vector scan, chunk spans and cascades.

use crate::db::models::*;
use crate::Database;

fn model(db: &Database, ident: &str) -> i64 {
    db.create_embedding_model(&NewEmbeddingModel {
        identifier: ident.into(),
        kind: ModelKind::LocalOnnx,
        model_path: None,
        tokenizer_path: None,
        api_config: None,
        execution_provider: None,
        is_matryoshka: false,
        native_dim: 3,
        default_dim: 3,
        normalize: true,
        tpm_limit: None,
        rpm_limit: None,
        max_concurrency: 1,
        query_prefix: None,
        document_prefix: None,
    })
    .unwrap()
    .id
}

/// (context_id, document_id) bound to `model_id`.
fn seed(db: &Database, model_id: i64) -> (i64, i64) {
    let n = db.list_contexts().unwrap().len();
    let ctx = db
        .create_context(&NewContext {
            name: format!("Bilder {n}"),
            description: None,
            chunking_strategy: "Semantic".into(),
            chunking_profile_id: None,
            structural_profile_id: None,
            embedding_model_id: Some(model_id),
            embedding_dim: Some(3),
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
        })
        .unwrap();
    let doc = db
        .create_document(&NewDocument {
            context_id: ctx.id,
            name: "r.pdf".into(),
            zip_entry: None,
            byte_size: None,
            page_count: Some(3),
            content_hash: None,
            extracted_text: Some("x".repeat(30)),
        })
        .unwrap();
    (ctx.id, doc.id)
}

fn img(ctx: i64, doc: i64, page: i64, bytes: &[u8]) -> NewPageImage {
    NewPageImage {
        context_id: ctx,
        document_id: doc,
        page,
        char_start: Some((page - 1) * 10),
        char_end: Some(page * 10),
        width: 80,
        height: 100,
        image: bytes.to_vec(),
    }
}

#[test]
fn insert_replaces_same_page_and_drops_its_vector() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "m");
    let (ctx, doc) = seed(&db, m);
    let a = db.insert_page_image(&img(ctx, doc, 2, b"old")).unwrap();
    db.upsert_page_image_embedding(a.id, m, &[1.0, 0.0, 0.0]).unwrap();
    assert_eq!(db.page_image_counts(ctx, m).unwrap(), (1, 1));

    let b = db.insert_page_image(&img(ctx, doc, 2, b"new")).unwrap();
    assert_eq!(db.list_page_images_for_document(doc).unwrap().len(), 1);
    assert_eq!(db.page_image_bytes(b.id).unwrap().unwrap(), b"new");
    assert_eq!(db.page_image_counts(ctx, m).unwrap(), (1, 0), "old vector cascaded away");
    assert_eq!((b.page, b.char_start, b.char_end, b.width, b.height), (2, Some(10), Some(20), 80, 100));
}

#[test]
fn listing_is_ordered_and_scoped() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "m");
    let (ctx, doc) = seed(&db, m);
    let (ctx2, doc2) = seed(&db, m);
    db.insert_page_image(&img(ctx, doc, 3, b"c")).unwrap();
    db.insert_page_image(&img(ctx, doc, 1, b"a")).unwrap();
    db.insert_page_image(&img(ctx2, doc2, 1, b"z")).unwrap();
    let pages: Vec<i64> = db.list_page_images_for_document(doc).unwrap().iter().map(|p| p.page).collect();
    assert_eq!(pages, vec![1, 3]);
    assert_eq!(db.list_page_images_for_context(ctx).unwrap().len(), 2);
    assert_eq!(db.list_page_images_for_context(ctx2).unwrap().len(), 1);
}

#[test]
fn needing_embedding_follows_the_context_model() {
    let db = Database::open_in_memory().unwrap();
    let m1 = model(&db, "m1");
    let m2 = model(&db, "m2");
    let (ctx, doc) = seed(&db, m1);
    let p1 = db.insert_page_image(&img(ctx, doc, 1, b"a")).unwrap();
    let p2 = db.insert_page_image(&img(ctx, doc, 2, b"b")).unwrap();
    assert_eq!(db.page_images_needing_embedding(ctx).unwrap().len(), 2);
    db.upsert_page_image_embedding(p1.id, m1, &[1.0, 0.0, 0.0]).unwrap();
    let ids: Vec<i64> = db.page_images_needing_embedding(ctx).unwrap().iter().map(|p| p.id).collect();
    assert_eq!(ids, vec![p2.id]);

    // Model switch: a vector of m1 counts as missing for m2.
    db.conn.execute("UPDATE contexts SET embedding_model_id = ?1 WHERE id = ?2", [m2, ctx]).unwrap();
    assert_eq!(db.page_images_needing_embedding(ctx).unwrap().len(), 2);
    assert_eq!(db.page_image_counts(ctx, m2).unwrap(), (2, 0));
    assert_eq!(db.page_image_counts(ctx, m1).unwrap(), (2, 1));

    assert_eq!(db.delete_page_image_embeddings_for_context(ctx).unwrap(), 1);
    assert_eq!(db.page_image_counts(ctx, m1).unwrap(), (2, 0));
}

#[test]
fn upsert_unknown_page_image_is_not_found() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "m");
    assert!(db.upsert_page_image_embedding(999, m, &[1.0]).is_err());
}

#[test]
fn scan_returns_only_vectors_of_the_requested_model() {
    let db = Database::open_in_memory().unwrap();
    let m1 = model(&db, "m1");
    let m2 = model(&db, "m2");
    let (ctx, doc) = seed(&db, m1);
    let p1 = db.insert_page_image(&img(ctx, doc, 1, b"a")).unwrap();
    let p2 = db.insert_page_image(&img(ctx, doc, 2, b"b")).unwrap();
    db.upsert_page_image_embedding(p1.id, m1, &[1.0, 0.0, 0.0]).unwrap();
    db.upsert_page_image_embedding(p2.id, m2, &[0.0, 1.0]).unwrap();
    let s1 = db.scan_page_image_vectors(ctx, m1).unwrap();
    assert_eq!(s1.len(), 1);
    assert_eq!((s1[0].page_image_id, s1[0].document_id, s1[0].page), (p1.id, doc, 1));
    assert_eq!((s1[0].char_start, s1[0].char_end), (Some(0), Some(10)));
    assert_eq!(s1[0].vector, vec![1.0, 0.0, 0.0]);
    let s2 = db.scan_page_image_vectors(ctx, m2).unwrap();
    assert_eq!(s2.len(), 1);
    assert_eq!(s2[0].vector.len(), 2);
}

#[test]
fn chunk_spans_read_page_and_only_matching_vectors() {
    let db = Database::open_in_memory().unwrap();
    let m1 = model(&db, "m1");
    let m2 = model(&db, "m2");
    let (ctx, doc) = seed(&db, m1);
    let mk = |i: i64, meta: &str, omitted: bool| {
        db.create_chunk(&NewChunk {
            context_id: ctx,
            document_id: doc,
            chunk_index: i,
            char_start: i * 10,
            char_end: i * 10 + 10,
            text: format!("c{i}"),
            signature: None,
            is_omitted: omitted,
            metadata: meta.into(),
        })
        .unwrap()
        .id
    };
    // Created out of index order on purpose.
    let c1 = mk(1, "not json", false);
    let c0 = mk(0, r#"{"page": 4}"#, false);
    let c2 = mk(2, "{}", true);
    let emb = |chunk_id: i64, model_id: i64, v: Vec<f32>| {
        db.insert_embedding(&NewEmbedding { chunk_id, context_id: ctx, document_id: doc, embedding_model_id: model_id, dim: v.len() as i64, vector: v })
            .unwrap()
    };
    emb(c0, m1, vec![1.0, 0.0, 0.0]);
    emb(c1, m2, vec![1.0, 0.0, 0.0]);
    emb(c2, m1, vec![0.0, 1.0]);
    let rows = db.chunk_spans_with_vectors(doc, m1, 3).unwrap();
    let ids: Vec<i64> = rows.iter().map(|r| r.0.chunk_id).collect();
    assert_eq!(ids, vec![c0, c1, c2]);
    assert_eq!(rows[0].0.page, Some(4));
    assert_eq!(rows[1].0.page, None, "invalid metadata is no page");
    assert!(rows[0].2.is_some());
    assert!(rows[1].2.is_none(), "vector of another model is not returned");
    assert!(rows[2].2.is_none(), "vector of another dimension is not returned");
    assert!(rows[2].1, "omitted flag is carried");
}

#[test]
fn deleting_the_document_or_model_cascades() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "m");
    let (ctx, doc) = seed(&db, m);
    let p = db.insert_page_image(&img(ctx, doc, 1, b"a")).unwrap();
    db.upsert_page_image_embedding(p.id, m, &[1.0, 0.0, 0.0]).unwrap();
    db.delete_embedding_model(m).unwrap();
    assert_eq!(db.list_page_images_for_context(ctx).unwrap().len(), 1);
    assert_eq!(db.scan_page_image_vectors(ctx, m).unwrap().len(), 0);
    db.delete_document(doc).unwrap();
    assert!(db.list_page_images_for_context(ctx).unwrap().is_empty());
}

#[test]
fn model_vision_path_and_context_flag_round_trip() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "m");
    assert_eq!(db.embedding_model(m).unwrap().unwrap().vision_model_path, None);
    db.set_embedding_model_vision_path(m, Some("/m/vision_encoder_q4.onnx")).unwrap();
    assert_eq!(
        db.embedding_model(m).unwrap().unwrap().vision_model_path.as_deref(),
        Some("/m/vision_encoder_q4.onnx")
    );
    let (ctx, _) = seed(&db, m);
    assert!(!db.context(ctx).unwrap().unwrap().embed_images, "default off");
    db.set_context_embed_images(ctx, true).unwrap();
    assert!(db.context(ctx).unwrap().unwrap().embed_images);
}

fn chunk(ctx: i64, doc: i64, i: i64, text: &str, start: i64) -> NewChunk {
    NewChunk {
        context_id: ctx,
        document_id: doc,
        chunk_index: i,
        char_start: start,
        char_end: start + text.len() as i64,
        text: text.into(),
        signature: None,
        is_omitted: false,
        metadata: "{}".into(),
    }
}

#[test]
fn replacing_image_document_text_swaps_chunks_and_keeps_the_picture() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "m");
    let (ctx, doc) = seed(&db, m);
    let p = db.insert_page_image(&img(ctx, doc, 1, b"jpeg")).unwrap();
    db.upsert_page_image_embedding(p.id, m, &[1.0, 0.0, 0.0]).unwrap();
    db.create_chunk(&chunk(ctx, doc, 0, "old", 0)).unwrap();

    let text = "# a.png\n\n## Generated image description\n\nA cat.\n";
    let n = db
        .replace_image_document_text(doc, text, &[chunk(ctx, doc, 0, "# a.png", 0), chunk(ctx, doc, 1, "A cat.", 40)])
        .unwrap();
    assert_eq!(n, 2);
    let d = db.document(doc).unwrap().unwrap();
    assert_eq!((d.extracted_text.as_deref(), d.byte_size), (Some(text), Some(text.len() as i64)));
    let texts: Vec<String> = db.list_chunks(ctx, true).unwrap().into_iter().map(|c| c.text).collect();
    assert_eq!(texts, vec!["# a.png".to_string(), "A cat.".to_string()]);
    let page = db.page_image(p.id).unwrap().expect("same picture row");
    assert_eq!((page.char_start, page.char_end), (Some(0), Some(text.len() as i64)));
    assert_eq!(db.scan_page_image_vectors(ctx, m).unwrap().len(), 1, "image vector kept");
    assert!(db.replace_image_document_text(9999, "x", &[]).is_err(), "unknown document");
}

#[test]
fn declined_image_descriptions_are_remembered_on_the_chunks() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "m");
    let (ctx, doc) = seed(&db, m);
    db.create_chunk(&chunk(ctx, doc, 0, "# a.png", 0)).unwrap();
    let mut broken = chunk(ctx, doc, 1, "Image: a.png", 9);
    broken.metadata = "not json".into();
    db.create_chunk(&broken).unwrap();
    assert!(db.image_description_declined_documents(ctx).unwrap().is_empty());

    assert_eq!(db.mark_image_description_declined(doc, "declined by m").unwrap(), 2);
    assert_eq!(db.image_description_declined_documents(ctx).unwrap(), vec![doc]);
    let texts: Vec<String> = db.list_chunks(ctx, true).unwrap().into_iter().map(|c| c.text).collect();
    assert_eq!(texts, vec!["# a.png".to_string(), "Image: a.png".to_string()], "text untouched");

    // A stored description replaces the chunks, and the mark goes with them.
    db.replace_image_document_text(doc, "# a.png\n", &[chunk(ctx, doc, 0, "# a.png", 0)]).unwrap();
    assert!(db.image_description_declined_documents(ctx).unwrap().is_empty());
}

#[test]
fn describe_state_lives_on_the_page_and_resets_with_a_new_render() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "m-state");
    let (ctx, doc) = seed(&db, m);
    let p = db.insert_page_image(&img(ctx, doc, 2, b"a")).unwrap();
    assert_eq!((p.description_state.as_deref(), p.description_model.as_deref()), (None, None));
    db.set_page_description_state(p.id, "declined", "vlm").unwrap();
    let got = db.page_image(p.id).unwrap().unwrap();
    assert_eq!((got.description_state.as_deref(), got.description_model.as_deref()), (Some("declined"), Some("vlm")));
    assert_eq!(db.list_page_images_for_context(ctx).unwrap()[0].description_state.as_deref(), Some("declined"));
    // Chunk churn does not touch it.
    db.delete_chunks_for_document(doc).unwrap();
    assert_eq!(db.page_image(p.id).unwrap().unwrap().description_state.as_deref(), Some("declined"));
    assert!(db.set_page_description_state(p.id, "maybe", "vlm").is_err(), "CHECK constraint");
    assert!(db.set_page_description_state(p.id + 99, "described", "vlm").is_err());
    let again = db.insert_page_image(&img(ctx, doc, 2, b"b")).unwrap();
    assert_eq!(again.description_state, None, "a new render starts fresh");
}

#[test]
fn renumbering_chunks_keeps_keyword_search_and_text_updates_reindex() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "m-fts");
    let (ctx, doc) = seed(&db, m);
    let a = db.create_chunk(&chunk(ctx, doc, 0, "alpha zebra", 0)).unwrap();
    let new = chunk(ctx, doc, 0, "gamma", 0);
    db.insert_chunks_in_order(doc, &[super::super::chunks::ChunkSlot::New(&new), super::super::chunks::ChunkSlot::Existing(a.id)]).unwrap();
    assert_eq!(db.chunk(a.id).unwrap().unwrap().chunk_index, 1);
    assert_eq!(db.keyword_search_context(ctx, "zebra", 5, None).unwrap().len(), 1);
    db.update_chunk_text(a.id, "alpha yak").unwrap();
    assert!(db.keyword_search_context(ctx, "zebra", 5, None).unwrap().is_empty());
    assert_eq!(db.keyword_search_context(ctx, "yak", 5, None).unwrap().len(), 1);
    let sql: String = db
        .conn
        .query_row("SELECT sql FROM sqlite_master WHERE name = 'chunks_fts_au'", [], |r| r.get(0))
        .unwrap();
    assert!(sql.contains("UPDATE OF text"), "{sql}");
}
