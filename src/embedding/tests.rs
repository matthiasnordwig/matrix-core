//! Retrieval-merge tests using a deterministic fake embedder, exercising the
//! mixed-embedding-space path (two contexts, two models, two dimensions).

use super::QueryEmbedder;
use crate::db::models::*;
use crate::{Database, Result};

/// Returns a unit vector pointing at axis 0 in the model's own dimension.
struct FakeEmbedder;
impl QueryEmbedder for FakeEmbedder {
    fn embed_query(&self, model: &EmbeddingModel, _query: &str) -> Result<Vec<f32>> {
        let mut v = vec![0.0f32; model.default_dim as usize];
        v[0] = 1.0;
        Ok(v)
    }
}

pub(super) fn model(db: &Database, ident: &str, dim: i64) -> EmbeddingModel {
    db.create_embedding_model(&NewEmbeddingModel {
        identifier: ident.into(),
        kind: ModelKind::LocalOnnx,
        model_path: Some(format!("/models/{ident}.onnx")),
        tokenizer_path: None,
        api_config: None,
        execution_provider: Some(ExecutionProvider::Ane),
        is_matryoshka: false,
        native_dim: dim,
        default_dim: dim,
        normalize: true,
        tpm_limit: None,
        rpm_limit: None,
        max_concurrency: 1,
        query_prefix: None,
        document_prefix: None,
    })
    .unwrap()
}

pub(super) fn context_with_vectors(
    db: &Database,
    name: &str,
    model_id: i64,
    dim: i64,
    vectors: &[Vec<f32>],
) -> Vec<i64> {
    let ctx = db
        .create_context(&NewContext {
            name: name.into(),
            description: None,
            chunking_profile_id: None,
            embedding_model_id: Some(model_id),
            embedding_dim: Some(dim),
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

    let mut chunk_ids = Vec::new();
    for (i, v) in vectors.iter().enumerate() {
        let chunk = db
            .create_chunk(&NewChunk {
                context_id: ctx.id,
                document_id: doc.id,
                chunk_index: i as i64,
                char_start: 0,
                char_end: 1,
                text: format!("{name} chunk {i}"),
                signature: None,
                is_omitted: false,
                metadata: "{}".into(),
            })
            .unwrap();
        db.insert_embedding(&NewEmbedding {
            chunk_id: chunk.id,
            context_id: ctx.id,
            document_id: doc.id,
            embedding_model_id: model_id,
            dim,
            vector: v.clone(),
        })
        .unwrap();
        chunk_ids.push(chunk.id);
    }
    chunk_ids
}

#[test]
fn retrieves_across_mixed_embedding_spaces() {
    let db = Database::open_in_memory().unwrap();

    // Two distinct models / dimensions — must never be compared cross-space.
    let m4 = model(&db, "model-4d", 4);
    let m3 = model(&db, "model-3d", 3);

    let a = context_with_vectors(
        &db,
        "Context_A",
        m4.id,
        4,
        &[vec![1.0, 0.0, 0.0, 0.0], vec![0.0, 1.0, 0.0, 0.0]],
    );
    let b = context_with_vectors(
        &db,
        "Context_B",
        m3.id,
        3,
        &[vec![1.0, 0.0, 0.0], vec![0.0, 0.0, 1.0]],
    );

    let hits = db
        .retrieve(&[1, 2], "query", 2, &FakeEmbedder)
        .unwrap();

    assert_eq!(hits.len(), 2);
    // The axis-0 vectors from each space are the best matches (score ~1.0).
    let ids: Vec<i64> = hits.iter().map(|h| h.chunk_id).collect();
    assert!(ids.contains(&a[0]), "best A chunk missing: {ids:?}");
    assert!(ids.contains(&b[0]), "best B chunk missing: {ids:?}");
    assert!(hits[0].score > 0.99);
}

#[test]
fn single_context_retrieval() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "m", 4);
    let ids = context_with_vectors(
        &db,
        "Only",
        m.id,
        4,
        &[vec![1.0, 0.0, 0.0, 0.0], vec![0.0, 1.0, 0.0, 0.0], vec![0.0, 0.0, 1.0, 0.0]],
    );
    let hits = db.retrieve(&[1], "q", 1, &FakeEmbedder).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].chunk_id, ids[0]);
}

// --- hybrid / RRF -----------------------------------------------------------

use super::retrieval::{hybrid_fanout, rrf_fuse, RRF_K};

#[test]
fn hybrid_fanout_is_max_50_5x() {
    assert_eq!(hybrid_fanout(1), 50);
    assert_eq!(hybrid_fanout(10), 50);
    assert_eq!(hybrid_fanout(20), 100);
}

