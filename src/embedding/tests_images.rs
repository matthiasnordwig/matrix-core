//! Hybrid retrieval with the page-image rank list (`image_list.rs`): without
//! image vectors bit-identical to the two-list fusion; an image hit lifts the
//! query-closest chunk of its page (structural `metadata.page` and LLM byte
//! ranges); never compares image vectors of another model; honours the
//! `doc_ids` scope, omitted chunks and the `img=0` switch; the preview mark
//! (`page_image_id`) only for an image rank within 30 % of the visible
//! `preview_k` (the pure rule and provenance math: `tests_provenance.rs`).

use std::collections::HashMap;

use super::fusion::FusionParams;
use super::retrieval::{hybrid_fanout, rrf_fuse_weighted};
use super::tests::{context_with_vectors, model};
use crate::db::models::*;
use crate::Database;

const Q: [f32; 3] = [1.0, 0.0, 0.0];
/// Matches no chunk text, so only the vector and image lists decide.
const RAW: &str = "zzzq";

fn on() -> FusionParams {
    FusionParams::default()
}

fn off() -> FusionParams {
    FusionParams { img: false, ..FusionParams::default() }
}

/// (context_id, document_id) for `model_id` (dim 3).
fn ctx_doc(db: &Database, model_id: i64) -> (i64, i64) {
    let ctx = db
        .create_context(&NewContext {
            name: "Bericht".into(),
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
        })
        .unwrap();
    (ctx.id, add_doc(db, ctx.id))
}

fn add_doc(db: &Database, ctx: i64) -> i64 {
    db.create_document(&NewDocument {
        context_id: ctx,
        name: "bericht.pdf".into(),
        zip_entry: None,
        byte_size: None,
        page_count: None,
        content_hash: None,
        extracted_text: None,
    })
    .unwrap()
    .id
}

/// One chunk: (metadata page, byte range, omitted, vector).
type Spec = (Option<i64>, (i64, i64), bool, [f32; 3]);

