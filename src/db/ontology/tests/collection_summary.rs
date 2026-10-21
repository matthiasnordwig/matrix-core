//! Library-card numbers (`collection_summary.rs`): per-context counts,
//! concepts by degree, links and the connected miniature selection, lens-aware.
use super::fixtures::{db, edge, node, seed_context_with_chunk};
use crate::db::collection_summary::{grow_preview_selection, PreviewNode};
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
fn top_nodes_by_degree_and_links() {
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
    let mut want = vec![(hub.id.min(a.id), hub.id.max(a.id)), (hub.id.min(b.id), hub.id.max(b.id))];
    want.sort();
    assert_eq!(db.ontology_links(ctx).unwrap(), want);
    // The miniature: `lone` has no link and is left out; links only among the picked.
    let (nodes, links) = db.preview_graph(ctx, 10).unwrap();
    assert_eq!(nodes.iter().map(|n| n.id).collect::<Vec<_>>(), vec![hub.id, a.id, b.id]);
    assert_eq!(links, want);
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
    assert_eq!(db.ontology_links(ctx).unwrap().len(), 1);
}

#[test]
fn community_slot_follows_the_active_lens_order() {
    let db = db();
    let (ctx, _) = seed_context_with_chunk(&db, "A");
    let a = node(&db, ctx, "a", "T");
    let b = node(&db, ctx, "b", "T");
    let lone = node(&db, ctx, "lone", "T");
    // Raw-view communities (lens NULL, the context has no active lens): slots by id.
    let c0 = db.create_ontology_community(ctx, "first", 1, "", None, None).unwrap();
    let c1 = db.create_ontology_community(ctx, "second", 1, "", None, None).unwrap();
    db.set_community_members(c0, &[b.id]).unwrap();
    db.set_community_members(c1, &[a.id]).unwrap();
    let top = db.top_ontology_nodes_by_degree(ctx, 10).unwrap();
    let slot = |id: i64| top.iter().find(|n| n.id == id).unwrap().community;
    assert_eq!((slot(b.id), slot(a.id), slot(lone.id)), (Some(0), Some(1), None));
    // In two communities → the later one's slot, like the graph view's map.
    db.set_community_members(c0, &[a.id, b.id]).unwrap();
    let top = db.top_ontology_nodes_by_degree(ctx, 10).unwrap();
    assert_eq!(top.iter().find(|n| n.id == a.id).unwrap().community, Some(1));
}

fn pn(ids_by_degree: &[(i64, i64)]) -> Vec<PreviewNode> {
    ids_by_degree.iter().map(|&(id, degree)| PreviewNode { id, degree, community: None }).collect()
}

#[test]
fn selection_grows_connected_from_the_strongest_hub() {
    // Hub 1 with leaves 2..=5; a separate triangle 10-11-12; pairs 20-21, 30-31 with high degree elsewhere.
    let nodes = pn(&[(20, 9), (1, 4), (10, 2), (11, 2), (12, 2), (2, 1), (3, 1), (4, 1), (5, 1), (21, 1), (30, 1), (31, 1)]);
    let links = [(1, 2), (1, 3), (1, 4), (1, 5), (10, 11), (11, 12), (10, 12), (20, 21), (30, 31)];
    // Pairs never seed; the star comes first, then the triangle.
    assert_eq!(grow_preview_selection(&nodes, &links, 100), vec![1, 10, 11, 12, 2, 3, 4, 5]);
    // Budget 6: the star (5) plus one triangle concept, which is a piece < 3 and dropped.
    assert_eq!(grow_preview_selection(&nodes, &links, 6), vec![1, 2, 3, 4, 5]);
    // Deterministic.
    assert_eq!(grow_preview_selection(&nodes, &links, 7), grow_preview_selection(&nodes, &links, 7));
}

#[test]
fn selection_prefers_concepts_linked_into_the_shape() {
    // After 1 and 2, concept 3 (linked to both) comes before the more connected 4 (linked to 1 only).
    let nodes = pn(&[(1, 3), (2, 2), (4, 2), (3, 2), (5, 1)]);
    let links = [(1, 2), (1, 3), (2, 3), (1, 4), (4, 5)];
    assert_eq!(&grow_preview_selection(&nodes, &links, 3)[..], &[1, 2, 3]);
}

#[test]
fn selection_without_shapes_takes_the_first_concepts() {
    let nodes = pn(&[(1, 1), (2, 1), (3, 0)]);
    assert_eq!(grow_preview_selection(&nodes, &[(1, 2)], 2), vec![1, 2]);
    assert_eq!(grow_preview_selection(&nodes, &[], 5), vec![1, 2, 3]);
}
