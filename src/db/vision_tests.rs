//! Image understanding (schema_v62): `llm_endpoints.supports_vision` and
//! `contexts.image_llm_id` — CRUD round-trips, the FK's ON DELETE SET NULL when
//! the endpoint is deleted, serde defaults for JSON that predates the columns,
//! and the migration applied on top of an existing v61 database.

use rusqlite::Connection;

use super::llm_endpoint_tests::seed_endpoint;
use super::models::*;
use super::Database;

fn new_context(name: &str, llm_id: Option<i64>, image_llm_id: Option<i64>) -> NewContext {
    NewContext { name: name.into(), chunking_strategy: "structural".into(), llm_id, image_llm_id, ..Default::default() }
}

fn spec_of(e: &LlmEndpoint) -> NewLlmEndpoint {
    serde_json::from_value(serde_json::to_value(e).unwrap()).unwrap()
}

#[test]
fn supports_vision_round_trips_through_create_and_update() {
    let db = Database::open_in_memory().unwrap();
    let id = seed_endpoint(&db, "ep", "openai");
    let created = db.llm_endpoint(id).unwrap().unwrap();
    assert!(!created.supports_vision, "default off");

    let mut spec = spec_of(&created);
    spec.supports_vision = true;
    assert!(db.update_llm_endpoint(id, &spec).unwrap().supports_vision);
    assert!(db.list_llm_endpoints().unwrap()[0].supports_vision);

    spec.name = "vision".into();
    let second = db.create_llm_endpoint(&spec).unwrap();
    assert!(second.supports_vision, "create persists the flag");
    spec.supports_vision = false;
    assert!(!db.update_llm_endpoint(second.id, &spec).unwrap().supports_vision);
}

#[test]
fn image_llm_id_round_trips_and_is_cleared_when_the_endpoint_is_deleted() {
    let db = Database::open_in_memory().unwrap();
    let chat = seed_endpoint(&db, "chat", "openai");
    let eyes = seed_endpoint(&db, "eyes", "anthropic");

    let ctx = db.create_context(&new_context("c", Some(chat), Some(eyes))).unwrap();
    assert_eq!(ctx.image_llm_id, Some(eyes));
    assert_eq!(db.context(ctx.id).unwrap().unwrap().image_llm_id, Some(eyes));

    let upd = db.update_context(ctx.id, &new_context("c", Some(chat), None)).unwrap();
    assert_eq!(upd.image_llm_id, None, "update writes None = descriptions off");
    let upd = db.update_context(ctx.id, &new_context("c", Some(chat), Some(chat))).unwrap();
    assert_eq!(upd.image_llm_id, Some(chat));

    // A dangling id is refused (the FK is enforced, not just declared).
    assert!(db.update_context(ctx.id, &new_context("c", Some(chat), Some(9999))).is_err());

    // Deleting the image endpoint turns descriptions off and leaves the rest.
    let other = db.create_context(&new_context("d", Some(chat), Some(eyes))).unwrap();
    assert!(db.delete_llm_endpoint(eyes).unwrap());
    let other = db.context(other.id).unwrap().unwrap();
    assert_eq!((other.llm_id, other.image_llm_id), (Some(chat), None));
    // Deleting the chat endpoint clears both llm_id and image_llm_id of `c`.
    assert!(db.delete_llm_endpoint(chat).unwrap());
    let c = db.context(ctx.id).unwrap().unwrap();
    assert_eq!((c.llm_id, c.image_llm_id), (None, None));
}

#[test]
fn json_predating_v62_deserializes_with_defaults() {
    let db = Database::open_in_memory().unwrap();
    let id = seed_endpoint(&db, "ep", "openai");
    let mut ep = serde_json::to_value(db.llm_endpoint(id).unwrap().unwrap()).unwrap();
    ep.as_object_mut().unwrap().remove("supports_vision");
    assert!(!serde_json::from_value::<LlmEndpoint>(ep.clone()).unwrap().supports_vision);
    assert!(!serde_json::from_value::<NewLlmEndpoint>(ep).unwrap().supports_vision);

    let ctx = db.create_context(&new_context("c", Some(id), Some(id))).unwrap();
    let mut v = serde_json::to_value(&ctx).unwrap();
    v.as_object_mut().unwrap().remove("image_llm_id");
    assert_eq!(serde_json::from_value::<Context>(v.clone()).unwrap().image_llm_id, None);
    assert_eq!(serde_json::from_value::<NewContext>(v).unwrap().image_llm_id, None);
}

/// A fresh connection brought to `version` exactly like `Database::migrate`
/// would (including the Rust hooks of v9/v50/v51/v52); also used by later
/// migration tests (`mmproj_tests.rs`).
pub(super) fn migrate_to(version: usize) -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    for (i, sql) in super::MIGRATIONS[..version].iter().enumerate() {
        let target = (i + 1) as i64;
        if target == 51 {
            super::migrate_v50_to_v51_embedding_kind(&mut conn).unwrap();
            continue;
        }
        if target == 52 {
            super::migrate_v51_to_v52_reranker_kind(&mut conn).unwrap();
            continue;
        }
        let tx = conn.transaction().unwrap();
        tx.execute_batch(sql).unwrap();
        if target == 9 {
            super::migrate_v8_to_v9_profiles(&tx).unwrap();
        }
        if target == 50 {
            super::migrate_v49_to_v50_reranker(&tx).unwrap();
        }
        tx.pragma_update(None, "user_version", target).unwrap();
        tx.commit().unwrap();
    }
    let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(v, version as i64);
    conn
}

/// v61 database with rows the v62 migration must keep.
fn migrate_to_61_then_seed() -> Connection {
    let conn = migrate_to(61);
    conn.execute_batch(
        "INSERT INTO llm_endpoints (id, name, base_url, model_id, provider, supports_tools)
             VALUES (1, 'old', 'http://x.invalid', 'm', 'openai', 1);
         INSERT INTO contexts (id, name, llm_id, chunking_strategy) VALUES (1, 'ctx', 1, 'structural');",
    )
    .unwrap();
    conn
}

#[test]
fn v62_applies_on_an_existing_v61_database() {
    let db = Database::init(migrate_to_61_then_seed()).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);

    let ep = db.llm_endpoint(1).unwrap().unwrap();
    assert!(ep.supports_tools && !ep.supports_vision, "existing row: flag defaults to 0");
    let ctx = db.context(1).unwrap().unwrap();
    assert_eq!((ctx.llm_id, ctx.image_llm_id), (Some(1), None));

    // The FK added by ALTER TABLE is live on the migrated DB too.
    let mut spec = new_context("ctx", Some(1), Some(1));
    spec.chunking_strategy = ctx.chunking_strategy.clone();
    assert_eq!(db.update_context(1, &spec).unwrap().image_llm_id, Some(1));
    assert!(db.delete_llm_endpoint(1).unwrap());
    assert_eq!(db.context(1).unwrap().unwrap().image_llm_id, None);
}
