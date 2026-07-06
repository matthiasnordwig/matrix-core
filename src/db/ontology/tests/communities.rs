//! Per-lens communities + lazy summary cache (schema_v37):
//! `ontology_communities.lens_id`/`members_key` and
//! `ontology_community_members`. See `communities.rs`.
use crate::db::embeddings::vector_to_blob;
use crate::db::models::*;
use super::fixtures::{db, node, seed_context_with_chunk};

fn lens(db: &crate::db::Database, ctx: i64, name: &str) -> i64 {
    let profile = db
        .create_ontology_profile(&NewOntologyProfile {
            name: format!("profile-{name}"),
            entity_types_json: "[]".into(),
            relation_types_json: "[]".into(),
            extract_prompt: None,
            dedup_prompt: None,
            community_prompt: None,
        })
        .unwrap();
    db.get_or_create_lens(ctx, name, profile.id, false).unwrap().id
}

#[test]
fn per_lens_crud_round_trip() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "CtxComm1");
    let a = node(&db, ctx, "A", "CONCEPT");
    let b = node(&db, ctx, "B", "CONCEPT");
    let lens_a = lens(&db, ctx, "lens-a");
    let lens_b = lens(&db, ctx, "lens-b");

    let ca = db.create_ontology_community(ctx, "Comm A", 2, "sum a", Some(lens_a), Some("1,2")).unwrap();
    db.set_community_members(ca, &[b.id, a.id]).unwrap(); // unsorted on purpose
    let cb = db.create_ontology_community(ctx, "Comm B", 1, "sum b", Some(lens_b), Some("1")).unwrap();
    db.set_community_members(cb, &[a.id]).unwrap();
    let craw = db.create_ontology_community(ctx, "Comm Raw", 1, "sum raw", None, None).unwrap();
    db.set_community_members(craw, &[b.id]).unwrap();

    // Each lens (and the NULL raw view) sees only its own rows.
    let for_a = db.list_communities_for_lens(ctx, Some(lens_a)).unwrap();
    assert_eq!(for_a.len(), 1);
    assert_eq!(for_a[0].id, ca);
    assert_eq!(for_a[0].lens_id, Some(lens_a));
    assert_eq!(for_a[0].members_key.as_deref(), Some("1,2"));
    assert_eq!(for_a[0].member_ids, vec![a.id, b.id], "member ids come back sorted");

    let for_b = db.list_communities_for_lens(ctx, Some(lens_b)).unwrap();
    assert_eq!(for_b.len(), 1);
    assert_eq!(for_b[0].id, cb);

    let for_raw = db.list_communities_for_lens(ctx, None).unwrap();
    assert_eq!(for_raw.len(), 1);
    assert_eq!(for_raw[0].id, craw);
    assert_eq!(for_raw[0].lens_id, None);

    // The lens-agnostic listing still returns everything (export path).
    assert_eq!(db.list_ontology_communities(ctx).unwrap().len(), 3);
    let mut pairs = db.list_community_members_for_context(ctx).unwrap();
    pairs.sort_unstable();
    let mut expected = vec![(ca, a.id), (ca, b.id), (cb, a.id), (craw, b.id)];
    expected.sort_unstable();
    assert_eq!(pairs, expected);
}

#[test]
fn set_community_members_replaces_previous_set() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "CtxComm2");
    let a = node(&db, ctx, "A", "CONCEPT");
    let b = node(&db, ctx, "B", "CONCEPT");
    let c = db.create_ontology_community(ctx, "Comm", 2, "s", None, None).unwrap();
    db.set_community_members(c, &[a.id, b.id]).unwrap();
    db.set_community_members(c, &[b.id]).unwrap();
    let listed = db.list_communities_for_lens(ctx, None).unwrap();
    assert_eq!(listed[0].member_ids, vec![b.id], "second set fully replaces the first");
}