#[test]
fn rrf_fuse_pure_math() {
    // chunk 1: rank1 in listA + rank2 in listB.
    // chunk 2: rank2 in listA only. chunk 3: rank1 in listB only.
    let list_a = vec![1, 2];
    let list_b = vec![3, 1];
    let fused = rrf_fuse(&[list_a, list_b], RRF_K, 10);

    let score = |id: i64| fused.iter().find(|s| s.chunk_id == id).unwrap().score;
    let expect_1 = 1.0 / (RRF_K + 1) as f32 + 1.0 / (RRF_K + 2) as f32;
    let expect_2 = 1.0 / (RRF_K + 2) as f32;
    let expect_3 = 1.0 / (RRF_K + 1) as f32;
    assert!((score(1) - expect_1).abs() < 1e-6);
    assert!((score(2) - expect_2).abs() < 1e-6);
    assert!((score(3) - expect_3).abs() < 1e-6);
    // chunk 1 appears in both lists -> highest.
    assert_eq!(fused[0].chunk_id, 1);
}

#[test]
fn rrf_fuse_truncates_and_breaks_ties_deterministically() {
    // Two chunks each at rank 1 of a separate single-item list -> equal score,
    // tie broken by ascending id.
    let fused = rrf_fuse(&[vec![7], vec![3]], RRF_K, 10);
    assert_eq!(fused.iter().map(|s| s.chunk_id).collect::<Vec<_>>(), vec![3, 7]);
    // top_k truncation.
    let fused = rrf_fuse(&[vec![7], vec![3]], RRF_K, 1);
    assert_eq!(fused.len(), 1);
}

#[test]
fn hybrid_keeps_cross_space_isolation() {
    let db = Database::open_in_memory().unwrap();
    // Two distinct models/dims, must never be compared cross-space.
    let m4 = model(&db, "hyb-4d", 4);
    let m3 = model(&db, "hyb-3d", 3);
    let a = context_with_vectors(
        &db,
        "Context_A",
        m4.id,
        4,
        &[vec![1.0, 0.0, 0.0, 0.0], vec![0.0, 1.0, 0.0, 0.0]],
    );
    let b = context_with_vectors(
        &db,
        "Context_B",
        m3.id,
        3,
        &[vec![1.0, 0.0, 0.0], vec![0.0, 0.0, 1.0]],
    );

    // Precompute per-model query vectors (axis-0 in each space).
    let mut qbm: std::collections::HashMap<i64, Vec<f32>> = std::collections::HashMap::new();
    qbm.insert(m4.id, vec![1.0, 0.0, 0.0, 0.0]);
    qbm.insert(m3.id, vec![1.0, 0.0, 0.0]);

    // Raw query "chunk" hits every chunk's text ("<name> chunk <i>") in FTS.
    let hits = db.retrieve_hybrid_with(&[1, 2], &qbm, "chunk", 4, None).unwrap();
    let ids: Vec<i64> = hits.iter().map(|h| h.chunk_id).collect();
    // Best of each space still surfaces; no cross-space crash / dim mismatch.
    assert!(ids.contains(&a[0]), "best A missing: {ids:?}");
    assert!(ids.contains(&b[0]), "best B missing: {ids:?}");
    assert!(hits.iter().all(|h| h.score > 0.0));
}

#[test]
fn hybrid_batch_matches_looped_single() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "hyb-batch", 4);
    let ids = context_with_vectors(
        &db,
        "Only",
        m.id,
        4,
        &[vec![1.0, 0.0, 0.0, 0.0], vec![0.0, 1.0, 0.0, 0.0]],
    );
    let mut qbm: std::collections::HashMap<i64, Vec<f32>> = std::collections::HashMap::new();
    qbm.insert(m.id, vec![1.0, 0.0, 0.0, 0.0]);

    let single = db.retrieve_hybrid_with(&[1], &qbm, "chunk 0", 2, None).unwrap();
    let batch = db
        .retrieve_hybrid_batch(&[1], &[qbm.clone()], &["chunk 0".to_string()], 2, None)
        .unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(
        batch[0].iter().map(|h| h.chunk_id).collect::<Vec<_>>(),
        single.iter().map(|h| h.chunk_id).collect::<Vec<_>>()
    );
    // "chunk 0" text present, so at least the first chunk is retrieved.
    assert!(single.iter().any(|h| h.chunk_id == ids[0]));
}