fn add_chunks(db: &Database, ctx: i64, doc: i64, model_id: i64, specs: &[Spec]) -> Vec<i64> {
    specs
        .iter()
        .enumerate()
        .map(|(i, (page, (s, e), omitted, v))| {
            let metadata = page.map(|p| format!(r#"{{"section":"","page":{p}}}"#)).unwrap_or_else(|| "{}".into());
            let c = db
                .create_chunk(&NewChunk {
                    context_id: ctx,
                    document_id: doc,
                    chunk_index: i as i64,
                    char_start: *s,
                    char_end: *e,
                    text: format!("abschnitt {i}"),
                    signature: None,
                    is_omitted: *omitted,
                    metadata,
                })
                .unwrap();
            db.insert_embedding(&NewEmbedding {
                chunk_id: c.id,
                context_id: ctx,
                document_id: doc,
                embedding_model_id: model_id,
                dim: 3,
                vector: v.to_vec(),
            })
            .unwrap();
            c.id
        })
        .collect()
}

fn add_page(db: &Database, ctx: i64, doc: i64, page: i64, range: Option<(i64, i64)>, vec_by: Option<(i64, &[f32])>) -> i64 {
    let p = db
        .insert_page_image(&NewPageImage {
            context_id: ctx,
            document_id: doc,
            page,
            char_start: range.map(|r| r.0),
            char_end: range.map(|r| r.1),
            width: 10,
            height: 10,
            image: vec![0xFF, 0xD8],
        })
        .unwrap();
    if let Some((model_id, v)) = vec_by {
        db.upsert_page_image_embedding(p.id, model_id, v).unwrap();
    }
    p.id
}

fn queries(model_id: i64) -> HashMap<i64, Vec<f32>> {
    HashMap::from([(model_id, Q.to_vec())])
}

/// The two-list hybrid fusion exactly as before the image list existed.
fn two_list_reference(db: &Database, ctxs: &[i64], q: &HashMap<i64, Vec<f32>>, raw: &str, top_k: usize, doc_ids: Option<&[i64]>) -> Vec<ScoredChunk> {
    let n = hybrid_fanout(top_k);
    let f = FusionParams::default();
    let kw = super::fusion::keyword_query(raw, f.drop_stopwords);
    let mut acc: HashMap<i64, f32> = HashMap::new();
    for (model_id, cids) in db.contexts_by_model(ctxs).unwrap() {
        let Some(qvec) = q.get(&model_id) else { continue };
        for cid in cids {
            let v: Vec<i64> = db.search_context(cid, qvec, n, doc_ids).unwrap().iter().map(|h| h.chunk_id).collect();
            let t: Vec<i64> = db.keyword_search_context(cid, &kw, n, doc_ids).unwrap().into_iter().map(|(id, _)| id).collect();
            for sc in rrf_fuse_weighted(&[(v.as_slice(), 1.0), (t.as_slice(), f.fts_weight)], f.k, n) {
                *acc.entry(sc.chunk_id).or_insert(0.0) += sc.score;
            }
        }
    }
    let mut out: Vec<ScoredChunk> = acc.into_iter().map(|(chunk_id, score)| ScoredChunk::new(chunk_id, score)).collect();
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap().then(a.chunk_id.cmp(&b.chunk_id)));
    out.truncate(top_k);
    out
}

fn bits(v: &[ScoredChunk]) -> Vec<(i64, u32, Option<i64>)> {
    v.iter().map(|s| (s.chunk_id, s.score.to_bits(), s.page_image_id)).collect()
}

fn run(db: &Database, ctxs: &[i64], q: &HashMap<i64, Vec<f32>>, doc_ids: Option<&[i64]>, f: &FusionParams) -> Vec<ScoredChunk> {
    db.retrieve_hybrid_with_params(ctxs, q, RAW, 10, 10, doc_ids, f).unwrap()
}

/// Structural layout: c0 p1 (best text hit), ca p2, c1 p3, c2 p3, c3 p5.
fn structural(db: &Database) -> (i64, i64, i64, Vec<i64>) {
    let m = model(db, "gemma", 3).id;
    let (ctx, doc) = ctx_doc(db, m);
    let ids = add_chunks(
        db,
        ctx,
        doc,
        m,
        &[
            (Some(1), (0, 10), false, [1.0, 0.0, 0.0]),
            (Some(2), (10, 20), false, [0.0, 1.0, 0.0]),
            (Some(3), (20, 30), false, [0.2, 1.0, 0.0]),
            (Some(3), (30, 40), false, [0.6, 0.8, 0.0]),
            (Some(5), (40, 50), false, [0.9, 0.4, 0.0]),
        ],
    );
    (m, ctx, doc, ids)
}

#[test]
fn without_image_vectors_the_result_is_bit_identical() {
    let db = Database::open_in_memory().unwrap();
    let ma = model(&db, "a", 4);
    let mb = model(&db, "b", 3);
    let a = context_with_vectors(&db, "A", ma.id, 4, &[vec![1.0, 0.0, 0.0, 0.0], vec![0.5, 0.5, 0.0, 0.0]]);
    let _ = context_with_vectors(&db, "B", mb.id, 3, &[vec![0.0, 1.0, 0.0], vec![1.0, 0.0, 0.0]]);
    let ctx_a = db.chunk(a[0]).unwrap().unwrap().context_id;
    let doc_a = db.chunk(a[0]).unwrap().unwrap().document_id;
    // Page images without any vector must not change anything either.
    add_page(&db, ctx_a, doc_a, 1, Some((0, 1)), None);
    let ctxs: Vec<i64> = db.list_contexts().unwrap().iter().map(|c| c.id).collect();
    let q = HashMap::from([(ma.id, vec![1.0, 0.0, 0.0, 0.0]), (mb.id, Q.to_vec())]);
    let reference = two_list_reference(&db, &ctxs, &q, "abschnitt chunk", 10, None);
    assert!(!reference.is_empty());
    for f in [on(), off()] {
        let got = db.retrieve_hybrid_with_params(&ctxs, &q, "abschnitt chunk", 10, 10, None, &f).unwrap();
        assert_eq!(bits(&got), bits(&reference));
    }
}

#[test]
fn structural_image_hit_lifts_the_closest_chunk_of_its_page() {
    let db = Database::open_in_memory().unwrap();
    let (m, ctx, doc, ids) = structural(&db);
    let before = run(&db, &[ctx], &queries(m), None, &on());
    assert_eq!(before[0].chunk_id, ids[0]);
    let pid = add_page(&db, ctx, doc, 3, None, Some((m, &Q)));
    let after = run(&db, &[ctx], &queries(m), None, &on());
    // Page 3 candidates: ca (p2..=3), c1, c2 (p3..=5) → c2 is closest to Q.
    assert_eq!(after[0].chunk_id, ids[3]);
    assert_eq!(after[0].page_image_id, Some(pid));
    assert!(after.iter().filter(|s| s.chunk_id != ids[3]).all(|s| s.page_image_id.is_none()));
    // c0 (best text hit, page 1) is not on page 3 and gets nothing.
    assert!(after.iter().any(|s| s.chunk_id == ids[0] && s.page_image_id.is_none()));
}

#[test]
fn llm_image_hit_uses_the_byte_range_of_its_page() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "gemma", 3).id;
    let (ctx, doc) = ctx_doc(&db, m);
    let ids = add_chunks(
        &db,
        ctx,
        doc,
        m,
        &[
            (None, (0, 10), false, [1.0, 0.0, 0.0]),
            (None, (10, 20), false, [0.3, 0.9, 0.0]),
            (None, (20, 30), false, [0.1, 1.0, 0.0]),
            (None, (30, 40), false, [0.95, 0.1, 0.0]),
        ],
    );
    let pid = add_page(&db, ctx, doc, 2, Some((12, 30)), Some((m, &Q)));
    let got = run(&db, &[ctx], &queries(m), None, &on());
    let marked: Vec<(i64, Option<i64>)> = got.iter().filter(|s| s.page_image_id.is_some()).map(|s| (s.chunk_id, s.page_image_id)).collect();
    assert_eq!(marked, vec![(ids[1], Some(pid))]);
    // Without a byte range the page cannot be mapped in this mode.
    let db2 = Database::open_in_memory().unwrap();
    let m2 = model(&db2, "gemma", 3).id;
    let (ctx2, doc2) = ctx_doc(&db2, m2);
    add_chunks(&db2, ctx2, doc2, m2, &[(None, (0, 10), false, [1.0, 0.0, 0.0])]);
    add_page(&db2, ctx2, doc2, 1, None, Some((m2, &Q)));
    assert_eq!(bits(&run(&db2, &[ctx2], &queries(m2), None, &on())), bits(&run(&db2, &[ctx2], &queries(m2), None, &off())));
}

