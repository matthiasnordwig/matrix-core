use crate::db::models::*;
use crate::db::embeddings::vector_to_blob;
use super::fixtures::{db, edge, node, seed_context_with_chunk};

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

    assert_eq!(db.get_ontology_nodes_raw(ctx).unwrap()[0].1, "PERSON", "raw type mirrors type at insert");

    assert!(db.get_ontology_nodes_missing_embeddings(ctx).unwrap().iter().any(|(id, _, _)| *id == n.id));
    assert_eq!(db.count_ontology_nodes_with_embeddings(ctx).unwrap(), 0);
    db.update_ontology_node_vector(n.id, &vector_to_blob(&[1.0, 0.0])).unwrap();
    assert!(db.get_ontology_nodes_missing_embeddings(ctx).unwrap().is_empty());
    assert_eq!(db.get_ontology_nodes_with_embeddings(ctx).unwrap().len(), 1);
    assert_eq!(db.count_ontology_nodes_with_embeddings(ctx).unwrap(), 1);

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

    let executed = db.merge_ontology_nodes(&[(keep.id, dup.id)]).unwrap();
    assert_eq!(executed, vec![(keep.id, dup.id)]);

    let edges = db.list_ontology_edges(ctx).unwrap();
    assert_eq!(edges.len(), 1, "duplicate edge must collapse into one");
    assert_eq!(edges[0].source_id, keep.id);

    let nodes = db.list_ontology_nodes(ctx).unwrap();
    assert_eq!(nodes.len(), 2, "the dropped duplicate node itself must be gone");
    assert!(nodes.iter().all(|n| n.id != dup.id));
}

#[test]
fn get_ontology_node_single_fetch_matches_list_shape() {
    let db = db();
    let (ctx, _chunk_id) = seed_context_with_chunk(&db, "CtxGetNode");
    let n = node(&db, ctx, "Uniper", "ORGANIZATION");

    let fetched = db.get_ontology_node(n.id).unwrap().expect("node must exist");
    assert_eq!(fetched.id, n.id);
    assert_eq!(fetched.label, "Uniper");
    assert_eq!(fetched.entity_type, "ORGANIZATION");

    assert!(db.get_ontology_node(999_999).unwrap().is_none());
}

#[test]
fn get_ontology_nodes_with_types_returns_entity_type_not_description() {
    // Regression: the structural-lint step (ontology/mod.rs, "1b") loads
    // node_labels as (label, entity_type) and the LabelEqualsType rule
    // compares label against that second field. It must be entity_type, not
    // the free-text description — a node whose description happens to equal
    // its label (but whose type differs) must NOT look like label == type.
    let db = db();
    let (ctx, _chunk_id) = seed_context_with_chunk(&db, "CtxNodeTypes");
    let n = db
        .create_ontology_node(&NewOntologyNode {
            context_id: ctx,
            label: "Uniper".into(),
            entity_type: "ORGANIZATION".into(),
            description: "Uniper".into(), // description == label on purpose
        })
        .unwrap();

    let types = db.get_ontology_nodes_with_types(ctx).unwrap();
    assert_eq!(
        types.get(&n.id),
        Some(&("Uniper".to_string(), "ORGANIZATION".to_string())),
        "second field must be entity_type, not description"
    );

    // Contrast: the description variant yields the description in that slot.
    let descs = db.get_ontology_nodes_with_descriptions(ctx).unwrap();
    assert_eq!(descs.get(&n.id), Some(&("Uniper".to_string(), "Uniper".to_string())));
}

#[test]
fn get_ontology_edge_single_fetch_matches_list_shape() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "CtxGetEdge");
    let a = node(&db, ctx, "A", "ORG");
    let b = node(&db, ctx, "B", "ORG");
    let e = edge(&db, ctx, chunk_id, a.id, b.id, "REL");

    let fetched = db.get_ontology_edge(e.id).unwrap().expect("edge must exist");
    assert_eq!(fetched.id, e.id);
    assert_eq!(fetched.source_id, a.id);
    assert_eq!(fetched.target_id, b.id);
    assert_eq!(fetched.relation_type, "REL");
    assert_eq!(fetched.chunk_evidences.get(&chunk_id), Some(&None), "evidence round-trips same as list_ontology_edges");

    assert!(db.get_ontology_edge(999_999).unwrap().is_none());
}

