use crate::db::models::*;
use super::fixtures::db;

fn make_model(db: &crate::db::Database, identifier: &str) -> i64 {
    db.create_embedding_model(&NewEmbeddingModel {
        identifier: identifier.into(),
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
    .unwrap()
    .id
}

#[test]
fn upsert_and_get_round_trip() {
    let db = db();
    let model_id = make_model(&db, "model-a");

    db.upsert_type_vector(model_id, "CORPORATION", &[1.0, 0.5]).unwrap();
    db.upsert_type_vector(model_id, "PERSON", &[0.0, 1.0]).unwrap();

    let got = db.get_type_vectors(model_id, &["CORPORATION".to_string(), "PERSON".to_string()]).unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got["CORPORATION"], vec![1.0, 0.5]);
    assert_eq!(got["PERSON"], vec![0.0, 1.0]);
}

#[test]
fn missing_raw_types_are_simply_absent_not_an_error() {
    let db = db();
    let model_id = make_model(&db, "model-b");
    db.upsert_type_vector(model_id, "CORPORATION", &[1.0, 0.0]).unwrap();

    let got = db.get_type_vectors(model_id, &["CORPORATION".to_string(), "NEVER_CACHED".to_string()]).unwrap();
    assert_eq!(got.len(), 1);
    assert!(got.contains_key("CORPORATION"));
    assert!(!got.contains_key("NEVER_CACHED"));
}

#[test]
fn empty_raw_types_list_returns_empty_map_without_querying() {
    let db = db();
    let model_id = make_model(&db, "model-c");
    let got = db.get_type_vectors(model_id, &[]).unwrap();
    assert!(got.is_empty());
}

#[test]
fn same_raw_type_is_isolated_per_embedding_model() {
    let db = db();
    let model_a = make_model(&db, "model-d1");
    let model_b = make_model(&db, "model-d2");

    db.upsert_type_vector(model_a, "CORPORATION", &[1.0, 0.0]).unwrap();
    db.upsert_type_vector(model_b, "CORPORATION", &[0.0, 1.0]).unwrap();

    let got_a = db.get_type_vectors(model_a, &["CORPORATION".to_string()]).unwrap();
    let got_b = db.get_type_vectors(model_b, &["CORPORATION".to_string()]).unwrap();
    assert_eq!(got_a["CORPORATION"], vec![1.0, 0.0]);
    assert_eq!(got_b["CORPORATION"], vec![0.0, 1.0]);
}

#[test]
fn upsert_replaces_existing_vector_for_same_key() {
    let db = db();
    let model_id = make_model(&db, "model-e");
    db.upsert_type_vector(model_id, "CORPORATION", &[1.0, 0.0]).unwrap();
    db.upsert_type_vector(model_id, "CORPORATION", &[0.0, 1.0]).unwrap();

    let got = db.get_type_vectors(model_id, &["CORPORATION".to_string()]).unwrap();
    assert_eq!(got["CORPORATION"], vec![0.0, 1.0], "second upsert must replace, not duplicate");
}

#[test]
fn cascade_deletes_when_embedding_model_is_deleted() {
    let db = db();
    let model_id = make_model(&db, "model-f");
    db.upsert_type_vector(model_id, "CORPORATION", &[1.0, 0.0]).unwrap();
    assert_eq!(db.get_type_vectors(model_id, &["CORPORATION".to_string()]).unwrap().len(), 1);

    db.delete_embedding_model(model_id).unwrap();

    // Querying a deleted model_id is a bit artificial (nothing should be
    // there), but the real guarantee is that the row is actually gone from
    // the table — check via a fresh model reusing the cache table directly
    // isn't possible without raw SQL, so re-create a model with the same id
    // is not guaranteed; instead assert via get_type_vectors on the same
    // (now-nonexistent) model_id returns empty, confirming no orphaned row.
    let got = db.get_type_vectors(model_id, &["CORPORATION".to_string()]).unwrap();
    assert!(got.is_empty(), "cache row must cascade-delete with its embedding model");
}
