//! Bundled-sample marker of a context (schema_v69, `contexts.sample_key`):
//! default `None`, the setter, serde default for JSON that predates the column
//! (export bundles of older versions), and the migration on an existing v68 DB.

use super::models::*;
use super::vision_tests::migrate_to;
use super::Database;

fn ctx(db: &Database, name: &str) -> Context {
    db.create_context(&NewContext { name: name.into(), chunking_strategy: "structural".into(), ..Default::default() }).unwrap()
}

#[test]
fn sample_key_defaults_to_none_and_the_setter_sets_and_clears_it() {
    let db = Database::open_in_memory().unwrap();
    let c = ctx(&db, "a");
    assert_eq!(c.sample_key, None, "a new collection is the user's own");
    db.set_context_sample_key(c.id, Some("hudson")).unwrap();
    assert_eq!(db.context(c.id).unwrap().unwrap().sample_key.as_deref(), Some("hudson"));
    // `update_context` (settings form) leaves the marker alone.
    let spec = NewContext { name: "renamed".into(), chunking_strategy: "structural".into(), ..Default::default() };
    assert_eq!(db.update_context(c.id, &spec).unwrap().sample_key.as_deref(), Some("hudson"));
    db.set_context_sample_key(c.id, None).unwrap();
    assert_eq!(db.context(c.id).unwrap().unwrap().sample_key, None);
}

#[test]
fn context_json_without_the_field_still_deserializes() {
    let db = Database::open_in_memory().unwrap();
    let mut v = serde_json::to_value(ctx(&db, "b")).unwrap();
    v.as_object_mut().unwrap().remove("sample_key");
    let back: Context = serde_json::from_value(v).unwrap();
    assert_eq!(back.sample_key, None);
}

#[test]
fn v69_applies_on_an_existing_v68_database() {
    let conn = migrate_to(68);
    conn.execute_batch("INSERT INTO contexts (id, name, chunking_strategy) VALUES (1, 'old', 'structural');").unwrap();
    let db = Database::init(conn).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);
    assert_eq!(db.context(1).unwrap().unwrap().sample_key, None, "existing rows are the user's own");
}
