use crate::db::models::*;
use crate::db::embeddings::vector_to_blob;
use super::fixtures::{db, edge, node, seed_context_with_chunk};

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

    // 1 hop: reaches A (the hit) and B, not C. Node format is
    // "label (type): description" — type comes from the active lens
    // (falling back to raw, here NULL active_lens_id since this context has
    // no lens).
    let result = db.retrieve_graph_with(&[ctx], &query, 1, 1, 5).unwrap();
    assert!(result.nodes.iter().any(|n| n.starts_with("A (CONCEPT):")));
    assert!(result.nodes.iter().any(|n| n.starts_with("B (CONCEPT):")));
    assert!(!result.nodes.iter().any(|n| n.starts_with("C (CONCEPT):")), "C is 2 hops away, must not appear at hops=1");

    let result_2hop = db.retrieve_graph_with(&[ctx], &query, 1, 2, 5).unwrap();
    assert!(result_2hop.nodes.iter().any(|n| n.starts_with("C (CONCEPT):")), "C must appear once hops=2");
}

#[test]
fn retrieve_graph_with_applies_active_lens_type_reversal_and_deletion() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx9b");
    let profile = db
        .create_ontology_profile(&NewOntologyProfile {
            name: "Compliance".into(),
            entity_types_json: "[\"PERSON\"]".into(),
            relation_types_json: "[\"REVERSED_REL\"]".into(),
            extract_prompt: None,
            dedup_prompt: None,
            community_prompt: None,
        })
        .unwrap();
    let lens = db.get_or_create_lens(ctx, "Compliance", profile.id, true).unwrap();
    db.set_context_active_lens(ctx, Some(lens.id)).unwrap();

    // A -> B -> C. A's active-lens type is snapped to PERSON. The A->B edge
    // is verdicted "reversed" (display should show B -> A). The B->C edge is
    // verdicted "deleted" — it must not be traversed at all, so C should be
    // unreachable even at hops=2.
    let a = node(&db, ctx, "A", "CONCEPT");
    let b = node(&db, ctx, "B", "CONCEPT");
    let c = node(&db, ctx, "C", "CONCEPT");
    db.update_ontology_node_vector(a.id, &vector_to_blob(&[1.0, 0.0])).unwrap();
    db.update_ontology_node_vector(b.id, &vector_to_blob(&[0.0, 1.0])).unwrap();
    db.update_ontology_node_vector(c.id, &vector_to_blob(&[0.0, 1.0])).unwrap();
    let ab = edge(&db, ctx, chunk_id, a.id, b.id, "RAW_REL");
    let bc = edge(&db, ctx, chunk_id, b.id, c.id, "RAW_REL");

    db.upsert_lens_node_type(lens.id, a.id, "PERSON").unwrap();
    db.upsert_lens_edge_verdict(lens.id, ab.id, "reversed", Some("REVERSED_REL")).unwrap();
    db.upsert_lens_edge_verdict(lens.id, bc.id, "deleted", Some("RAW_REL")).unwrap();

    let model_id = 1i64;
    let mut query = std::collections::HashMap::new();
    query.insert(model_id, vec![1.0f32, 0.0]);

    let result = db.retrieve_graph_with(&[ctx], &query, 1, 2, 5).unwrap();
    assert!(result.nodes.iter().any(|n| n.starts_with("A (PERSON):")), "A's displayed type must come from the active lens, not the raw type");
    assert!(!result.nodes.iter().any(|n| n.starts_with("C")), "C is only reachable via the lens-deleted B->C edge, must not appear");
    assert!(result.edges.iter().any(|e| e == "B -[REVERSED_REL]-> A"), "a 'reversed' verdict must swap displayed source/target and use the resolved relation type; got {:?}", result.edges);
    assert!(!result.edges.iter().any(|e| e.contains("RAW_REL")), "the raw relation type must not leak through once a lens resolves it");
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
    assert!(batch[0].nodes.iter().any(|n| n.starts_with("A (CONCEPT):")));
    assert!(batch[1].nodes.iter().any(|n| n.starts_with("B (CONCEPT):")));
}
