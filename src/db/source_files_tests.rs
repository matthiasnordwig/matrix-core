//! Tests for `source_files.rs` (schema_v71): roots widen only, a gone file is only
//! marked, a moved file keeps its document, pending rows stay out of the list,
//! every call stays inside its context, and the rows go with their document.

use crate::db::models::*;
use crate::db::Database;

fn context(db: &Database, name: &str) -> i64 {
    db.create_context(&NewContext { name: name.into(), chunking_strategy: "structural".into(), ..Default::default() })
        .unwrap()
        .id
}

fn document(db: &Database, ctx: i64, name: &str) -> i64 {
    db.create_document(&NewDocument {
        context_id: ctx,
        name: name.into(),
        zip_entry: None,
        byte_size: None,
        page_count: None,
        content_hash: None,
        extracted_text: Some(format!("text of {name}")),
    })
    .unwrap()
    .id
}

fn source(doc: i64, ctx: i64, path: &str) -> SourceFile {
    SourceFile {
        document_id: doc,
        context_id: ctx,
        path: path.into(),
        byte_size: 10,
        mtime_ns: 1_000,
        sha256: format!("hash-{path}"),
        missing_since: None,
    }
}

#[test]
fn a_root_recorded_twice_stays_one_row_and_recursive_only_widens() {
    let db = Database::open_in_memory().unwrap();
    let ctx = context(&db, "A");
    db.record_source_root(ctx, "/data/a", true, true).unwrap();
    db.record_source_root(ctx, "/data/a", true, false).unwrap();
    db.record_source_root(ctx, "/data/b.pdf", false, false).unwrap();
    let roots = db.source_roots(ctx).unwrap();
    assert_eq!(roots.len(), 2);
    assert!(roots[0].recursive, "a non-recursive add must not narrow a recursive root");
    assert!(!roots[1].is_dir);
    assert_eq!(db.contexts_with_source_roots().unwrap(), vec![ctx]);
    db.set_source_roots_checked(ctx, 42).unwrap();
    assert!(db.source_roots(ctx).unwrap().iter().all(|r| r.checked_at == Some(42)));
}

#[test]
fn a_gone_file_is_marked_not_deleted_and_comes_back() {
    let db = Database::open_in_memory().unwrap();
    let ctx = context(&db, "A");
    let doc = document(&db, ctx, "a");
    db.record_source_file(&source(doc, ctx, "/d/a.txt"), false).unwrap();
    assert_eq!(db.mark_sources_missing(ctx, &["/d/a.txt".into()], 100).unwrap(), 1);
    assert_eq!(db.mark_sources_missing(ctx, &["/d/a.txt".into()], 200).unwrap(), 0, "first time kept");
    assert_eq!(db.missing_source_documents(&[ctx]).unwrap(), vec![(doc, "/d/a.txt".to_string(), 100)]);
    assert!(db.document(doc).unwrap().is_some(), "marking never deletes the document");
    assert_eq!(db.missing_document_ids(ctx).unwrap(), [doc].into_iter().collect());
    assert_eq!(db.set_source_present(ctx, "/d/a.txt", 11, 2_000).unwrap(), 1);
    assert!(db.missing_source_documents(&[ctx]).unwrap().is_empty());
    let f = &db.source_files(ctx).unwrap()[0];
    assert_eq!((f.byte_size, f.mtime_ns, f.missing_since), (11, 2_000, None));
}

#[test]
fn only_a_missing_file_moves_and_its_document_stays() {
    let db = Database::open_in_memory().unwrap();
    let ctx = context(&db, "A");
    let doc = document(&db, ctx, "a");
    db.record_source_file(&source(doc, ctx, "/d/a.txt"), false).unwrap();
    assert_eq!(db.move_missing_source(ctx, "/d/a.txt", "/d/b.txt", 10, 5).unwrap(), 0, "present files do not move");
    db.mark_sources_missing(ctx, &["/d/a.txt".into()], 100).unwrap();
    assert_eq!(db.move_missing_source(ctx, "/d/a.txt", "/d/b.txt", 10, 5).unwrap(), 1);
    let f = &db.source_files(ctx).unwrap()[0];
    assert_eq!((f.document_id, f.path.as_str(), f.missing_since), (doc, "/d/b.txt", None));
}

