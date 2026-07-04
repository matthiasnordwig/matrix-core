//! CRUD + retrieval tests for the ontology/GraphRAG schema, all against an
//! in-memory database. Mirrors the fixture style of `db::tests`.
use crate::db::models::*;
use crate::db::{embeddings::vector_to_blob, Database};

fn db() -> Database {
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
fn seed_context_with_chunk(db: &Database, name: &str) -> (i64, i64) {
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

fn node(db: &Database, ctx: i64, label: &str, entity_type: &str) -> OntologyNode {
    db.create_ontology_node(&NewOntologyNode {
        context_id: ctx,
        label: label.into(),
        entity_type: entity_type.into(),
        description: String::new(),
    })
    .unwrap()
}

fn edge(db: &Database, ctx: i64, chunk_id: i64, source: i64, target: i64, rel: &str) -> OntologyEdge {
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

// --- profiles -----------------------------------------------------------

#[test]
fn ontology_profile_crud_round_trip() {
    let db = db();
    let created = db
        .create_ontology_profile(&NewOntologyProfile {
            name: "Legal".into(),
            entity_types_json: "[\"PERSON\"]".into(),
            relation_types_json: "[\"RELATED_TO\"]".into(),
            extract_prompt: None,
            dedup_prompt: None,
            community_prompt: None,
        })
        .unwrap();
    assert_eq!(db.list_ontology_profiles().unwrap().len(), 1);

    let updated = db
        .update_ontology_profile(
            created.id,
            &NewOntologyProfile {
                name: "Legal v2".into(),
                entity_types_json: "[\"PERSON\",\"ORG\"]".into(),
                relation_types_json: "[\"RELATED_TO\"]".into(),
                extract_prompt: Some("prompt".into()),
                dedup_prompt: None,
                community_prompt: None,
            },
        )
        .unwrap();
    assert_eq!(updated.name, "Legal v2");
    assert_eq!(db.ontology_profile(created.id).unwrap().unwrap().name, "Legal v2");

    assert!(db.delete_ontology_profile(created.id).unwrap());
    assert!(db.ontology_profile(created.id).unwrap().is_none());
}

// --- nodes ----------------------------------------------------------------

#[test]
fn node_crud_and_lookups() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "Ctx1");

    let n = node(&db, ctx, "Alice", "PERSON");
    assert_eq!(db.list_ontology_nodes(ctx).unwrap().len(), 1);
    assert_eq!(
        db.get_ontology_node_id_by_label_fast(ctx, "alice").unwrap(),
        Some(n.id),
        "label lookup is case-insensitive"
    );

    db.update_ontology_node_type(n.id, "ORG").unwrap();
    assert_eq!(db.get_ontology_nodes(ctx).unwrap()[0].1, "ORG");

    assert!(db.get_ontology_nodes_missing_embeddings(ctx).unwrap().iter().any(|(id, _, _)| *id == n.id));
    db.update_ontology_node_vector(n.id, &vector_to_blob(&[1.0, 0.0])).unwrap();
    assert!(db.get_ontology_nodes_missing_embeddings(ctx).unwrap().is_empty());
    assert_eq!(db.get_ontology_nodes_with_embeddings(ctx).unwrap().len(), 1);

    db.update_ontology_node_community(n.id, Some(42)).unwrap();
    assert_eq!(db.list_ontology_nodes(ctx).unwrap()[0].community_id, Some(42));

    db.delete_ontology_node(n.id).unwrap();
    assert!(db.list_ontology_nodes(ctx).unwrap().is_empty());
}

#[test]
fn semantic_search_ranks_nearest_node() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "Ctx2");
    let a = node(&db, ctx, "A", "CONCEPT");
    let b = node(&db, ctx, "B", "CONCEPT");
    db.update_ontology_node_vector(a.id, &vector_to_blob(&[1.0, 0.0])).unwrap();
    db.update_ontology_node_vector(b.id, &vector_to_blob(&[0.0, 1.0])).unwrap();

    let hits = db.search_ontology_nodes_semantic(ctx, &[0.9, 0.1], 1).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, a.id);
}

#[test]
fn merge_ontology_nodes_rewires_edges_and_drops_duplicate() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx3");
    let keep = node(&db, ctx, "OpenAI", "ORG");
    let dup = node(&db, ctx, "Open AI", "ORG");
    let other = node(&db, ctx, "Sam Altman", "PERSON");

    // Both the kept and the duplicate node point at the same target via the
    // same relation — merging must not create a second, redundant edge.
    edge(&db, ctx, chunk_id, keep.id, other.id, "FOUNDED_BY");
    edge(&db, ctx, chunk_id, dup.id, other.id, "FOUNDED_BY");

    db.merge_ontology_nodes(&[(keep.id, dup.id)]).unwrap();

    let edges = db.list_ontology_edges(ctx).unwrap();
    assert_eq!(edges.len(), 1, "duplicate edge must collapse into one");
    assert_eq!(edges[0].source_id, keep.id);

    let nodes = db.list_ontology_nodes(ctx).unwrap();
    assert_eq!(nodes.len(), 2, "the dropped duplicate node itself must be gone");
    assert!(nodes.iter().all(|n| n.id != dup.id));
}