#[test]
fn members_key_cache_is_per_lens_and_skips_empty_summaries() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "CtxComm3");
    let lens_a = lens(&db, ctx, "lens-a");
    let lens_b = lens(&db, ctx, "lens-b");

    let ca = db.create_ontology_community(ctx, "Comm A", 2, "cached summary", Some(lens_a), Some("1,2")).unwrap();
    db.update_community_vector(ca, &vector_to_blob(&[0.5, 0.5])).unwrap();
    // Same member set under lens B, but with an EMPTY summary (recompute miss).
    db.create_ontology_community(ctx, "Comm B", 2, "", Some(lens_b), Some("1,2")).unwrap();

    // Hit: same lens + same key, summary and vector come back.
    let hit = db.find_community_by_members_key(ctx, Some(lens_a), "1,2").unwrap();
    let (summary, vector) = hit.expect("cache hit expected");
    assert_eq!(summary, "cached summary");
    assert!(vector.is_some());

    // Cache key is (lens_id, members_key), NOT members_key alone: the same
    // node set under another lens must not reuse lens A's summary
    // (architecture-review 2026-07-06). Lens B's own row is skipped because
    // its summary is empty.
    assert!(db.find_community_by_members_key(ctx, Some(lens_b), "1,2").unwrap().is_none());
    assert!(db.find_community_by_members_key(ctx, None, "1,2").unwrap().is_none());
    // Different key, no hit.
    assert!(db.find_community_by_members_key(ctx, Some(lens_a), "1,3").unwrap().is_none());
}

#[test]
fn delete_communities_for_lens_leaves_other_lenses_as_cache() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "CtxComm4");
    let lens_a = lens(&db, ctx, "lens-a");
    let lens_b = lens(&db, ctx, "lens-b");
    db.create_ontology_community(ctx, "A1", 2, "sa", Some(lens_a), Some("1,2")).unwrap();
    db.create_ontology_community(ctx, "B1", 2, "sb", Some(lens_b), Some("1,2")).unwrap();
    db.create_ontology_community(ctx, "R1", 2, "sr", None, Some("1,2")).unwrap();

    db.delete_communities_for_lens(ctx, Some(lens_a)).unwrap();
    assert!(db.list_communities_for_lens(ctx, Some(lens_a)).unwrap().is_empty());
    assert_eq!(db.list_communities_for_lens(ctx, Some(lens_b)).unwrap().len(), 1, "other lens's cache must survive");
    assert_eq!(db.list_communities_for_lens(ctx, None).unwrap().len(), 1, "raw view must survive");

    db.delete_communities_for_lens(ctx, None).unwrap();
    assert!(db.list_communities_for_lens(ctx, None).unwrap().is_empty());
    assert_eq!(db.list_communities_for_lens(ctx, Some(lens_b)).unwrap().len(), 1);

    // Full wipe still clears everything.
    db.delete_communities_for_context(ctx).unwrap();
    assert!(db.list_ontology_communities(ctx).unwrap().is_empty());
}

#[test]
fn member_rows_cascade_on_node_and_lens_delete() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "CtxComm5");
    let a = node(&db, ctx, "A", "CONCEPT");
    let b = node(&db, ctx, "B", "CONCEPT");
    let lens_a = lens(&db, ctx, "lens-a");
    let c = db.create_ontology_community(ctx, "Comm", 2, "s", Some(lens_a), Some("1,2")).unwrap();
    db.set_community_members(c, &[a.id, b.id]).unwrap();

    // Node delete (dedup merge hard-deletes the losing row the same way)
    // silently shrinks membership via ON DELETE CASCADE.
    db.delete_ontology_node(a.id).unwrap();
    let listed = db.list_communities_for_lens(ctx, Some(lens_a)).unwrap();
    assert_eq!(listed[0].member_ids, vec![b.id]);

    // Lens delete cascades the whole community rows (and their members).
    db.delete_lens(lens_a).unwrap();
    assert!(db.list_communities_for_lens(ctx, Some(lens_a)).unwrap().is_empty());
    assert!(db.list_community_members_for_context(ctx).unwrap().is_empty());
}

#[test]
fn list_communities_missing_vector_skips_vectored_and_empty_summary_rows() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "CtxComm6");
    let with_vec = db.create_ontology_community(ctx, "HasVec", 2, "s1", None, Some("1,2")).unwrap();
    db.update_community_vector(with_vec, &vector_to_blob(&[1.0, 0.0])).unwrap();
    let missing = db.create_ontology_community(ctx, "NoVec", 2, "s2", None, Some("3,4")).unwrap();
    db.create_ontology_community(ctx, "EmptySummary", 2, "", None, Some("5,6")).unwrap();

    let rows = db.list_communities_missing_vector(ctx).unwrap();
    assert_eq!(rows.len(), 1, "only the summarized-but-unvectored row needs embedding");
    assert_eq!(rows[0].id, missing);
}
