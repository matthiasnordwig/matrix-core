//! Graph-vector provenance (schema_v67): vectors of another embedding space
//! never take part in a comparison, count as missing and are dropped by
//! `clear_graph_vectors_for_context` (the model-change hook).
use crate::db::embeddings::vector_to_blob;
use crate::db::models::*;
use crate::db::Database;
use super::fixtures::{db, model_of, node, seed_context_with_chunk};

/// Rebinds the context to a fresh 2-dim model (what a "Search model" change
/// does to `contexts.embedding_model_id`) and returns the new model id.
fn switch_model(db: &Database, ctx: i64, ident: &str) -> i64 {
    let m = db
        .create_embedding_model(&NewEmbeddingModel {
            identifier: ident.into(),
            kind: ModelKind::LocalOnnx,
            model_path: Some("/models/other.onnx".into()),
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
    db.conn.execute("UPDATE contexts SET embedding_model_id = ?1 WHERE id = ?2", [m.id, ctx]).unwrap();
    m.id
}

#[test]
fn foreign_space_vectors_give_no_hits_and_count_as_missing() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "GvForeign");
    let old = model_of(&db, ctx);
    let a = node(&db, ctx, "A", "CONCEPT");
    db.update_ontology_node_vector(a.id, &vector_to_blob(&[1.0, 0.0]), old).unwrap();
    let co = db.create_ontology_community(ctx, "C", 1, "summary", None, Some("1")).unwrap();
    db.update_community_vector(co, &vector_to_blob(&[1.0, 0.0]), old).unwrap();
    assert_eq!(db.node_vector_counts(ctx).unwrap(), (1, 1));
    assert_eq!(db.community_vector_counts(ctx).unwrap(), (1, 1));

    // Same dimension, other space — the measured jina/embeddinggemma case.
    let new = switch_model(&db, ctx, "GvForeign-new");
    assert!(db.search_ontology_nodes_semantic(ctx, new, &[1.0, 0.0], 10).unwrap().is_empty());
    let mut q = std::collections::HashMap::new();
    q.insert(new, vec![1.0f32, 0.0]);
    let g = db.retrieve_graph_with(&[ctx], &q, 5, 1, 5).unwrap();
    assert!(g.nodes.is_empty() && g.community_summaries.is_empty(), "{g:?}");
    let batch = db.retrieve_graph_batch(&[ctx], &[q], 5, 1, 5).unwrap();
    assert!(batch[0].nodes.is_empty() && batch[0].community_summaries.is_empty());
    assert!(db.get_ontology_nodes_with_embeddings_and_lens_type(ctx).unwrap().is_empty(), "dedup must not mix spaces");
    assert_eq!(db.count_ontology_nodes_with_embeddings(ctx).unwrap(), 0);
    assert!(db.community_vector_blobs(ctx).unwrap().is_empty(), "export must not ship foreign vectors");

    // ... counts as missing (re-embedding picks it up) and the summary cache
    // hands back the summary without the foreign vector.
    assert_eq!(db.get_ontology_nodes_missing_embeddings(ctx).unwrap().len(), 1);
    assert_eq!(db.list_communities_missing_vector(ctx).unwrap().len(), 1);
    let (summary, vector) = db.find_community_by_members_key(ctx, None, "1").unwrap().unwrap();
    assert_eq!(summary, "summary");
    assert!(vector.is_none());
    assert_eq!(db.node_vector_counts(ctx).unwrap(), (1, 0));
    assert_eq!(db.community_vector_counts(ctx).unwrap(), (1, 0));

    // Re-embedded with the new model it is searchable again.
    db.update_ontology_node_vector(a.id, &vector_to_blob(&[1.0, 0.0]), new).unwrap();
    assert_eq!(db.search_ontology_nodes_semantic(ctx, new, &[1.0, 0.0], 10).unwrap()[0].0, a.id);
    // ... and the query model must match: the old model's query finds nothing.
    assert!(db.search_ontology_nodes_semantic(ctx, old, &[1.0, 0.0], 10).unwrap().is_empty());
}

#[test]
fn clear_graph_vectors_drops_node_and_community_vectors_only_of_that_context() {
    let db = db();
    let (ctx, _c1) = seed_context_with_chunk(&db, "GvClear");
    let (other, _c2) = seed_context_with_chunk(&db, "GvKeep");
    let a = node(&db, ctx, "A", "CONCEPT");
    let b = node(&db, other, "B", "CONCEPT");
    db.update_ontology_node_vector(a.id, &vector_to_blob(&[1.0, 0.0]), model_of(&db, ctx)).unwrap();
    db.update_ontology_node_vector(b.id, &vector_to_blob(&[1.0, 0.0]), model_of(&db, other)).unwrap();
    let co = db.create_ontology_community(ctx, "C", 1, "summary", None, Some("1")).unwrap();
    db.update_community_vector(co, &vector_to_blob(&[1.0, 0.0]), model_of(&db, ctx)).unwrap();

    assert_eq!(db.clear_graph_vectors_for_context(ctx).unwrap(), (1, 1));
    assert_eq!(db.node_vector_counts(ctx).unwrap(), (1, 0));
    assert_eq!(db.community_vector_counts(ctx).unwrap(), (1, 0));
    assert_eq!(db.node_vector_counts(other).unwrap(), (1, 1), "other contexts untouched");
    let (blob, model): (Option<Vec<u8>>, Option<i64>) = db
        .conn
        .query_row("SELECT vector_blob, vector_model_id FROM ontology_nodes WHERE id = ?1", [a.id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert!(blob.is_none() && model.is_none());
    assert_eq!(db.clear_graph_vectors_for_context(ctx).unwrap(), (0, 0), "idempotent");
}