#[test]
fn image_vectors_of_another_model_are_never_compared() {
    let db = Database::open_in_memory().unwrap();
    let (m, ctx, doc, _) = structural(&db);
    let other = model(&db, "jina", 3).id;
    // Same dimension on purpose: only the model filter keeps it out.
    add_page(&db, ctx, doc, 3, None, Some((other, &Q)));
    // A vector of the right model but another dimension is skipped, too.
    add_page(&db, ctx, doc, 5, None, Some((m, &[1.0, 0.0])));
    let got = run(&db, &[ctx], &queries(m), None, &on());
    assert!(got.iter().all(|s| s.page_image_id.is_none()));
    assert_eq!(bits(&got), bits(&run(&db, &[ctx], &queries(m), None, &off())));
}

#[test]
fn doc_scope_applies_to_the_image_list() {
    let db = Database::open_in_memory().unwrap();
    let (m, ctx, doc, _) = structural(&db);
    let other_doc = add_doc(&db, ctx);
    add_chunks(&db, ctx, other_doc, m, &[(Some(1), (0, 10), false, [0.0, 0.0, 1.0])]);
    add_page(&db, ctx, other_doc, 1, None, Some((m, &Q)));
    let scope = [doc];
    let got = run(&db, &[ctx], &queries(m), Some(&scope), &on());
    assert!(got.iter().all(|s| s.page_image_id.is_none()));
    assert_eq!(bits(&got), bits(&run(&db, &[ctx], &queries(m), Some(&scope), &off())));
    assert!(run(&db, &[ctx], &queries(m), Some(&[]), &on()).is_empty());
    // Unscoped, the other document's page does count.
    assert!(run(&db, &[ctx], &queries(m), None, &on()).iter().any(|s| s.page_image_id.is_some()));
}

