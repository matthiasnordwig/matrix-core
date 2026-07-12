//! FTS5 keyword-index tests (schema_v48): availability smoke test,
//! trigger-sync roundtrip, and `keyword_search_context` behaviour.

use super::models::*;
use super::Database;
use rusqlite::Connection;
use std::sync::atomic::{AtomicI64, Ordering};

static MODEL_SEQ: AtomicI64 = AtomicI64::new(0);

/// FIRST STEP smoke test: prove the bundled rusqlite has FTS5 compiled in.
/// If this fails, AP1 must STOP and escalate (no LIKE fallback).
#[test]
fn fts5_is_available() {
    let conn = Connection::open_in_memory().expect("open in-memory");
    conn.execute_batch("CREATE VIRTUAL TABLE t USING fts5(body);")
        .expect("FTS5 must be available in the bundled rusqlite (escalate if not)");
    conn.execute("INSERT INTO t(body) VALUES ('hello world')", [])
        .unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM t WHERE t MATCH 'hello'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

/// Build model -> context -> document and return (context_id, document_id).
fn seed_ctx(db: &Database) -> (i64, i64) {
    let seq = MODEL_SEQ.fetch_add(1, Ordering::Relaxed);
    let m = db
        .create_embedding_model(&NewEmbeddingModel {
            identifier: format!("fts-embed-{seq}"),
            kind: ModelKind::LocalOnnx,
            model_path: Some("/models/x.onnx".into()),
            tokenizer_path: None,
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
            name: format!("FTS-{seq}"),
            description: None,
            chunking_profile_id: None,
            embedding_model_id: Some(m.id),
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

fn add_doc(db: &Database, ctx: i64, name: &str) -> i64 {
    db.create_document(&NewDocument {
        context_id: ctx,
        name: name.into(),
        zip_entry: None,
        byte_size: None,
        page_count: None,
        content_hash: None,
        extracted_text: None,
    })
    .unwrap()
    .id
}

fn add_chunk(db: &Database, ctx: i64, doc: i64, idx: i64, text: &str) -> i64 {
    db.create_chunk(&NewChunk {
        context_id: ctx,
        document_id: doc,
        chunk_index: idx,
        char_start: 0,
        char_end: text.len() as i64,
        text: text.into(),
        signature: None,
        is_omitted: false,
        metadata: "{}".into(),
    })
    .unwrap()
    .id
}

/// INSERT / UPDATE / DELETE triggers keep `chunks_fts` in sync with `chunks`.
#[test]
fn fts_triggers_keep_index_in_sync() {
    let db = Database::open_in_memory().unwrap();
    let (ctx, doc) = seed_ctx(&db);

    // INSERT -> searchable.
    let c1 = add_chunk(&db, ctx, doc, 0, "the quick brown fox");
    let hits = db.keyword_search_context(ctx, "quick", 10, None).unwrap();
    assert_eq!(hits.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![c1]);

    // UPDATE -> old term gone, new term present.
    db.update_chunk_text(c1, "a lazy sleeping dog").unwrap();
    assert!(db.keyword_search_context(ctx, "quick", 10, None).unwrap().is_empty());
    let hits = db.keyword_search_context(ctx, "lazy", 10, None).unwrap();
    assert_eq!(hits.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![c1]);

    // DELETE -> gone.
    db.delete_chunk(c1).unwrap();
    assert!(db.keyword_search_context(ctx, "lazy", 10, None).unwrap().is_empty());
}

/// Context isolation + rank ordering + robust escaping of identifier-like
/// queries (`§`, dots, spaces) that would otherwise break FTS5 syntax.
#[test]
fn keyword_search_context_scoping_and_escaping() {
    let db = Database::open_in_memory().unwrap();
    let (ctx_a, doc_a) = seed_ctx(&db);
    let (ctx_b, doc_b) = seed_ctx(&db);

    let a1 = add_chunk(&db, ctx_a, doc_a, 0, "Anforderung nach AT 4.3.2 der MaRisk");
    let _a2 = add_chunk(&db, ctx_a, doc_a, 1, "unrelated risk content");
    let _b1 = add_chunk(&db, ctx_b, doc_b, 0, "AT 4.3.2 appears in the other context");

    // Identifier with dots/spaces must not raise an FTS5 syntax error and must
    // stay scoped to ctx_a.
    let hits = db.keyword_search_context(ctx_a, "AT 4.3.2", 10, None).unwrap();
    let ids: Vec<i64> = hits.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, vec![a1], "expected only ctx_a hit, got {ids:?}");

    // Ranks are 1-based and contiguous from the best hit.
    assert_eq!(hits[0].1, 1);

    // `§`, quotes and periods must be handled without error.
    for q in ["§ 25a KWG", "Art. 28", "a \"weird\" query", "()MATCH*"] {
        db.keyword_search_context(ctx_a, q, 10, None).unwrap();
    }
    // Empty / whitespace-only query is a well-defined empty result.
    assert!(db.keyword_search_context(ctx_a, "   ", 10, None).unwrap().is_empty());
    assert!(db.keyword_search_context(ctx_a, "", 10, None).unwrap().is_empty());
}

/// Omitted chunks are indexed by the FTS triggers but must NOT surface in
/// keyword search — they are never embedded (vector search can't return them),
/// so the FTS leg of hybrid retrieval must exclude them too, or omitted chunks
/// leak into chat/grid sources. Regression guard for the AP5-review finding.
#[test]
fn keyword_search_excludes_omitted_chunks() {
    let db = Database::open_in_memory().unwrap();
    let (ctx, doc) = seed_ctx(&db);

    let kept = add_chunk(&db, ctx, doc, 0, "Anforderungen an die Auslagerung nach AT 9");
    let omitted = db
        .create_chunk(&NewChunk {
            context_id: ctx,
            document_id: doc,
            chunk_index: 1,
            char_start: 0,
            char_end: 0,
            text: "Auslagerung Auslagerung (omitted footer)".into(),
            signature: None,
            is_omitted: true,
            metadata: "{}".into(),
        })
        .unwrap()
        .id;

    let hits = db.keyword_search_context(ctx, "Auslagerung", 10, None).unwrap();
    let ids: Vec<i64> = hits.iter().map(|(id, _)| *id).collect();
    assert!(ids.contains(&kept), "non-omitted chunk must be found");
    assert!(!ids.contains(&omitted), "omitted chunk must NOT surface in FTS results");
}

/// AP8 file-level scope: `keyword_search_context` with `doc_ids = Some(&[…])`
/// must return only chunks of the named documents — the FTS leg of the doc_ids
/// filter (leak-regression: the vector leg has its own guard, both must match).
#[test]
fn keyword_search_respects_doc_scope() {
    let db = Database::open_in_memory().unwrap();
    let (ctx, doc_a) = seed_ctx(&db);
    let doc_b = add_doc(&db, ctx, "b.pdf");

    let a1 = add_chunk(&db, ctx, doc_a, 0, "Auslagerung nach AT 9 (file A)");
    let b1 = add_chunk(&db, ctx, doc_b, 0, "Auslagerung nach AT 9 (file B)");

    // No scope: both files match.
    let all: Vec<i64> = db
        .keyword_search_context(ctx, "Auslagerung", 10, None)
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert!(all.contains(&a1) && all.contains(&b1));

    // Scope to doc_a only: b1 must NOT surface.
    let scoped: Vec<i64> = db
        .keyword_search_context(ctx, "Auslagerung", 10, Some(&[doc_a]))
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert!(scoped.contains(&a1), "in-scope chunk must be found");
    assert!(!scoped.contains(&b1), "out-of-scope chunk must NOT leak");

    // Empty scope = no documents in scope → empty result (never "all").
    assert!(db
        .keyword_search_context(ctx, "Auslagerung", 10, Some(&[]))
        .unwrap()
        .is_empty());
}

/// AP8 `phrase_search_context`: exact-phrase (adjacent, in order) FTS, unlike
/// the OR-fused keyword path. Terms out of order must NOT match; the same
/// omitted-exclusion and doc-scope guards apply.
#[test]
fn phrase_search_is_exact_and_scoped() {
    use super::fts::escape_fts_phrase;
    // Pure escaping: whole query becomes one quoted phrase (not OR-joined).
    assert_eq!(escape_fts_phrase("Art. 33").unwrap(), "\"Art. 33\"");
    assert_eq!(escape_fts_phrase("  Art.\t33 ").unwrap(), "\"Art. 33\"");
    assert!(escape_fts_phrase("§ — ()").is_none());

    let db = Database::open_in_memory().unwrap();
    let (ctx, doc_a) = seed_ctx(&db);
    let doc_b = add_doc(&db, ctx, "b.pdf");

    let hit = add_chunk(&db, ctx, doc_a, 0, "Meldepflicht nach Art. 33 DSGVO binnen 72 Stunden");
    // Same terms, but not adjacent/in order → phrase must NOT match.
    let _reorder = add_chunk(&db, ctx, doc_a, 1, "33 verschiedene Artikel");
    let other_doc = add_chunk(&db, ctx, doc_b, 0, "Art. 33 in file B");

    let ids: Vec<i64> = db
        .phrase_search_context(ctx, "Art. 33", 10, None)
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert!(ids.contains(&hit), "exact adjacent phrase must be found");
    assert!(ids.contains(&other_doc), "phrase in file B matches without scope");

    // Doc scope restricts to file A.
    let scoped: Vec<i64> = db
        .phrase_search_context(ctx, "Art. 33", 10, Some(&[doc_a]))
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert!(scoped.contains(&hit));
    assert!(!scoped.contains(&other_doc), "out-of-scope phrase hit must NOT leak");
}