#[test]
fn list_for_active_lens_applies_type_resolution_reversal_and_deletion() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "CtxLens");
    let profile = db
        .create_ontology_profile(&NewOntologyProfile {
            name: "Lens".into(),
            entity_types_json: "[\"PERSON\"]".into(),
            relation_types_json: "[\"KNOWS\"]".into(),
            extract_prompt: None,
            dedup_prompt: None,
            community_prompt: None,
        })
        .unwrap();
    let a = node(&db, ctx, "Alice", "ACTOR");
    let b = node(&db, ctx, "Bob", "ACTOR");
    let c = node(&db, ctx, "Carol", "ACTOR");
    let keep = edge(&db, ctx, chunk_id, a.id, b.id, "linked_to");
    let rev = edge(&db, ctx, chunk_id, b.id, c.id, "linked_to");
    let del = edge(&db, ctx, chunk_id, a.id, c.id, "linked_to");

    let lens = db.get_or_create_lens(ctx, "Lens", profile.id, true).unwrap();
    db.set_context_active_lens(ctx, Some(lens.id)).unwrap();
    db.upsert_lens_node_type(lens.id, a.id, "PERSON").unwrap();
    db.upsert_lens_edge_verdict(lens.id, keep.id, "valid", Some("KNOWS")).unwrap();
    db.upsert_lens_edge_verdict(lens.id, rev.id, "reversed", Some("KNOWS")).unwrap();
    db.upsert_lens_edge_verdict(lens.id, del.id, "deleted", None).unwrap();

    // Raw listing ignores the lens entirely (this is what the Ontology tab
    // used to call — hence "always the raw graph").
    let raw_nodes = db.list_ontology_nodes(ctx).unwrap();
    assert_eq!(raw_nodes.iter().find(|n| n.id == a.id).unwrap().entity_type, "ACTOR");
    assert_eq!(db.list_ontology_edges(ctx).unwrap().len(), 3, "raw view keeps deleted edges");

    // Lens-aware listing resolves the type, swaps the reversed edge, drops
    // the deleted one.
    let nodes = db.list_ontology_nodes_for_active_lens(ctx).unwrap();
    assert_eq!(nodes.iter().find(|n| n.id == a.id).unwrap().entity_type, "PERSON", "resolved type");
    assert_eq!(nodes.iter().find(|n| n.id == b.id).unwrap().entity_type, "ACTOR", "no lens row -> raw fallback");

    let edges = db.list_ontology_edges_for_active_lens(ctx).unwrap();
    assert_eq!(edges.len(), 2, "deleted edge excluded");
    let rev_shown = edges.iter().find(|e| e.id == rev.id).unwrap();
    assert_eq!(rev_shown.source_id, c.id, "reversed: source/target swapped for display");
    assert_eq!(rev_shown.target_id, b.id);
    assert_eq!(rev_shown.relation_type, "KNOWS", "resolved relation type");
    assert!(!edges.iter().any(|e| e.id == del.id));
}

#[test]
fn list_for_active_lens_with_no_lens_matches_raw_view() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "CtxNoLens");
    let a = node(&db, ctx, "Alice", "ACTOR");
    let b = node(&db, ctx, "Bob", "ACTOR");
    edge(&db, ctx, chunk_id, a.id, b.id, "linked_to");
    // active_lens_id stays NULL: lens-aware listing must equal the raw view.
    let lens_nodes = db.list_ontology_nodes_for_active_lens(ctx).unwrap();
    assert_eq!(lens_nodes.iter().find(|n| n.id == a.id).unwrap().entity_type, "ACTOR");
    assert_eq!(db.list_ontology_edges_for_active_lens(ctx).unwrap().len(), 1);
}

#[test]
fn merge_dropping_an_edge_between_the_two_nodes_leaves_no_self_loop() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "CtxLoop");
    let keep = node(&db, ctx, "Uniper", "CORPORATION");
    let dup = node(&db, ctx, "Uniper Group", "CORPORATION");
    let other = node(&db, ctx, "Bob", "PERSON");
    // An edge connecting the two soon-to-be-merged nodes becomes keep->keep
    // after rewiring — that self-loop must be cleaned up, not persisted.
    edge(&db, ctx, chunk_id, keep.id, dup.id, "OWNS");
    // A normal edge on the winner must survive untouched.
    edge(&db, ctx, chunk_id, keep.id, other.id, "EMPLOYS");

    db.merge_ontology_nodes(&[(keep.id, dup.id)]).unwrap();

    let edges = db.list_ontology_edges(ctx).unwrap();
    assert!(!edges.iter().any(|e| e.source_id == e.target_id), "no self-loop must remain");
    assert_eq!(edges.len(), 1, "only the unrelated edge survives");
    assert_eq!(edges[0].relation_type, "EMPLOYS");
}

#[test]
fn merge_with_missing_winner_is_skipped_not_an_error() {
    // Regression for the production FK abort: a stale dedup candidate can
    // elect an already-deleted node as winner. The merge must skip that pair
    // (and report it as not executed) instead of failing the transaction —
    // with foreign_keys=ON, rewiring edges onto a dead id is an FK error
    // that would roll back every merge in the batch.
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx4");
    let dead = node(&db, ctx, "Ghost", "ORG");
    let alive = node(&db, ctx, "Survivor", "ORG");
    let other = node(&db, ctx, "Bystander", "PERSON");
    edge(&db, ctx, chunk_id, alive.id, other.id, "RELATED_TO");
    db.delete_ontology_node(dead.id).unwrap();

    let executed = db.merge_ontology_nodes(&[(dead.id, alive.id)]).unwrap();

    assert!(executed.is_empty(), "pair with dead winner must be skipped");
    let nodes = db.list_ontology_nodes(ctx).unwrap();
    assert!(nodes.iter().any(|n| n.id == alive.id), "loser must survive a skipped merge");
    assert_eq!(db.list_ontology_edges(ctx).unwrap().len(), 1, "its edges must be untouched");
}