#[test]
fn pending_rows_are_not_listed_until_confirmed() {
    let db = Database::open_in_memory().unwrap();
    let ctx = context(&db, "A");
    let doc = document(&db, ctx, "a");
    db.record_source_file(&source(doc, ctx, "/d/a.txt"), true).unwrap();
    assert!(db.source_files(ctx).unwrap().is_empty());
    assert_eq!(db.pending_source_documents(ctx).unwrap(), vec![doc]);
    assert_eq!(db.mark_sources_missing(ctx, &["/d/a.txt".into()], 1).unwrap(), 0, "pending rows are never marked");
    db.confirm_pending_sources(ctx, &[doc]).unwrap();
    assert_eq!(db.source_files(ctx).unwrap().len(), 1);
    assert!(db.pending_source_documents(ctx).unwrap().is_empty());
}

#[test]
fn every_call_stays_inside_its_context() {
    let db = Database::open_in_memory().unwrap();
    let (a, b) = (context(&db, "A"), context(&db, "B"));
    let (da, db_doc) = (document(&db, a, "a"), document(&db, b, "b"));
    db.record_source_file(&source(da, a, "/shared/x.txt"), false).unwrap();
    db.record_source_file(&source(db_doc, b, "/shared/x.txt"), true).unwrap();
    assert_eq!(db.mark_sources_missing(a, &["/shared/x.txt".into()], 7).unwrap(), 1);
    assert!(db.missing_source_documents(&[b]).unwrap().is_empty());
    db.confirm_pending_sources(a, &[db_doc]).unwrap();
    assert_eq!(db.pending_source_documents(b).unwrap(), vec![db_doc], "context a cannot confirm b's rows");
    assert_eq!(db.set_source_present(b, "/shared/x.txt", 1, 1).unwrap(), 0, "b's row is pending");
    assert_eq!(db.missing_source_documents(&[a]).unwrap().len(), 1, "b's call did not touch a");
}

#[test]
fn documents_without_a_row_are_listed_by_file_name() {
    let db = Database::open_in_memory().unwrap();
    let ctx = context(&db, "A");
    let (old, tracked) = (document(&db, ctx, "old.pdf"), document(&db, ctx, "new.pdf"));
    db.record_source_file(&source(tracked, ctx, "/d/new.pdf"), false).unwrap();
    let rows = db.documents_without_source(ctx).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].0, rows[0].1.as_str()), (old, "old.pdf"));
}

#[test]
fn source_rows_go_with_their_document_and_context() {
    let db = Database::open_in_memory().unwrap();
    let ctx = context(&db, "A");
    let doc = document(&db, ctx, "a");
    db.record_source_root(ctx, "/d", true, true).unwrap();
    db.record_source_file(&source(doc, ctx, "/d/a.txt"), false).unwrap();
    db.delete_document(doc).unwrap();
    assert!(db.source_files(ctx).unwrap().is_empty());
    db.delete_context(ctx).unwrap();
    assert!(db.contexts_with_source_roots().unwrap().is_empty());
}

#[test]
fn the_lease_keeps_a_second_holder_out_until_it_expires_or_is_released() {
    let db = Database::open_in_memory().unwrap();
    let ctx = context(&db, "A");
    assert!(db.try_source_lease(ctx, "p1", 100, 60).unwrap());
    assert!(db.try_source_lease(ctx, "p1", 120, 60).unwrap(), "the holder renews");
    assert!(!db.try_source_lease(ctx, "p2", 150, 60).unwrap());
    assert_eq!(db.source_lease_holder(ctx, 150).unwrap().as_deref(), Some("p1"));
    assert!(db.try_source_lease(ctx, "p2", 181, 60).unwrap(), "expired leases are taken over");
    db.release_source_lease(ctx, "p1").unwrap();
    assert_eq!(db.source_lease_holder(ctx, 182).unwrap().as_deref(), Some("p2"), "only the holder releases");
    db.release_source_lease(ctx, "p2").unwrap();
    assert!(db.source_lease_holder(ctx, 182).unwrap().is_none());
}

