//! schema_v76: `cell_sources` of a grid result — written by `set_grid_cell_sources`, read back with
//! the row, cleared when the row is written again (a re-run must not keep the old answer's origins).

use super::models::*;
use super::Database;

fn upsert(db: &Database, status: ChatStatus) -> GridChatResult {
    db.upsert_grid_chat_result(&GridChatUpsert {
        run_id: "r".into(),
        row_uid: "u".into(),
        row_ref_type: RowRefType::Chunk,
        row_ref_id: 1,
        prompt: "p".into(),
        columns_context: None,
        retrieved_refs: None,
        response: Some("{}".into()),
        status,
        error: None,
        prompt_snapshot: None,
        row_source_text: None,
    })
    .unwrap()
}

#[test]
fn cell_sources_round_trip_and_are_cleared_by_the_next_write() {
    let db = Database::open_in_memory().unwrap();
    assert_eq!(upsert(&db, ChatStatus::Queued).cell_sources, None);
    db.set_grid_cell_sources("r", "u", r#"[{"a":{"from":"own"}}]"#).unwrap();
    let stored = db.list_grid_chat_results("r").unwrap();
    assert_eq!(stored[0].cell_sources.as_deref(), Some(r#"[{"a":{"from":"own"}}]"#));
    assert_eq!(upsert(&db, ChatStatus::Done).cell_sources, None);
}