/// AP8 file-level scope on the VECTOR path: `retrieve_with` / `retrieve_hybrid_with`
/// with `doc_ids = Some(&[…])` must never return a chunk from an out-of-scope
/// document (leak-regression, mirror of the FTS-side test in `db/fts_tests.rs`).
#[test]
fn doc_scope_restricts_vector_and_hybrid_lists() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "scope-4d", 4);
    let ctx = db
        .create_context(&NewContext {
            name: "Scoped".into(),
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
            image_llm_id: None,
            chunking_strategy: "Semantic".into(),
            structural_profile_id: None,
        })
        .unwrap();
    // Two documents in the SAME context/embedding space.
    let mut docs = Vec::new();
    for name in ["a.pdf", "b.pdf"] {
        docs.push(
            db.create_document(&NewDocument {
                context_id: ctx.id,
                name: name.into(),
                zip_entry: None,
                byte_size: None,
                page_count: None,
                content_hash: None,
                extracted_text: None,
            })
            .unwrap()
            .id,
        );
    }
    // One identical axis-0 vector per doc so both would tie without a filter.
    let mut chunk_by_doc = Vec::new();
    for (i, &doc) in docs.iter().enumerate() {
        let chunk = db
            .create_chunk(&NewChunk {
                context_id: ctx.id,
                document_id: doc,
                chunk_index: i as i64,
                char_start: 0,
                char_end: 1,
                text: format!("Auslagerung chunk in doc {i}"),
                signature: None,
                is_omitted: false,
                metadata: "{}".into(),
            })
            .unwrap();
        db.insert_embedding(&NewEmbedding {
            chunk_id: chunk.id,
            context_id: ctx.id,
            document_id: doc,
            embedding_model_id: m.id,
            dim: 4,
            vector: vec![1.0, 0.0, 0.0, 0.0],
        })
        .unwrap();
        chunk_by_doc.push(chunk.id);
    }
    let mut qbm: std::collections::HashMap<i64, Vec<f32>> = std::collections::HashMap::new();
    qbm.insert(m.id, vec![1.0, 0.0, 0.0, 0.0]);

    // No scope: both docs' chunks retrievable.
    let all: Vec<i64> = db
        .retrieve_with(&[ctx.id], &qbm, 10, None)
        .unwrap()
        .into_iter()
        .map(|h| h.chunk_id)
        .collect();
    assert!(all.contains(&chunk_by_doc[0]) && all.contains(&chunk_by_doc[1]));

    // Scope to doc a: only a's chunk, vector path.
    let scoped: Vec<i64> = db
        .retrieve_with(&[ctx.id], &qbm, 10, Some(&[docs[0]]))
        .unwrap()
        .into_iter()
        .map(|h| h.chunk_id)
        .collect();
    assert!(scoped.contains(&chunk_by_doc[0]), "in-scope chunk must be found");
    assert!(!scoped.contains(&chunk_by_doc[1]), "out-of-scope chunk must NOT leak (vector)");

    // Scope to doc a: hybrid path (vector + FTS) must be scoped identically.
    let scoped_hybrid: Vec<i64> = db
        .retrieve_hybrid_with(&[ctx.id], &qbm, "Auslagerung", 10, Some(&[docs[0]]))
        .unwrap()
        .into_iter()
        .map(|h| h.chunk_id)
        .collect();
    assert!(scoped_hybrid.contains(&chunk_by_doc[0]));
    assert!(!scoped_hybrid.contains(&chunk_by_doc[1]), "out-of-scope chunk must NOT leak (hybrid)");

    // Empty scope → empty on both paths.
    assert!(db.retrieve_with(&[ctx.id], &qbm, 10, Some(&[])).unwrap().is_empty());
    assert!(db
        .retrieve_hybrid_with(&[ctx.id], &qbm, "Auslagerung", 10, Some(&[]))
        .unwrap()
        .is_empty());
}

// --- score_chunks_by_ids (TOOL_CALLS_V2_PLAN AP4) ----------------------------

#[test]
fn score_chunks_by_ids_ranks_within_one_space() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "model-4d", 4);
    // Three chunks at increasing angular distance from the query axis.
    let ids = context_with_vectors(
        &db,
        "ctx",
        m.id,
        4,
        &[
            vec![1.0, 0.0, 0.0, 0.0], // identical to query -> cosine 1.0
            vec![0.0, 1.0, 0.0, 0.0], // orthogonal -> cosine 0.0
            vec![0.7, 0.7, 0.0, 0.0], // 45 degrees -> cosine ~0.707
        ],
    );
    let mut qbm: std::collections::HashMap<i64, Vec<f32>> = std::collections::HashMap::new();
    qbm.insert(m.id, vec![1.0, 0.0, 0.0, 0.0]);

    let scores = db.score_chunks_by_ids(&ids, &qbm).unwrap();
    assert!((scores[&ids[0]] - 1.0).abs() < 1e-6);
    assert!(scores[&ids[1]].abs() < 1e-6);
    assert!(scores[&ids[2]] > scores[&ids[1]] && scores[&ids[2]] < scores[&ids[0]]);
}

