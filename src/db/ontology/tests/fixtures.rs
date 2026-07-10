//! Shared test fixtures for the ontology/GraphRAG DB test suite (split out of
//! the former flat tests.rs once it approached the ~600-line threshold —
//! see CLAUDE.md's tests.rs splitting convention). Mirrors the fixture style
//! of `db::tests`.
use crate::db::models::*;
use crate::db::Database;

pub(super) fn db() -> Database {
    Database::open_in_memory().expect("open in-memory db")
}

/// Minimal context (bound to a real 2-dim embedding model, needed for
/// `retrieve_graph_with`/`retrieve_graph_batch`, which skip contexts with no
/// model via `contexts_by_model`) + one chunk (ontology_edges/edge_chunks
/// require a real chunk_id). `name` must be unique per call within the same
/// database (both `contexts.name` and `embedding_models.identifier` are
/// UNIQUE). In a fresh in-memory db, the first call's model gets id 1 —
/// tests with a single `seed_context_with_chunk` call rely on that for their
/// hardcoded `query_by_model` key.
pub(super) fn seed_context_with_chunk(db: &Database, name: &str) -> (i64, i64) {
    let model = db
        .create_embedding_model(&NewEmbeddingModel {
            identifier: format!("test-embed-{name}"),
            kind: ModelKind::LocalOnnx,
            model_path: Some("/models/test.onnx".into()),
            tokenizer_path: None,
            api_config: None,
            execution_provider: Some(ExecutionProvider::Ane),
            is_matryoshka: false,
            native_dim: 2,
            default_dim: 2,
            normalize: true,
            tpm_limit: None,
            rpm_limit: None,
            max_concurrency: 1,
        })
        .unwrap();
    let ctx = db
        .create_context(&NewContext {
            name: name.into(),
            description: None,
            chunking_profile_id: None,
            embedding_model_id: Some(model.id),
            embedding_dim: Some(2),
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
            name: "doc.pdf".into(),
            zip_entry: None,
            byte_size: None,
            page_count: None,
            content_hash: None,
            extracted_text: None,
        })
        .unwrap();
    let chunk = db
        .create_chunk(&NewChunk {
            context_id: ctx.id,
            document_id: doc.id,
            chunk_index: 0,
            char_start: 0,
            char_end: 1,
            text: "chunk".into(),
            signature: None,
            is_omitted: false,
            metadata: "{}".into(),
        })
        .unwrap();
    (ctx.id, chunk.id)
}

pub(super) fn node(db: &Database, ctx: i64, label: &str, entity_type: &str) -> OntologyNode {
    db.create_ontology_node(&NewOntologyNode {
        context_id: ctx,
        label: label.into(),
        entity_type: entity_type.into(),
        description: String::new(),
    })
    .unwrap()
}

pub(super) fn edge(db: &Database, ctx: i64, chunk_id: i64, source: i64, target: i64, rel: &str) -> OntologyEdge {
    db.create_ontology_edge(&NewOntologyEdge {
        context_id: ctx,
        source_id: source,
        target_id: target,
        relation_type: rel.into(),
        chunk_id,
        evidence: None,
    })
    .unwrap()
}
