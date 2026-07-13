//! Chat-session CRUD round-trips (AP6 history-awareness, `schema_v53`): create/
//! list/rename/delete, message append + ordering, cascade on delete, and the
//! `updated_at` bump that drives the most-recent-first session ordering.

use super::Database;

fn db() -> Database {
    Database::open_in_memory().expect("open in-memory db")
}

#[test]
fn create_and_get_round_trip() {
    let db = db();
    let s = db.create_chat_session("First question").unwrap();
    assert!(s.id > 0);
    assert_eq!(s.title, "First question");
    assert!(s.created_at > 0);
    assert_eq!(s.created_at, s.updated_at);

    let got = db.chat_session(s.id).unwrap().expect("session exists");
    assert_eq!(got.id, s.id);
    assert_eq!(got.title, "First question");
    assert!(db.chat_session(9999).unwrap().is_none());
}

#[test]
fn append_and_fetch_messages_in_order() {
    let db = db();
    let s = db.create_chat_session("Q").unwrap();
    let m1 = db.append_chat_message(s.id, "user", "hello").unwrap();
    let m2 = db.append_chat_message(s.id, "assistant", "hi there").unwrap();
    let m3 = db.append_chat_message(s.id, "user", "and again?").unwrap();

    assert_eq!(m1.session_id, s.id);
    assert_eq!(m1.role, "user");
    assert_eq!(m1.content, "hello");
    // Tool columns are reserved for the later tool-loop AP; always NULL here.
    assert!(m1.tool_calls_json.is_none());
    assert!(m1.tool_payload_json.is_none());

    let msgs = db.chat_messages_for_session(s.id).unwrap();
    assert_eq!(
        msgs.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![m1.id, m2.id, m3.id]
    );
    assert_eq!(
        msgs.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
        vec!["hello", "hi there", "and again?"]
    );
}

#[test]
fn append_bumps_session_updated_at() {
    let db = db();
    let s = db.create_chat_session("Q").unwrap();
    // Force a distinguishable timestamp, then append and confirm the bump.
    db.conn
        .execute(
            "UPDATE chat_sessions SET updated_at = 1000 WHERE id = ?1",
            [s.id],
        )
        .unwrap();
    db.append_chat_message(s.id, "user", "hi").unwrap();
    let after = db.chat_session(s.id).unwrap().unwrap();
    assert!(
        after.updated_at > 1000,
        "append must bump updated_at (was {})",
        after.updated_at
    );
}

#[test]
fn rename_updates_title_only() {
    let db = db();
    let s = db.create_chat_session("old").unwrap();
    let renamed = db.rename_chat_session(s.id, "new title").unwrap();
    assert_eq!(renamed.id, s.id);
    assert_eq!(renamed.title, "new title");
    assert_eq!(renamed.created_at, s.created_at);

    // Renaming a missing session is a NotFound, not a silent no-op.
    assert!(db.rename_chat_session(9999, "x").is_err());
}

#[test]
fn delete_cascades_messages() {
    let db = db();
    let s = db.create_chat_session("Q").unwrap();
    db.append_chat_message(s.id, "user", "a").unwrap();
    db.append_chat_message(s.id, "assistant", "b").unwrap();

    assert!(db.delete_chat_session(s.id).unwrap());
    assert!(db.chat_session(s.id).unwrap().is_none());
    // Cascade: the messages must be gone with the session.
    let remaining: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM chat_messages WHERE session_id = ?1",
            [s.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0);
    // Deleting an already-gone session returns false.
    assert!(!db.delete_chat_session(s.id).unwrap());
}

#[test]
fn list_orders_most_recently_updated_first() {
    let db = db();
    let a = db.create_chat_session("A").unwrap();
    let b = db.create_chat_session("B").unwrap();
    let c = db.create_chat_session("C").unwrap();
    // Distinct updated_at so ordering is deterministic (create timestamps can
    // collide within the same second).
    for (id, ts) in [(a.id, 100), (b.id, 300), (c.id, 200)] {
        db.conn
            .execute(
                "UPDATE chat_sessions SET updated_at = ?2 WHERE id = ?1",
                rusqlite::params![id, ts],
            )
            .unwrap();
    }
    let listed: Vec<i64> = db
        .list_chat_sessions()
        .unwrap()
        .iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(listed, vec![b.id, c.id, a.id]);
}

#[test]
fn turn_meta_round_trip() {
    // schema_v57: assistant turns persist model / reasoning_effort / answer_json;
    // plain appends leave them NULL (user turns, pre-v57 behavior).
    let db = db();
    let s = db.create_chat_session("Q").unwrap();

    let plain = db.append_chat_message(s.id, "user", "hello").unwrap();
    assert!(plain.model.is_none());
    assert!(plain.reasoning_effort.is_none());
    assert!(plain.answer_json.is_none());

    let meta = super::ChatTurnMeta {
        model: Some("gpt-5".into()),
        reasoning_effort: Some("high".into()),
        answer_json: Some(r#"{"sources":[],"citations":[]}"#.into()),
    };
    let a = db
        .append_chat_message_with_meta(s.id, "assistant", "hi [1]", &meta)
        .unwrap();
    assert_eq!(a.model.as_deref(), Some("gpt-5"));
    assert_eq!(a.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(a.answer_json.as_deref(), Some(r#"{"sources":[],"citations":[]}"#));

    // Tool variant carries meta AND the trace columns.
    let t = db
        .append_chat_message_with_tools(s.id, "assistant", "looped", &meta, Some("[]"), Some("[]"))
        .unwrap();
    assert_eq!(t.model.as_deref(), Some("gpt-5"));
    assert_eq!(t.tool_calls_json.as_deref(), Some("[]"));

    // Reload path (row_to_message) preserves the columns.
    let msgs = db.chat_messages_for_session(s.id).unwrap();
    assert_eq!(msgs.len(), 3);
    assert!(msgs[0].model.is_none());
    assert_eq!(msgs[1].model.as_deref(), Some("gpt-5"));
    assert_eq!(msgs[1].reasoning_effort.as_deref(), Some("high"));
}
