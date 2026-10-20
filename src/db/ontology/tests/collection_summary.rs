//! Library-card numbers computed in SQL (`collection_summary.rs`): per-context
//! counts, top-by-degree concepts and the links among them, lens-aware.
use super::fixtures::{db, edge, node, seed_context_with_chunk};
use crate::db::models::NewOntologyProfile;

#[test]
fn counts_are_per_context() {
    let db = db();
    let (a, _) = seed_context_with_chunk(&db, "A");
    let (b, _) = seed_context_with_chunk(&db, "B");
    node(&db, a, "x", "T");
    node(&db, a, "y", "T");
    node(&db, b, "z", "T");
    let nodes = db.count_ontology_nodes_by_context().unwrap();
    assert_eq!((nodes[&a], nodes[&b]), (2, 1));
    let docs = db.count_documents_by_context().unwrap();
    assert_eq!((docs[&a], docs[&b]), (1, 1));
    assert_eq!(nodes[&a] as usize, db.list_ontology_nodes_for_active_lens(a).unwrap().len(), "same count as the lens-aware list");
}

#[test]
fn top_nodes_by_degree_and_links_among() {
    let db = db();
    let (ctx, chunk) = seed_context_with_chunk(&db, "A");
    let hub = node(&db, ctx, "hub", "T");
    let a = node(&db, ctx, "a", "T");
    let b = node(&db, ctx, "b", "T");
    let lone = node(&db, ctx, "lone", "T");
    edge(&db, ctx, chunk, hub.id, a.id, "R1");
    edge(&db, ctx, chunk, hub.id, b.id, "R1");
    edge(&db, ctx, chunk, a.id, hub.id, "R2"); // second, opposite-direction link hub-a
    let top = db.top_ontology_nodes_by_degree(ctx, 3).unwrap();
    let got: Vec<(i64, i64)> = top.iter().map(|n| (n.id, n.degree)).collect();
    // hub: 3 edge endpoints, a: 2, b: 1; `lone` (0) is cut by the limit.
    assert_eq!(got, vec![(hub.id, 3), (a.id, 2), (b.id, 1)]);
    let all = db.top_ontology_nodes_by_degree(ctx, 10).unwrap();
    assert_eq!(all.last().map(|n| (n.id, n.degree)), Some((lone.id, 0)));

    // Opposite-direction duplicates collapse to one undirected link.
    let ids = [hub.id, a.id, b.id];
    let mut links = db.ontology_links_among(ctx, &ids).unwrap();
    links.sort();
    let mut want = vec![(hub.id.min(a.id), hub.id.max(a.id)), (hub.id.min(b.id), hub.id.max(b.id))];
    want.sort();
    assert_eq!(links, want);
    // Only links between the given nodes; fewer than two ids → none.
    assert_eq!(db.ontology_links_among(ctx, &[hub.id, lone.id]).unwrap(), vec![]);
    assert!(db.ontology_links_among(ctx, &[hub.id]).unwrap().is_empty());
}

#[test]
fn active_lens_deleted_edges_do_not_count() {
    let db = db();
    let (ctx, chunk) = seed_context_with_chunk(&db, "A");
    let profile = db
        .create_ontology_profile(&NewOntologyProfile {
            name: "P".into(),
            entity_types_json: "[\"T\"]".into(),
            relation_types_json: "[\"R\"]".into(),
            extract_prompt: None,
            dedup_prompt: None,
            community_prompt: None,
        })
        .unwrap();
    let lens = db.get_or_create_lens(ctx, "L", profile.id, true).unwrap();
    let a = node(&db, ctx, "a", "T");
    let b = node(&db, ctx, "b", "T");
    let c = node(&db, ctx, "c", "T");
    let ab = edge(&db, ctx, chunk, a.id, b.id, "R");
    let ac = edge(&db, ctx, chunk, a.id, c.id, "R");
    db.upsert_lens_edge_verdict(lens.id, ab.id, "deleted", None).unwrap();
    db.upsert_lens_edge_verdict(lens.id, ac.id, "reversed", None).unwrap();

    // Lens not active: raw view, both edges count.
    assert_eq!(db.top_ontology_nodes_by_degree(ctx, 1).unwrap()[0].degree, 2);
    db.set_context_active_lens(ctx, Some(lens.id)).unwrap();
    let top = db.top_ontology_nodes_by_degree(ctx, 5).unwrap();
    let deg = |id: i64| top.iter().find(|n| n.id == id).unwrap().degree;
    assert_eq!((deg(a.id), deg(b.id), deg(c.id)), (1, 0, 1));
    assert_eq!(db.ontology_links_among(ctx, &[a.id, b.id, c.id]).unwrap().len(), 1);
}
