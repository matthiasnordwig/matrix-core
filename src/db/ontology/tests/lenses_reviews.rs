use crate::db::models::*;
use super::fixtures::{db, edge, node, seed_context_with_chunk};

// --- edge reviews (non-blocking polarity-check flags) -----------------------

#[test]
fn edge_review_round_trip() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx13");
    let a = node(&db, ctx, "GHG Protocol", "STANDARD");
    let b = node(&db, ctx, "Uniper", "ORGANIZATION");
    let e = edge(&db, ctx, chunk_id, a.id, b.id, "APPLIES_TO");

    assert!(db.list_ontology_edge_reviews(ctx).unwrap().is_empty());

    db.insert_ontology_edge_review(
        ctx,
        e.id,
        Some(chunk_id),
        "APPLIES_TO",
        Some("entspricht derzeit nicht dem GHG Protocol"),
        "LLM: unclear",
    )
    .unwrap();

    let rows = db.list_ontology_edge_reviews(ctx).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].edge_id, e.id);
    assert_eq!(rows[0].relation_type, "APPLIES_TO");
    assert_eq!(rows[0].evidence.as_deref(), Some("entspricht derzeit nicht dem GHG Protocol"));
    assert_eq!(rows[0].reason, "LLM: unclear");
    assert_eq!(rows[0].source_label, "GHG Protocol", "source/target labels must be joined in from the edge's nodes");
    assert_eq!(rows[0].target_label, "Uniper");

    let review_id = rows[0].id;
    db.delete_ontology_edge_review(review_id).unwrap();
    assert!(db.list_ontology_edge_reviews(ctx).unwrap().is_empty());
}

#[test]
fn edge_review_cascade_deletes_when_edge_is_deleted() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx14");
    let a = node(&db, ctx, "GHG Protocol", "STANDARD");
    let b = node(&db, ctx, "Uniper", "ORGANIZATION");
    let e = edge(&db, ctx, chunk_id, a.id, b.id, "APPLIES_TO");

    db.insert_ontology_edge_review(ctx, e.id, Some(chunk_id), "APPLIES_TO", None, "LLM: unclear").unwrap();
    assert_eq!(db.list_ontology_edge_reviews(ctx).unwrap().len(), 1);

    db.delete_ontology_edge(e.id).unwrap();
    assert!(db.list_ontology_edge_reviews(ctx).unwrap().is_empty());
}

// --- lenses (non-destructive schema materialization) -----------------------

#[test]
fn raw_type_mirrors_type_at_insert_and_manual_edit() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx15");
    let a = node(&db, ctx, "Bundesrepublik Deutschland", "COUNTRY");
    assert_eq!(a.entity_type, "COUNTRY");
    assert_eq!(a.raw_entity_type, "COUNTRY");

    db.update_ontology_node_type(a.id, "STATE").unwrap();
    let reloaded = db.list_ontology_nodes(ctx).unwrap().into_iter().find(|n| n.id == a.id).unwrap();
    assert_eq!(reloaded.entity_type, "STATE");
    assert_eq!(reloaded.raw_entity_type, "STATE", "manual edit must update raw type too, not just the display type");

    let b = node(&db, ctx, "Uniper", "ORGANIZATION");
    let e = edge(&db, ctx, chunk_id, a.id, b.id, "LOCATED_IN");
    assert_eq!(e.raw_relation_type, "LOCATED_IN");
    db.update_ontology_edge(e.id, "HEADQUARTERED_IN").unwrap();
    let reloaded_edges = db.list_ontology_edges(ctx).unwrap();
    let e2 = reloaded_edges.iter().find(|x| x.id == e.id).unwrap();
    assert_eq!(e2.relation_type, "HEADQUARTERED_IN");
    assert_eq!(e2.raw_relation_type, "HEADQUARTERED_IN");
}

