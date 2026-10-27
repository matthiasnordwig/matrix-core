//! Provisional collections (schema_v72, `provisional.rs`): hidden from
//! `list_contexts`, unique names, kept/discarded, cleaned up with the chat and
//! at start-up — and the invariant that none of these deletes ever removes a
//! collection the user made.

use super::models::*;
use super::vision_tests::migrate_to;
use super::Database;

fn spec(name: &str) -> NewContext {
    NewContext { name: name.into(), chunking_strategy: "structural".into(), ..Default::default() }
}

fn own(db: &Database, name: &str) -> i64 {
    db.create_context(&spec(name)).unwrap().id
}

fn exists(db: &Database, id: i64) -> bool {
    db.context(id).unwrap().is_some()
}

fn listed(db: &Database) -> Vec<i64> {
    db.list_contexts().unwrap().iter().map(|c| c.id).collect()
}

#[test]
fn provisional_collections_are_left_out_of_the_list_but_readable_by_id() {
    let db = Database::open_in_memory().unwrap();
    let mine = own(&db, "Mine");
    let chat = db.create_chat_session("q").unwrap().id;
    let p = db.create_provisional_context(&spec("a (chat)"), Some(chat)).unwrap();
    assert_eq!(listed(&db), vec![mine]);
    assert!(db.is_provisional_context(p.id).unwrap() && !db.is_provisional_context(mine).unwrap());
    assert_eq!(db.context(p.id).unwrap().unwrap().name, "a (chat)");
    assert_eq!(db.provisional_context_for_chat(chat).unwrap().map(|c| c.id), Some(p.id));
}

#[test]
fn a_taken_name_gets_a_number_and_counts_provisional_names_too() {
    let db = Database::open_in_memory().unwrap();
    own(&db, "x");
    let a = db.create_provisional_context(&spec("x"), None).unwrap();
    let b = db.create_provisional_context(&spec("x"), None).unwrap();
    assert_eq!((a.name.as_str(), b.name.as_str()), ("x 2", "x 3"));
    assert!(db.context_name_taken("x 3").unwrap() && !db.context_name_taken("x 4").unwrap());
}

#[test]
fn keep_clears_the_mark_and_names_it_but_never_touches_an_own_collection() {
    let db = Database::open_in_memory().unwrap();
    let mine = own(&db, "Mine");
    let chat = db.create_chat_session("q").unwrap().id;
    let p = db.create_provisional_context(&spec("a (chat)"), Some(chat)).unwrap().id;
    assert!(db.keep_provisional_context(p, "Contracts").unwrap());
    assert!(!db.is_provisional_context(p).unwrap());
    assert_eq!(db.context(p).unwrap().unwrap().name, "Contracts");
    assert!(listed(&db).contains(&p));
    assert!(db.provisional_context_for_chat(chat).unwrap().is_none(), "the chat no longer holds it");
    // Kept = ordinary: deleting the chat leaves it, a second keep/discard does nothing.
    db.delete_chat_session(chat).unwrap();
    assert!(exists(&db, p));
    assert!(!db.keep_provisional_context(mine, "Renamed").unwrap());
    assert_eq!(db.context(mine).unwrap().unwrap().name, "Mine");
    assert!(!db.discard_provisional_context(p).unwrap() && exists(&db, p));
}

#[test]
fn discard_deletes_the_provisional_collection_with_its_documents_only() {
    let db = Database::open_in_memory().unwrap();
    let mine = own(&db, "Mine");
    let p = db.create_provisional_context(&spec("a (chat)"), None).unwrap().id;
    let doc = db.create_document(&NewDocument {
        context_id: p, name: "a.pdf".into(), zip_entry: None, byte_size: None, page_count: None, content_hash: None, extracted_text: None,
    }).unwrap().id;
    assert!(!db.discard_provisional_context(mine).unwrap(), "an own collection is never discarded");
    assert!(exists(&db, mine));
    assert!(db.discard_provisional_context(p).unwrap());
    assert!(!exists(&db, p) && db.document(doc).unwrap().is_none());
    assert!(!db.discard_provisional_context(p).unwrap(), "gone already");
}

#[test]
fn deleting_a_chat_deletes_its_provisional_collection_and_nothing_else() {
    let db = Database::open_in_memory().unwrap();
    let mine = own(&db, "Mine");
    let chat = db.create_chat_session("q").unwrap().id;
    let other = db.create_chat_session("r").unwrap().id;
    let p = db.create_provisional_context(&spec("a"), Some(chat)).unwrap().id;
    let q = db.create_provisional_context(&spec("b"), Some(other)).unwrap().id;
    assert!(db.delete_chat_session(chat).unwrap());
    assert!(!exists(&db, p), "the chat's attachment went with it");
    assert!(exists(&db, q) && exists(&db, mine));
}

#[test]
fn start_up_sweep_removes_attachments_without_a_reachable_chat_only() {
    let db = Database::open_in_memory().unwrap();
    let mine = own(&db, "Mine");
    let asked = db.create_chat_session("asked").unwrap().id;
    db.append_chat_message(asked, "user", "what is in it?").unwrap();
    let silent = db.create_chat_session("never asked").unwrap().id;
    let kept_chat = db.create_provisional_context(&spec("kept"), Some(asked)).unwrap().id;
    let no_chat = db.create_provisional_context(&spec("stateless"), None).unwrap().id;
    let empty_chat = db.create_provisional_context(&spec("silent"), Some(silent)).unwrap().id;
    // An own collection without any chat is untouched (it has no mark).
    assert_eq!(db.delete_orphan_provisional_contexts().unwrap(), 2);
    assert!(exists(&db, kept_chat) && exists(&db, mine));
    assert!(!exists(&db, no_chat) && !exists(&db, empty_chat));
    assert_eq!(db.delete_orphan_provisional_contexts().unwrap(), 0, "idempotent");
}

#[test]
fn bind_attaches_a_provisional_collection_to_a_chat_only() {
    let db = Database::open_in_memory().unwrap();
    let mine = own(&db, "Mine");
    let chat = db.create_chat_session("q").unwrap().id;
    let p = db.create_provisional_context(&spec("a"), None).unwrap().id;
    assert!(db.bind_provisional_context(p, chat).unwrap());
    assert!(!db.bind_provisional_context(mine, chat).unwrap());
    assert_eq!(db.provisional_context_for_chat(chat).unwrap().map(|c| c.id), Some(p));
}

#[test]
fn v72_applies_on_an_existing_v71_database() {
    let conn = migrate_to(71);
    conn.execute_batch("INSERT INTO contexts (id, name, chunking_strategy) VALUES (1, 'old', 'structural');").unwrap();
    let db = Database::init(conn).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);
    assert!(!db.is_provisional_context(1).unwrap(), "existing rows are the user's own");
    assert_eq!(listed(&db), vec![1]);
    assert_eq!(db.delete_orphan_provisional_contexts().unwrap(), 0);
}