// --- edges ------------------------------------------------------------------

#[test]
fn edge_crud_and_curation() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx4");
    let a = node(&db, ctx, "A", "CONCEPT");
    let b = node(&db, ctx, "B", "CONCEPT");

    let e = edge(&db, ctx, chunk_id, a.id, b.id, "RELATED_TO");
    assert_eq!(e.chunk_evidences.len(), 1);
    assert!(e.chunk_evidences.contains_key(&chunk_id));

    let counts = db.get_node_edge_counts(ctx).unwrap();
    assert_eq!(counts[&a.id], 1);
    assert_eq!(counts[&b.id], 1);

    db.reverse_ontology_edge(e.id).unwrap();
    let edges = db.list_ontology_edges(ctx).unwrap();
    assert_eq!(edges[0].source_id, b.id);
    assert_eq!(edges[0].target_id, a.id);

    db.invert_ontology_edge(e.id).unwrap();
    let edges = db.list_ontology_edges(ctx).unwrap();
    assert_eq!(edges[0].source_id, a.id, "invert flips it back");

    db.update_ontology_edge_type(e.id, "PART_OF").unwrap();
    assert_eq!(db.get_ontology_edges(ctx).unwrap()[0].1, "PART_OF");

    db.delete_ontology_edge(e.id).unwrap();
    assert!(db.list_ontology_edges(ctx).unwrap().is_empty());
}

// --- communities (incl. the Uniper2 community-coloring regression) ---------

#[test]
fn delete_communities_resets_node_community_id_to_null() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "Ctx5");
    let n = node(&db, ctx, "A", "CONCEPT");
    let comm_id = db.create_ontology_community(ctx, "Cluster 1", 1, "summary").unwrap();
    db.update_ontology_node_community(n.id, Some(comm_id)).unwrap();
    assert_eq!(db.list_ontology_nodes(ctx).unwrap()[0].community_id, Some(comm_id));

    db.delete_communities_for_context(ctx).unwrap();

    assert!(db.list_ontology_communities(ctx).unwrap().is_empty());
    assert_eq!(
        db.list_ontology_nodes(ctx).unwrap()[0].community_id,
        None,
        "nodes must fall back to NULL (grey), never keep a stale community_id"
    );
}

// --- lifecycle ----------------------------------------------------------------

#[test]
fn delete_ontology_for_context_only_touches_that_context() {
    let db = db();
    let (ctx_a, chunk_a) = seed_context_with_chunk(&db, "Ctx6");
    let (ctx_b, chunk_b) = seed_context_with_chunk(&db, "Ctx7");
    let a1 = node(&db, ctx_a, "A1", "CONCEPT");
    let a2 = node(&db, ctx_a, "A2", "CONCEPT");
    edge(&db, ctx_a, chunk_a, a1.id, a2.id, "RELATED_TO");
    let b1 = node(&db, ctx_b, "B1", "CONCEPT");
    let b2 = node(&db, ctx_b, "B2", "CONCEPT");
    edge(&db, ctx_b, chunk_b, b1.id, b2.id, "RELATED_TO");
    db.insert_extracted_chunk(ctx_a, chunk_a).unwrap();
    db.insert_extracted_chunk(ctx_b, chunk_b).unwrap();

    db.delete_ontology_for_context(ctx_a).unwrap();

    assert!(db.list_ontology_nodes(ctx_a).unwrap().is_empty());
    assert!(db.list_ontology_edges(ctx_a).unwrap().is_empty());
    assert!(db.get_chunks_with_ontology(ctx_a).unwrap().is_empty());

    assert_eq!(db.list_ontology_nodes(ctx_b).unwrap().len(), 2, "other context must survive");
    assert_eq!(db.list_ontology_edges(ctx_b).unwrap().len(), 1);
    assert!(db.get_chunks_with_ontology(ctx_b).unwrap().contains(&chunk_b));
}

// --- retrieval (recursive-CTE hop expansion) --------------------------------