#[test]
fn img_off_switches_the_list_off() {
    let db = Database::open_in_memory().unwrap();
    let (m, ctx, doc, ids) = structural(&db);
    add_page(&db, ctx, doc, 3, None, Some((m, &Q)));
    let got = run(&db, &[ctx], &queries(m), None, &off());
    assert_eq!(got[0].chunk_id, ids[0]);
    assert_eq!(bits(&got), bits(&two_list_reference(&db, &[ctx], &queries(m), RAW, 10, None)));
}

#[test]
fn omitted_chunks_are_never_chosen_and_best_rank_wins() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "gemma", 3).id;
    let (ctx, doc) = ctx_doc(&db, m);
    let ids = add_chunks(
        &db,
        ctx,
        doc,
        m,
        &[
            (Some(1), (0, 10), false, [0.0, 0.0, 1.0]),
            (Some(3), (10, 20), false, [0.2, 1.0, 0.0]),
            (Some(4), (20, 30), true, [1.0, 0.0, 0.0]),
            (Some(6), (30, 40), false, [0.0, 1.0, 0.0]),
        ],
    );
    // Page 3: c0 (p1..=3, score 0) or c1 (p3..=4) → c1. Page 4: c1 or the
    // omitted c2 (closest, but never a target) → c1 again.
    let p3 = add_page(&db, ctx, doc, 3, None, Some((m, &Q)));
    let p4 = add_page(&db, ctx, doc, 4, None, Some((m, &[0.9, 0.1, 0.0])));
    let got = run(&db, &[ctx], &queries(m), None, &on());
    let get = |id: i64| got.iter().find(|s| s.chunk_id == id).and_then(|s| s.page_image_id);
    assert_eq!(get(ids[2]), None, "omitted chunk is never the image target");
    assert_eq!(get(ids[1]), Some(p3), "the better-ranked page image wins");
    assert!(got.iter().all(|s| s.page_image_id != Some(p4)));
}

#[test]
fn preview_mark_needs_an_image_rank_within_30_percent_of_preview_k_but_ranks_stay_equal() {
    let db = Database::open_in_memory().unwrap();
    let (m, ctx, doc, ids) = structural(&db);
    // Image ranks: page 3 (cos 1.0) → rank 1, page 5 (cos ~0.71) → rank 2,
    // page 1 (cos 0) → rank 3. Text ranks don't matter for the preview.
    let p3 = add_page(&db, ctx, doc, 3, None, Some((m, &Q)));
    let p5 = add_page(&db, ctx, doc, 5, None, Some((m, &[0.5, 0.5, 0.0])));
    let p1 = add_page(&db, ctx, doc, 1, None, Some((m, &[0.0, 1.0, 0.0])));
    let get = |k: usize| db.retrieve_hybrid_with_params(&[ctx], &queries(m), RAW, 10, k, None, &on()).unwrap();
    let marked = |v: &[ScoredChunk]| -> Vec<(i64, i64)> {
        let mut x: Vec<_> = v.iter().filter_map(|s| s.page_image_id.map(|p| (s.chunk_id, p))).collect();
        x.sort();
        x
    };
    let all = get(10);
    // preview_k 10 → image rank ≤ 3: all three pages.
    assert_eq!(marked(&all), { let mut e = vec![(ids[3], p3), (ids[4], p5), (ids[0], p1)]; e.sort(); e });
    // 1 → ≤ 1, 2 → ≤ 1 (0.6 rounded up), 4 → ≤ 2 (1.2 rounded up), 0 → none.
    for (k, expect) in [(1, vec![(ids[3], p3)]), (2, vec![(ids[3], p3)]), (4, vec![(ids[3], p3), (ids[4], p5)]), (0, vec![])] {
        let got = get(k);
        let ranks = |v: &[ScoredChunk]| v.iter().map(|s| (s.chunk_id, s.score.to_bits())).collect::<Vec<_>>();
        assert_eq!(ranks(&got), ranks(&all), "preview_k={k} must not change ranks or scores");
        let mut expect = expect;
        expect.sort();
        assert_eq!(marked(&got), expect, "preview_k={k}");
    }
    // The default entry point uses top_k as the threshold.
    let few = db.retrieve_hybrid_with(&[ctx], &queries(m), RAW, 1, None).unwrap();
    assert_eq!(few.len(), 1);
    let same_as_k1 = get(1).into_iter().next().unwrap();
    assert_eq!((few[0].chunk_id, few[0].page_image_id), (same_as_k1.chunk_id, same_as_k1.page_image_id));
}