#[test]
fn get_or_create_lens_updates_in_place_for_same_profile() {
    let db = db();
    let (ctx, _) = seed_context_with_chunk(&db, "Ctx16");
    let profile = db
        .create_ontology_profile(&NewOntologyProfile {
            name: "Compliance".into(),
            entity_types_json: "[\"ORGANIZATION\"]".into(),
            relation_types_json: "[\"RELATED_TO\"]".into(),
            extract_prompt: None,
            dedup_prompt: None,
            community_prompt: None,
        })
        .unwrap();

    let lens1 = db.get_or_create_lens(ctx, "Compliance", profile.id, false).unwrap();
    assert!(!lens1.is_extraction_lens);

    // Re-running for the same (context, profile) refreshes in place — same
    // id, not a second row — and is_extraction_lens only ever turns true.
    let lens2 = db.get_or_create_lens(ctx, "Compliance", profile.id, true).unwrap();
    assert_eq!(lens1.id, lens2.id);
    assert!(lens2.is_extraction_lens);
    assert_eq!(db.list_lenses_for_context(ctx).unwrap().len(), 1);

    let lens3 = db.get_or_create_lens(ctx, "Compliance", profile.id, false).unwrap();
    assert!(lens3.is_extraction_lens, "is_extraction_lens must not flip back to false");
}

#[test]
fn deleting_active_lens_falls_back_context_to_raw() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx17");
    let profile = db
        .create_ontology_profile(&NewOntologyProfile {
            name: "Compliance".into(),
            entity_types_json: "[\"ORGANIZATION\"]".into(),
            relation_types_json: "[\"RELATED_TO\"]".into(),
            extract_prompt: None,
            dedup_prompt: None,
            community_prompt: None,
        })
        .unwrap();
    let lens = db.get_or_create_lens(ctx, "Compliance", profile.id, true).unwrap();

    let a = node(&db, ctx, "Bundesrepublik Deutschland", "COUNTRY");
    let b = node(&db, ctx, "Uniper", "ORGANIZATION");
    let e = edge(&db, ctx, chunk_id, a.id, b.id, "REGULATES");
    db.upsert_lens_node_type(lens.id, a.id, "GOVERNMENT_BODY").unwrap();
    db.upsert_lens_edge_verdict(lens.id, e.id, "valid", Some("REGULATES")).unwrap();
    db.set_context_active_lens(ctx, Some(lens.id)).unwrap();
    assert_eq!(db.context(ctx).unwrap().unwrap().active_lens_id, Some(lens.id));

    db.delete_lens(lens.id).unwrap();
    assert_eq!(db.context(ctx).unwrap().unwrap().active_lens_id, None, "active_lens_id must fall back to NULL, not dangle or error");
    assert!(db.list_lenses_for_context(ctx).unwrap().is_empty());
}

#[test]
fn upsert_lens_edge_verdict_preserves_resolved_type_when_only_verdict_changes() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx18");
    let profile = db
        .create_ontology_profile(&NewOntologyProfile {
            name: "Compliance".into(),
            entity_types_json: "[\"ORGANIZATION\"]".into(),
            relation_types_json: "[\"RELATED_TO\"]".into(),
            extract_prompt: None,
            dedup_prompt: None,
            community_prompt: None,
        })
        .unwrap();
    let lens = db.get_or_create_lens(ctx, "Compliance", profile.id, true).unwrap();
    db.set_context_active_lens(ctx, Some(lens.id)).unwrap();
    let a = node(&db, ctx, "GHG Protocol", "STANDARD");
    let b = node(&db, ctx, "Uniper", "ORGANIZATION");
    let e = edge(&db, ctx, chunk_id, a.id, b.id, "APPLIES_TO");

    // materialize_lens's normal write: resolved type + valid verdict.
    db.upsert_lens_edge_verdict(lens.id, e.id, "valid", Some("APPLIES_TO")).unwrap();
    // verify_edge_polarity's write: only flips the verdict, no resolved type.
    db.upsert_lens_edge_verdict(lens.id, e.id, "deleted", None).unwrap();

    let edges_for_lens = db.get_ontology_edges_full_for_lens(ctx).unwrap();
    assert!(edges_for_lens.is_empty(), "a 'deleted' verdict must exclude the edge from get_ontology_edges_full_for_lens");
}
