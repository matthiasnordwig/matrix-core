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

#[test]
fn get_ontology_edge_review_single_fetch_matches_list() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx13b");
    let a = node(&db, ctx, "GHG Protocol", "STANDARD");
    let b = node(&db, ctx, "Uniper", "ORGANIZATION");
    let e = edge(&db, ctx, chunk_id, a.id, b.id, "APPLIES_TO");
    db.insert_ontology_edge_review(ctx, e.id, Some(chunk_id), "APPLIES_TO", Some("quote"), "lint: self_loop: X").unwrap();

    let listed = db.list_ontology_edge_reviews(ctx).unwrap();
    assert_eq!(listed.len(), 1);
    let fetched = db.get_ontology_edge_review(listed[0].id).unwrap().expect("row must exist");
    assert_eq!(fetched.id, listed[0].id);
    assert_eq!(fetched.edge_id, e.id);
    assert_eq!(fetched.reason, "lint: self_loop: X");
    assert_eq!(fetched.source_label, "GHG Protocol");

    assert!(db.get_ontology_edge_review(999_999).unwrap().is_none());
}

#[test]
fn bulk_delete_ontology_edge_reviews_removes_only_given_ids() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx13c");
    let a = node(&db, ctx, "A", "ORGANIZATION");
    let b = node(&db, ctx, "B", "ORGANIZATION");
    let c = node(&db, ctx, "C", "ORGANIZATION");
    let e1 = edge(&db, ctx, chunk_id, a.id, b.id, "REL");
    let e2 = edge(&db, ctx, chunk_id, b.id, c.id, "REL");
    let e3 = edge(&db, ctx, chunk_id, a.id, c.id, "REL");
    db.insert_ontology_edge_review(ctx, e1.id, Some(chunk_id), "REL", None, "lint: self_loop: a").unwrap();
    db.insert_ontology_edge_review(ctx, e2.id, Some(chunk_id), "REL", None, "lint: self_loop: b").unwrap();
    db.insert_ontology_edge_review(ctx, e3.id, Some(chunk_id), "REL", None, "LLM verdict: unclear").unwrap();

    let rows = db.list_ontology_edge_reviews(ctx).unwrap();
    assert_eq!(rows.len(), 3);
    let to_delete: Vec<i64> = rows.iter().filter(|r| r.reason.starts_with("lint:")).map(|r| r.id).collect();
    assert_eq!(to_delete.len(), 2);

    let n = db.bulk_delete_ontology_edge_reviews(&to_delete).unwrap();
    assert_eq!(n, 2);

    let remaining = db.list_ontology_edge_reviews(ctx).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].reason, "LLM verdict: unclear");

    // Empty slice is a no-op, not an error.
    assert_eq!(db.bulk_delete_ontology_edge_reviews(&[]).unwrap(), 0);
}

#[test]
fn bulk_delete_ontology_edges_cascades_their_reviews() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx13d");
    let a = node(&db, ctx, "A", "ORGANIZATION");
    let b = node(&db, ctx, "B", "ORGANIZATION");
    let c = node(&db, ctx, "C", "ORGANIZATION");
    let e1 = edge(&db, ctx, chunk_id, a.id, b.id, "REL");
    let e2 = edge(&db, ctx, chunk_id, b.id, c.id, "REL");
    db.insert_ontology_edge_review(ctx, e1.id, Some(chunk_id), "REL", None, "lint: self_loop: a").unwrap();
    db.insert_ontology_edge_review(ctx, e2.id, Some(chunk_id), "REL", None, "lint: self_loop: b").unwrap();

    let n = db.bulk_delete_ontology_edges(&[e1.id, e2.id]).unwrap();
    assert_eq!(n, 2);
    assert!(db.list_ontology_edge_reviews(ctx).unwrap().is_empty());
    assert!(db.get_ontology_edge(e1.id).unwrap().is_none());
    assert!(db.get_ontology_edge(e2.id).unwrap().is_none());

    assert_eq!(db.bulk_delete_ontology_edges(&[]).unwrap(), 0);
}

