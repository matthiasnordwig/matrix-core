//! schema_v66: a grid run's start settings (`GridRunInfo`) — stored via
//! `set_grid_run_info`, read back by `grid_run_meta` and joined into
//! `list_grid_runs`; runs without meta or from before v66 report all `None`.

use super::models::*;
use super::Database;

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

fn add_row(db: &Database, run: &str, prompt: &str) {
    db.upsert_grid_chat_result(&GridChatUpsert {
        run_id: run.into(),
        row_uid: format!("{run}-u"),
        prompt_snapshot: None,
        row_ref_type: RowRefType::Chunk,
        row_ref_id: 1,
        prompt: prompt.into(),
        columns_context: None,
        retrieved_refs: None,
        response: Some("r".into()),
        status: ChatStatus::Done,
        error: None,
        row_source_text: None,
    })
    .unwrap();
}

fn agentic_claude() -> GridRunInfo {
    GridRunInfo {
        llm_endpoint_id: Some(3),
        pool_id: None,
        model_label: Some("Claude (claude-x)".into()),
        tool_call_budget: Some(8),
        template_name: Some("Findings".into()),
        context_mode: Some("Classic".into()),
    }
}

#[test]
fn run_info_round_trips_through_meta_and_history() {
    let db = db();
    add_row(&db, "run-a", "Same prompt");
    db.upsert_grid_run_meta("run-a", "sys", None).unwrap();
    db.set_grid_run_info("run-a", &agentic_claude()).unwrap();

    let meta = db.grid_run_meta("run-a").unwrap().unwrap();
    assert_eq!(meta.system_prompt, "sys");
    assert_eq!(meta.info, agentic_claude());

    let runs = db.list_grid_runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].prompt, "Same prompt");
    assert_eq!(runs[0].info, agentic_claude());
}

#[test]
fn rerun_overwrites_info_and_meta_upsert_keeps_it() {
    let db = db();
    add_row(&db, "run-b", "p");
    db.upsert_grid_run_meta("run-b", "sys", None).unwrap();
    db.set_grid_run_info("run-b", &agentic_claude()).unwrap();
    // A re-run upserts the prompt first; the info stays until it is set anew.
    db.upsert_grid_run_meta("run-b", "sys2", None).unwrap();
    assert_eq!(db.grid_run_meta("run-b").unwrap().unwrap().info, agentic_claude());

    let local_classic = GridRunInfo {
        llm_endpoint_id: None,
        pool_id: Some(1),
        model_label: Some("Pool: Local".into()),
        tool_call_budget: Some(0),
        template_name: None,
        context_mode: Some("Hybrid".into()),
    };
    db.set_grid_run_info("run-b", &local_classic).unwrap();
    assert_eq!(db.grid_run_meta("run-b").unwrap().unwrap().info, local_classic);
}

#[test]
fn legacy_runs_report_no_info() {
    let db = db();
    // No meta row at all (pre-AP7 run) …
    add_row(&db, "run-old", "p1");
    // … and a meta row written without start settings (pre-v66 run).
    add_row(&db, "run-v65", "p2");
    db.upsert_grid_run_meta("run-v65", "sys", Some("")).unwrap();

    for run in db.list_grid_runs().unwrap() {
        assert_eq!(run.info, GridRunInfo::default(), "{}", run.run_id);
    }
    assert_eq!(db.grid_run_meta("run-v65").unwrap().unwrap().info, GridRunInfo::default());
    // Setting info on a run without meta row is a no-op, not an error.
    db.set_grid_run_info("run-old", &agentic_claude()).unwrap();
    assert!(db.grid_run_meta("run-old").unwrap().is_none());
}

#[test]
fn run_info_serializes_flat_for_the_frontend() {
    let run = GridRun { run_id: "r".into(), prompt: "p".into(), updated_at: 1, info: agentic_claude() };
    let v = serde_json::to_value(&run).unwrap();
    assert_eq!(v["model_label"], "Claude (claude-x)");
    assert_eq!(v["tool_call_budget"], 8);
    assert!(v.get("info").is_none());
    // Older payloads without the fields still deserialize.
    let old: GridRunMeta = serde_json::from_str(r#"{"system_prompt":"s","json_schema":null}"#).unwrap();
    assert_eq!(old.info, GridRunInfo::default());
}

/// schema_v70 drops the dead `grid_sheets`/`grid_rows` tables, on a fresh and
/// on an existing v69 database; the history table `grid_chat_results` stays.
#[test]
fn v70_drops_the_dead_grid_sheet_tables() {
    let tables = |conn: &rusqlite::Connection| -> Vec<String> {
        let mut st = conn.prepare("SELECT name FROM sqlite_master WHERE name LIKE 'grid_%' OR name = 'idx_grid_rows_sheet'").unwrap();
        st.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
    };
    let old = super::vision_tests::migrate_to(69);
    assert!(tables(&old).iter().any(|t| t == "grid_rows"), "v69 still has the table");
    let db = Database::init(old).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);
    let fresh = tables(&db.conn);
    assert!(fresh.iter().all(|t| t != "grid_rows" && t != "grid_sheets" && t != "idx_grid_rows_sheet"), "{fresh:?}");
    assert!(fresh.iter().any(|t| t == "grid_chat_results"));
}
