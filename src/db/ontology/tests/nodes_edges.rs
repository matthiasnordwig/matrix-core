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

    db.update_ontology_node_type(n.id, "ORG").unwrap();
    assert_eq!(db.get_ontology_nodes_raw(ctx).unwrap()[0].1, "ORG");

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
