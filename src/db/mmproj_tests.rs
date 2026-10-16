//! Image projector of local vision models (schema_v65): `llm_endpoints.mmproj_path`
//! — create/update round-trip, the narrow relocation setter, serde default for
//! JSON that predates the column, and the migration on an existing v64 database.

use super::llm_endpoint_tests::seed_endpoint;
use super::models::*;
use super::vision_tests::migrate_to;
use super::Database;

fn spec_of(e: &LlmEndpoint) -> NewLlmEndpoint {
    serde_json::from_value(serde_json::to_value(e).unwrap()).unwrap()
}

#[test]
fn mmproj_path_round_trips_through_create_and_update() {
    let db = Database::open_in_memory().unwrap();
    let id = seed_endpoint(&db, "local", "gguf");
    let created = db.llm_endpoint(id).unwrap().unwrap();
    assert_eq!(created.mmproj_path, None, "default: text only");

    let mut spec = spec_of(&created);
    spec.mmproj_path = Some("/m/mmproj-x.gguf".into());
    spec.supports_vision = true;
    let updated = db.update_llm_endpoint(id, &spec).unwrap();
    assert_eq!(updated.mmproj_path.as_deref(), Some("/m/mmproj-x.gguf"));

    spec.name = "second".into();
    assert_eq!(db.create_llm_endpoint(&spec).unwrap().mmproj_path.as_deref(), Some("/m/mmproj-x.gguf"));
    spec.name = "local".into();
    spec.mmproj_path = None;
    assert_eq!(db.update_llm_endpoint(id, &spec).unwrap().mmproj_path, None, "update clears it");
}

#[test]
fn mmproj_path_setter_touches_nothing_else() {
    let db = Database::open_in_memory().unwrap();
    let id = seed_endpoint(&db, "local", "gguf");
    let before = db.llm_endpoint(id).unwrap().unwrap();
    db.set_llm_endpoint_mmproj_path(id, "/new/mmproj.gguf").unwrap();
    let after = db.llm_endpoint(id).unwrap().unwrap();
    assert_eq!(after.mmproj_path.as_deref(), Some("/new/mmproj.gguf"));
    assert_eq!((after.base_url, after.name, after.context_window), (before.base_url, before.name, before.context_window));
}

#[test]
fn json_predating_v65_deserializes_without_projector() {
    let db = Database::open_in_memory().unwrap();
    let id = seed_endpoint(&db, "ep", "openai");
    let mut v = serde_json::to_value(db.llm_endpoint(id).unwrap().unwrap()).unwrap();
    v.as_object_mut().unwrap().remove("mmproj_path");
    assert_eq!(serde_json::from_value::<LlmEndpoint>(v.clone()).unwrap().mmproj_path, None);
    assert_eq!(serde_json::from_value::<NewLlmEndpoint>(v).unwrap().mmproj_path, None);
}

#[test]
fn v65_applies_on_an_existing_v64_database() {
    let conn = migrate_to(64);
    conn.execute_batch(
        "INSERT INTO llm_endpoints (id, name, base_url, model_id, provider, supports_vision)
             VALUES (1, 'old', '/m/model.gguf', 'model.gguf', 'gguf', 0);",
    )
    .unwrap();
    let db = Database::init(conn).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);
    let ep = db.llm_endpoint(1).unwrap().unwrap();
    assert_eq!((ep.base_url.as_str(), ep.mmproj_path), ("/m/model.gguf", None));
    db.set_llm_endpoint_mmproj_path(1, "/m/mmproj.gguf").unwrap();
    assert_eq!(db.llm_endpoint(1).unwrap().unwrap().mmproj_path.as_deref(), Some("/m/mmproj.gguf"));
}