#[test]
fn score_chunks_by_ids_never_compares_across_spaces() {
    let db = Database::open_in_memory().unwrap();
    let m4 = model(&db, "model-4d", 4);
    let m2 = model(&db, "model-2d", 2);
    let ids4 = context_with_vectors(&db, "ctx4", m4.id, 4, &[vec![1.0, 0.0, 0.0, 0.0]]);
    let ids2 = context_with_vectors(&db, "ctx2", m2.id, 2, &[vec![1.0, 0.0]]);

    // Query vector only provided for the 4-dim model — the 2-dim chunk's
    // context has no entry in query_by_model, so it must score 0.0, not error
    // or panic on a dimension mismatch.
    let mut qbm: std::collections::HashMap<i64, Vec<f32>> = std::collections::HashMap::new();
    qbm.insert(m4.id, vec![1.0, 0.0, 0.0, 0.0]);

    let mut all_ids = ids4.clone();
    all_ids.extend(&ids2);
    let scores = db.score_chunks_by_ids(&all_ids, &qbm).unwrap();
    assert!((scores[&ids4[0]] - 1.0).abs() < 1e-6);
    assert_eq!(scores[&ids2[0]], 0.0);
}

#[test]
fn score_chunks_by_ids_missing_vector_scores_zero() {
    let db = Database::open_in_memory().unwrap();
    let m = model(&db, "model-4d", 4);
    let ctx = db
        .create_context(&NewContext {
            name: "ctx".into(),
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
    // A chunk with no embedding row at all (e.g. embedding still pending).
    let chunk = db
        .create_chunk(&NewChunk {
            context_id: ctx.id,
            document_id: doc.id,
            chunk_index: 0,
            char_start: 0,
            char_end: 1,
            text: "unembedded".into(),
            signature: None,
            is_omitted: false,
            metadata: "{}".into(),
        })
        .unwrap();

    let mut qbm: std::collections::HashMap<i64, Vec<f32>> = std::collections::HashMap::new();
    qbm.insert(m.id, vec![1.0, 0.0, 0.0, 0.0]);
    let scores = db.score_chunks_by_ids(&[chunk.id], &qbm).unwrap();
    assert_eq!(scores[&chunk.id], 0.0);
}

// --- fusion.rs: keyword cleanup + weighted RRF ------------------------------

use super::fusion::{keyword_query, FusionParams};
use super::retrieval::rrf_fuse_weighted;

#[test]
fn keyword_query_drops_function_words_but_keeps_identifiers() {
    assert_eq!(
        keyword_query("Within what period must the client pay the provider's invoices?", true),
        "period client pay provider's invoices?"
    );
    assert_eq!(
        keyword_query("Welche Pflichten gelten nach § 25b KWG und Art. 28 DORA?", true),
        "Pflichten gelten § 25b KWG Art. 28 DORA?"
    );
    // A number/identifier is never a function word, even if short.
    assert_eq!(keyword_query("in 2 Nr. 6", true), "2 Nr. 6");
    // Only function words: keep the query as typed.
    assert_eq!(keyword_query("Was ist das?", true), "Was ist das?");
    // Switched off: unchanged.
    assert_eq!(keyword_query("Was regelt die CRR?", false), "Was regelt die CRR?");
}

#[test]
fn fusion_params_parse_and_defaults() {
    let d = FusionParams::default();
    assert!(d.drop_stopwords);
    assert_eq!((d.k, d.fts_weight), (10, 1.0));
    assert_eq!(FusionParams::parse("stop=0"), FusionParams { drop_stopwords: false, ..d });
    assert_eq!(FusionParams::parse("stop=1,w=0.5,k=10"), FusionParams { drop_stopwords: true, fts_weight: 0.5, k: 10, img: true });
    assert!(d.img);
    assert_eq!(FusionParams::parse("img=0"), FusionParams { img: false, ..d });
    assert_eq!(FusionParams::parse("img=1,k=20"), FusionParams { k: 20, ..d });
    assert_eq!(FusionParams::parse("w=9"), FusionParams { fts_weight: 2.0, ..d });
    assert_eq!(FusionParams::parse("bogus,w=x"), d);
}

#[test]
fn weighted_rrf_lets_a_strong_vector_hit_beat_double_presence() {
    // Chunk 1: vector rank 1 only. Chunk 2: rank 5 in both lists.
    let vec_list: Vec<i64> = vec![1, 9, 8, 7, 2];
    let fts_list: Vec<i64> = vec![6, 5, 4, 3, 2];
    let top = |w: f32, k: usize| rrf_fuse_weighted(&[(&vec_list, 1.0), (&fts_list, w)], k, 10)[0].chunk_id;
    assert_eq!(top(1.0, RRF_K), 2, "plain RRF: double presence wins");
    // With k = 60 even a quarter weight is not enough (1.25/65 > 1/61) ...
    assert_eq!(top(0.25, RRF_K), 2);
    // ... a small k makes the position count.
    assert_eq!(top(0.25, 10), 1);
    assert_eq!(top(1.0, 10), 2);
}
