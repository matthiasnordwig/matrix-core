//! Tests for `transfer.rs`: schema introspection, per-context row counts
//! (direct and one FK hop), exact inserts and the never-reused reserved
//! node id.

use crate::db::models::*;
use crate::db::Database;

fn ctx(db: &Database, name: &str) -> i64 {
    db.create_context(&NewContext {
        name: name.into(),
        description: None,
        chunking_strategy: "structural".into(),
        chunking_profile_id: None,
        structural_profile_id: None,
        embedding_model_id: None,
        embedding_dim: None,
        llm_id: None,
        fallback_llm_id: None,
        ontology_profile_id: None,
        ontology_pool_id: None,
        ontology_extract_llm_id: None,
        ontology_extract_pool_id: None,
        ontology_extract_reasoning_effort: None,
        extract_title_llm: false,
        auto_merge_ontology: false,
    })
    .unwrap()
    .id
}

#[test]
fn schema_tables_reports_columns_foreign_keys_and_virtual_tables() {
    let db = Database::open_in_memory().unwrap();
    let tables = db.schema_tables().unwrap();
    let get = |n: &str| tables.iter().find(|t| t.name == n).unwrap_or_else(|| panic!("{n} missing"));
    let chunks = get("chunks");
    assert!(chunks.columns.iter().any(|c| c == "context_id"));
    assert!(chunks.foreign_keys.contains(&("document_id".to_string(), "documents".to_string())));
    let ec = get("ontology_edge_chunks");
    assert!(ec.foreign_keys.contains(&("edge_id".to_string(), "ontology_edges".to_string())));
    assert!(get("chunks_fts").is_virtual);
    assert!(!chunks.is_virtual);
    assert!(tables.iter().all(|t| !t.name.starts_with("sqlite_")));
}

#[test]
fn count_context_rows_direct_and_via_parent() {
    let db = Database::open_in_memory().unwrap();
    let a = ctx(&db, "A");
    let b = ctx(&db, "B");
    let na = db.insert_ontology_node_exact(a, "X", "Org", "Organisation", "d").unwrap();
    let nb = db.insert_ontology_node_exact(a, "Y", "Org", "Org", "d").unwrap();
    db.insert_ontology_node_exact(b, "Z", "Org", "Org", "d").unwrap();
    let e = db.insert_ontology_edge_exact(a, na, nb, "owns", "holds").unwrap();
    let edges = db.list_ontology_edges(a).unwrap();
    assert_eq!((edges[0].id, edges[0].relation_type.as_str(), edges[0].raw_relation_type.as_str()), (e, "owns", "holds"));
    let nodes = db.list_ontology_nodes(a).unwrap();
    assert!(nodes.iter().any(|n| n.label == "X" && n.raw_entity_type == "Organisation"));

    let profile = db
        .create_ontology_profile(&NewOntologyProfile {
            name: "P".into(),
            entity_types_json: "[]".into(),
            relation_types_json: "[]".into(),
            extract_prompt: None,
            dedup_prompt: None,
            community_prompt: None,
        })
        .unwrap();
    let lens = db.insert_ontology_lens_exact(a, "P", profile.id, true, 1_700_000_000).unwrap();
    db.upsert_lens_node_type(lens, na, "Organisation").unwrap();
    db.upsert_lens_edge_verdict(lens, e, "reversed", Some("owned_by")).unwrap();
    assert_eq!(db.list_lenses_for_context(a).unwrap()[0].created_at, 1_700_000_000);
    assert_eq!(db.lens_node_types(lens).unwrap(), vec![(na, "Organisation".to_string())]);
    assert_eq!(db.lens_edge_verdicts(lens).unwrap(), vec![(e, "reversed".to_string(), Some("owned_by".to_string()))]);

    assert_eq!(db.count_context_rows("contexts", a).unwrap(), 1);
    assert_eq!(db.count_context_rows("ontology_nodes", a).unwrap(), 2);
    assert_eq!(db.count_context_rows("ontology_nodes", b).unwrap(), 1);
    assert_eq!(db.count_context_rows("ontology_lens_node_types", a).unwrap(), 1);
    assert_eq!(db.count_context_rows("ontology_lens_edge_verdicts", b).unwrap(), 0);
    assert!(db.count_context_rows("no_such_table", a).is_err());
    assert!(db.count_context_rows("app_settings", a).is_err());
}

#[test]
fn reserved_node_id_is_never_reused_and_exact_rows_keep_their_values() {
    let db = Database::open_in_memory().unwrap();
    let a = ctx(&db, "A");
    let reserved = db.reserve_ontology_node_id(a).unwrap();
    assert!(db.list_ontology_nodes(a).unwrap().is_empty());
    let next = db.insert_ontology_node_exact(a, "N", "T", "T", "").unwrap();
    assert!(next > reserved);

    db.insert_ontology_merge_log_exact(&MergeLogEntry {
        id: 0,
        context_id: a,
        winner_id: next,
        loser_id: reserved,
        loser_label: "Old".into(),
        loser_entity_type: "T".into(),
        merged_at: 42,
    })
    .unwrap();
    let log = db.list_ontology_merge_log(a).unwrap();
    assert_eq!((log[0].winner_id, log[0].loser_id, log[0].merged_at), (next, reserved, 42));

    let m = db.insert_ontology_node_exact(a, "M", "T", "T", "").unwrap();
    let e = db.insert_ontology_edge_exact(a, next, m, "r", "r").unwrap();
    db.insert_ontology_edge_review_exact(&OntologyEdgeReview {
        id: 0,
        context_id: a,
        edge_id: e,
        chunk_id: None,
        relation_type: "r".into(),
        evidence: Some("ev".into()),
        reason: "lint: x".into(),
        created_at: 7,
        attempts: 2,
        source_label: String::new(),
        target_label: String::new(),
    })
    .unwrap();
    let r = &db.list_ontology_edge_reviews(a).unwrap()[0];
    assert_eq!((r.created_at, r.attempts, r.source_label.as_str()), (7, 2, "N"));
}