#[test]
fn retrieve_graph_with_expands_hops_and_stops_at_limit() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx8");
    // A -> B -> C, three hops apart from the query's perspective (A is the hit).
    let a = node(&db, ctx, "A", "CONCEPT");
    let b = node(&db, ctx, "B", "CONCEPT");
    let c = node(&db, ctx, "C", "CONCEPT");
    // Only A points at the query direction — B/C are orthogonal, so with
    // top_k_nodes=1 only A seeds the recursive-CTE hop expansion below.
    db.update_ontology_node_vector(a.id, &vector_to_blob(&[1.0, 0.0])).unwrap();
    db.update_ontology_node_vector(b.id, &vector_to_blob(&[0.0, 1.0])).unwrap();
    db.update_ontology_node_vector(c.id, &vector_to_blob(&[0.0, 1.0])).unwrap();
    edge(&db, ctx, chunk_id, a.id, b.id, "RELATED_TO");
    edge(&db, ctx, chunk_id, b.id, c.id, "RELATED_TO");

    let model_id = 1i64;
    let mut query = std::collections::HashMap::new();
    query.insert(model_id, vec![1.0f32, 0.0]);

    // 1 hop: reaches A (the hit) and B, not C.
    let result = db.retrieve_graph_with(&[ctx], &query, 1, 1, 5).unwrap();
    assert!(result.nodes.iter().any(|n| n.starts_with("A:")));
    assert!(result.nodes.iter().any(|n| n.starts_with("B:")));
    assert!(!result.nodes.iter().any(|n| n.starts_with("C:")), "C is 2 hops away, must not appear at hops=1");

    let result_2hop = db.retrieve_graph_with(&[ctx], &query, 1, 2, 5).unwrap();
    assert!(result_2hop.nodes.iter().any(|n| n.starts_with("C:")), "C must appear once hops=2");
}

#[test]
fn retrieve_graph_batch_matches_single_query_result() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx9");
    let a = node(&db, ctx, "A", "CONCEPT");
    let b = node(&db, ctx, "B", "CONCEPT");
    db.update_ontology_node_vector(a.id, &vector_to_blob(&[1.0, 0.0])).unwrap();
    db.update_ontology_node_vector(b.id, &vector_to_blob(&[0.0, 1.0])).unwrap();
    edge(&db, ctx, chunk_id, a.id, b.id, "RELATED_TO");

    let model_id = 1i64;
    let mut q1 = std::collections::HashMap::new();
    q1.insert(model_id, vec![1.0f32, 0.0]);
    let mut q2 = std::collections::HashMap::new();
    q2.insert(model_id, vec![0.0f32, 1.0]);

    let batch = db.retrieve_graph_batch(&[ctx], &[q1, q2], 1, 1, 5).unwrap();
    assert_eq!(batch.len(), 2);
    assert!(batch[0].nodes.iter().any(|n| n.starts_with("A:")));
    assert!(batch[1].nodes.iter().any(|n| n.starts_with("B:")));
}

// --- metrics (rolling last-3-runs window) -----------------------------------

#[test]
fn phase_metrics_keep_only_last_three_runs() {
    let db = db();
    for ms in [100.0, 200.0, 300.0, 400.0] {
        db.insert_phase_metric("extract", "model-a", ms).unwrap();
    }
    let avg = db.get_phase_averages("model-a").unwrap()["extract"];
    // Only the last 3 (200, 300, 400) must survive the rolling window.
    assert!((avg - 300.0).abs() < 1e-6, "expected avg of last 3 runs, got {avg}");
}

// --- dedup cache --------------------------------------------------------------

#[test]
fn dedup_cache_round_trip() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "Ctx10");
    db.cache_dedup_decision(ctx, 1, 2, true).unwrap();
    db.cache_dedup_decision(ctx, 3, 4, false).unwrap();

    let cache = db.get_dedup_cache(ctx).unwrap();
    assert_eq!(cache[&(1, 2)], true);
    assert_eq!(cache[&(3, 4)], false);
}

// --- quarantine + chunk-state resumability ----------------------------------

#[test]
fn quarantine_round_trip() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx11");
    db.insert_quarantined_chunk(ctx, chunk_id, "{\"nodes\":[]}", "JSON parse error").unwrap();

    let rows = db.get_quarantined_chunks(ctx).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].error_reason, "JSON parse error");

    db.delete_quarantined_chunk(chunk_id).unwrap();
    assert!(db.get_quarantined_chunks(ctx).unwrap().is_empty());
}

#[test]
fn chunk_state_save_load_delete_round_trip() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx12");
    assert_eq!(db.load_chunk_state(ctx, chunk_id).unwrap(), None);

    db.save_chunk_state(ctx, chunk_id, "[0,1]", "{\"nodes\":[]}").unwrap();
    assert_eq!(
        db.load_chunk_state(ctx, chunk_id).unwrap(),
        Some(("[0,1]".to_string(), "{\"nodes\":[]}".to_string()))
    );

    // Upsert overwrites, doesn't duplicate.
    db.save_chunk_state(ctx, chunk_id, "[0,1,2]", "{\"nodes\":[1]}").unwrap();
    assert_eq!(
        db.load_chunk_state(ctx, chunk_id).unwrap(),
        Some(("[0,1,2]".to_string(), "{\"nodes\":[1]}".to_string()))
    );

    db.delete_chunk_state(ctx, chunk_id).unwrap();
    assert_eq!(db.load_chunk_state(ctx, chunk_id).unwrap(), None);
}
