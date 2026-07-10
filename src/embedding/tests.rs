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

fn model(db: &Database, ident: &str, dim: i64) -> EmbeddingModel {
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
    })
    .unwrap()
}

fn context_with_vectors(
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