#[test]
fn insert_ontology_edge_review_defaults_attempts_to_zero() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx13e");
    let a = node(&db, ctx, "A", "ORGANIZATION");
    let b = node(&db, ctx, "B", "ORGANIZATION");
    let e = edge(&db, ctx, chunk_id, a.id, b.id, "REL");
    db.insert_ontology_edge_review(ctx, e.id, Some(chunk_id), "REL", None, "lint: self_loop: a").unwrap();
    let rows = db.list_ontology_edge_reviews(ctx).unwrap();
    assert_eq!(rows[0].attempts, 0, "non-verification reviews carry attempts=0");
}

#[test]
fn upsert_verification_failure_dedups_and_increments_attempts() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx13f");
    let a = node(&db, ctx, "A", "ORGANIZATION");
    let b = node(&db, ctx, "B", "ORGANIZATION");
    let e = edge(&db, ctx, chunk_id, a.id, b.id, "REL");

    // First failure inserts a fresh row at attempts=1.
    let n1 = db.upsert_verification_failure(ctx, e.id, Some(chunk_id), "REL", Some("q"), "decode error").unwrap();
    assert_eq!(n1, 1);
    // Subsequent failures bump the SAME row instead of piling up duplicates.
    let n2 = db.upsert_verification_failure(ctx, e.id, Some(chunk_id), "REL", Some("q"), "decode error again").unwrap();
    let n3 = db.upsert_verification_failure(ctx, e.id, Some(chunk_id), "REL", Some("q"), "still failing").unwrap();
    assert_eq!((n2, n3), (2, 3));

    let rows = db.list_ontology_edge_reviews(ctx).unwrap();
    assert_eq!(rows.len(), 1, "one edge -> one verification-failure row, not three");
    assert_eq!(rows[0].attempts, 3);
    assert_eq!(rows[0].reason, "verification call failed: still failing", "reason refreshed to the latest error");
}

#[test]
fn upsert_verification_failure_does_not_touch_llm_verdict_row() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx13g");
    let a = node(&db, ctx, "A", "ORGANIZATION");
    let b = node(&db, ctx, "B", "ORGANIZATION");
    let e = edge(&db, ctx, chunk_id, a.id, b.id, "REL");
    // A genuine human-judgment "unclear" row must survive a later call-failure
    // on the same edge — they are different classes and coexist.
    db.insert_ontology_edge_review(ctx, e.id, Some(chunk_id), "REL", Some("q"), "LLM verdict: unclear").unwrap();
    db.upsert_verification_failure(ctx, e.id, Some(chunk_id), "REL", Some("q"), "decode error").unwrap();
    let rows = db.list_ontology_edge_reviews(ctx).unwrap();
    assert_eq!(rows.len(), 2, "unclear + call-failed coexist on one edge");
}

#[test]
fn update_edge_review_rewrites_reason_and_attempts() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx13h");
    let a = node(&db, ctx, "A", "ORGANIZATION");
    let b = node(&db, ctx, "B", "ORGANIZATION");
    let e = edge(&db, ctx, chunk_id, a.id, b.id, "REL");
    db.upsert_verification_failure(ctx, e.id, Some(chunk_id), "REL", None, "boom").unwrap();
    let id = db.list_ontology_edge_reviews(ctx).unwrap()[0].id;

    // A re-verify that finally returns "unclear" converts the failure row into a
    // genuine human-judgment item and carries the attempts count over.
    db.update_edge_review(id, "LLM verdict: unclear", 2).unwrap();
    let rows = db.list_ontology_edge_reviews(ctx).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].reason, "LLM verdict: unclear");
    assert_eq!(rows[0].attempts, 2);
}

// --- lenses (non-destructive schema materialization) -----------------------

#[test]
fn raw_type_mirrors_type_at_insert_and_manual_edit() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx15");
    let a = node(&db, ctx, "Bundesrepublik Deutschland", "COUNTRY");
    assert_eq!(a.entity_type, "COUNTRY");
    assert_eq!(a.raw_entity_type, "COUNTRY");

    db.update_ontology_node(a.id, "Bundesrepublik Deutschland", "STATE", "", &[]).unwrap();
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