/// A context with an embedding model and one chunk + vector per document text.
fn searchable(db: &Database, texts: &[&str]) -> (i64, Vec<i64>, Vec<i64>) {
    let m = db
        .create_embedding_model(&NewEmbeddingModel {
            identifier: "sf-embed".into(),
            kind: ModelKind::LocalOnnx,
            model_path: Some("/m/x.onnx".into()),
            tokenizer_path: None,
            api_config: None,
            execution_provider: None,
            is_matryoshka: false,
            native_dim: 2,
            default_dim: 2,
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
            name: "S".into(),
            chunking_strategy: "structural".into(),
            embedding_model_id: Some(m.id),
            embedding_dim: Some(2),
            ..Default::default()
        })
        .unwrap()
        .id;
    let (mut docs, mut chunks) = (Vec::new(), Vec::new());
    for (i, t) in texts.iter().enumerate() {
        let d = document(db, ctx, &format!("d{i}"));
        let c = db
            .create_chunk(&NewChunk {
                context_id: ctx,
                document_id: d,
                chunk_index: 0,
                char_start: 0,
                char_end: t.len() as i64,
                text: t.to_string(),
                signature: None,
                is_omitted: false,
                metadata: "{}".into(),
            })
            .unwrap();
        db.set_chunk_refs(c.id, ctx, t).unwrap();
        db.insert_embedding(&NewEmbedding { chunk_id: c.id, context_id: ctx, document_id: d, embedding_model_id: m.id, dim: 2, vector: vec![1.0, 0.0] })
            .unwrap();
        db.record_source_file(&source(d, ctx, &format!("/d/{i}.txt")), false).unwrap();
        docs.push(d);
        chunks.push(c.id);
    }
    (ctx, docs, chunks)
}

#[test]
fn every_search_path_leaves_out_documents_whose_file_is_missing_by_exclusion() {
    let db = Database::open_in_memory().unwrap();
    let text = "Quarzbruecke fee under Art. 5 DSGVO";
    let (ctx, docs, chunks) = searchable(&db, &[text, text, text]);
    db.mark_sources_missing(ctx, &["/d/1.txt".into()], 9).unwrap();
    let ids = |v: Vec<(i64, usize)>| v.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
    let keyword = ids(db.keyword_search_context(ctx, "Quarzbruecke", 10, None).unwrap());
    let phrase = ids(db.phrase_search_context(ctx, "Quarzbruecke fee", 10, None).unwrap());
    let vector: Vec<i64> = db.search_context(ctx, &[1.0, 0.0], 10, None).unwrap().into_iter().map(|s| s.chunk_id).collect();
    let hybrid: Vec<i64> = db
        .retrieve_hybrid_with(&[ctx], &[(db.context(ctx).unwrap().unwrap().embedding_model_id.unwrap(), vec![1.0, 0.0])].into_iter().collect(), "Quarzbruecke", 10, None)
        .unwrap()
        .into_iter()
        .map(|s| s.chunk_id)
        .collect();
    let key: String = db.conn.query_row("SELECT ref_key FROM chunk_refs LIMIT 1", [], |r| r.get(0)).unwrap();
    let refs: Vec<i64> = db.chunks_with_ref(&[ctx], &key).unwrap().into_iter().map(|c| c.id).collect();
    for (name, got) in [("keyword", keyword), ("phrase", phrase), ("vector", vector), ("hybrid", hybrid)] {
        let mut got = got;
        got.sort();
        assert_eq!(got, vec![chunks[0], chunks[2]], "{name}: the missing document is left out");
    }
    assert_eq!(refs, vec![chunks[0], chunks[2]], "citing chunks leave it out too");
    // A narrower file scope still works next to the exclusion, and reading by id still finds it.
    let scoped = ids(db.keyword_search_context(ctx, "Quarzbruecke", 10, Some(&[docs[1], docs[2]])).unwrap());
    assert_eq!(scoped, vec![chunks[2]]);
    assert_eq!(db.chunks_by_ids(&[chunks[1]]).unwrap().len(), 1);
    // Back on disk: searched again.
    db.set_source_present(ctx, "/d/1.txt", 10, 1_000).unwrap();
    assert_eq!(db.keyword_search_context(ctx, "Quarzbruecke", 10, None).unwrap().len(), 3);
}