#[test]
fn merge_ontology_nodes_logs_loser_label_and_type_before_deleting_it() {
    // Regression for ISSUES.md "ontology_dedup_cache — keine Merge-Historie
    // ...": merge_ontology_nodes hard-deletes the losing node row, so its
    // label/type must be captured in ontology_merge_log *before* the DELETE,
    // in the same transaction, or the history is unrecoverable.
    let db = db();
    let (ctx, _chunk_id) = seed_context_with_chunk(&db, "CtxMergeLog");
    let keep = node(&db, ctx, "OpenAI", "ORG");
    let dup = node(&db, ctx, "Open AI", "ORG");

    let executed = db.merge_ontology_nodes(&[(keep.id, dup.id)]).unwrap();
    assert_eq!(executed, vec![(keep.id, dup.id)]);

    let log = db.list_ontology_merge_log(ctx).unwrap();
    assert_eq!(log.len(), 1, "exactly one merge log row");
    let entry = &log[0];
    assert_eq!(entry.context_id, ctx);
    assert_eq!(entry.winner_id, keep.id);
    assert_eq!(entry.loser_id, dup.id);
    assert_eq!(entry.loser_label, "Open AI");
    assert_eq!(entry.loser_entity_type, "ORG");

    // The log must survive the loser's deletion, not merely predate it.
    let nodes = db.list_ontology_nodes(ctx).unwrap();
    assert!(nodes.iter().all(|n| n.id != dup.id), "loser node must actually be gone");
    assert!(nodes.iter().any(|n| n.id == keep.id), "winner must survive");
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

    db.invert_ontology_edge(e.id).unwrap();
    let edges = db.list_ontology_edges(ctx).unwrap();
    assert_eq!(edges[0].source_id, b.id, "invert swaps source and target");
    assert_eq!(edges[0].target_id, a.id);

    db.invert_ontology_edge(e.id).unwrap();
    assert_eq!(db.list_ontology_edges(ctx).unwrap()[0].source_id, a.id, "inverting again flips it back");

    assert_eq!(db.get_ontology_edges(ctx).unwrap()[0].1, "RELATED_TO", "relation type unchanged by inversion");

    db.delete_ontology_edge(e.id).unwrap();
    assert!(db.list_ontology_edges(ctx).unwrap().is_empty());
}

#[test]
fn get_ontology_edge_id_resolves_natural_key() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx4b");
    let a = node(&db, ctx, "A", "CONCEPT");
    let b = node(&db, ctx, "B", "CONCEPT");

    assert!(db.get_ontology_edge_id(ctx, a.id, b.id, "RELATED_TO").unwrap().is_none());

    let e = edge(&db, ctx, chunk_id, a.id, b.id, "RELATED_TO");
    let found = db.get_ontology_edge_id(ctx, a.id, b.id, "RELATED_TO").unwrap();
    assert_eq!(found, Some(e.id));

    // relation_type match is case-insensitive, mirroring create_ontology_edge's own lookup.
    let found_ci = db.get_ontology_edge_id(ctx, a.id, b.id, "related_to").unwrap();
    assert_eq!(found_ci, Some(e.id));
}

#[test]
fn add_ontology_edge_chunk_with_evidence_inserts_and_updates() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx4c");
    let a = node(&db, ctx, "A", "CONCEPT");
    let b = node(&db, ctx, "B", "CONCEPT");
    let e = edge(&db, ctx, chunk_id, a.id, b.id, "RELATED_TO");

    // Second evidence chunk, created purely via create_document/create_chunk
    // helpers (seed_context_with_chunk only makes one chunk).
    let doc = db.list_documents(ctx).unwrap().remove(0);
    let chunk2 = db
        .create_chunk(&NewChunk {
            context_id: ctx,
            document_id: doc.id,
            chunk_index: 1,
            char_start: 1,
            char_end: 2,
            text: "chunk2".into(),
            signature: None,
            is_omitted: false,
            metadata: "{}".into(),
        })
        .unwrap();

    db.add_ontology_edge_chunk_with_evidence(e.id, chunk2.id, Some("proof")).unwrap();
    let edges = db.list_ontology_edges(ctx).unwrap();
    let reloaded = edges.iter().find(|x| x.id == e.id).unwrap();
    assert_eq!(reloaded.chunk_evidences.get(&chunk2.id), Some(&Some("proof".to_string())));

    // Re-inserting with None must not clobber the existing evidence (COALESCE).
    db.add_ontology_edge_chunk_with_evidence(e.id, chunk2.id, None).unwrap();
    let edges = db.list_ontology_edges(ctx).unwrap();
    let reloaded = edges.iter().find(|x| x.id == e.id).unwrap();
    assert_eq!(reloaded.chunk_evidences.get(&chunk2.id), Some(&Some("proof".to_string())));
}

// --- communities (incl. the Uniper2 community-coloring regression) ---------

#[test]
fn delete_communities_resets_node_community_id_to_null() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "Ctx5");
    let n = node(&db, ctx, "A", "CONCEPT");
    let comm_id = db.create_ontology_community(ctx, "Cluster 1", 1, "summary", None, None).unwrap();
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
