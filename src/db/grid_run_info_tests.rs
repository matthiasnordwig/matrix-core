//! schema_v66: a grid run's start settings (`GridRunInfo`) — stored via
//! `set_grid_run_info`, read back by `grid_run_meta` and joined into
//! `list_grid_runs`; runs without meta or from before v66 report all `None`.
//! schema_v81: the unsearched sources of a run (`set_grid_run_not_searched`).
//! schema_v83: the settings key of a run (`set_grid_run_settings_key`).

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

fn dora() -> NotSearchable {
    NotSearchable { id: 7, name: "DORA".into(), reason: "not_indexed".into() }
}

/// schema_v81: the unsearched sources stay with the run; an empty list stores NULL, a re-run
/// overwrites the list, a meta upsert (re-run start) keeps it until it is set anew.
#[test]
fn unsearched_sources_round_trip_and_a_rerun_overwrites_them() {
    let db = db();
    db.upsert_grid_run_meta("run-n", "sys", None).unwrap();
    assert!(db.grid_run_meta("run-n").unwrap().unwrap().sources_not_searched.is_empty());
    let empty = NotSearchable { id: 8, name: "Empty".into(), reason: "no_documents".into() };
    db.set_grid_run_not_searched("run-n", &[dora(), empty.clone()]).unwrap();
    assert_eq!(db.grid_run_meta("run-n").unwrap().unwrap().sources_not_searched, vec![dora(), empty.clone()]);
    db.upsert_grid_run_meta("run-n", "sys2", None).unwrap();
    assert_eq!(db.grid_run_meta("run-n").unwrap().unwrap().sources_not_searched.len(), 2);
    db.set_grid_run_not_searched("run-n", &[empty.clone()]).unwrap();
    assert_eq!(db.grid_run_meta("run-n").unwrap().unwrap().sources_not_searched, vec![empty]);
    db.set_grid_run_not_searched("run-n", &[]).unwrap();
    let stored: Option<String> = db
        .conn
        .query_row("SELECT sources_not_searched FROM grid_run_meta WHERE run_id = 'run-n'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, None, "an empty list stores NULL");
    // A run without meta row: a no-op, not an error.
    db.set_grid_run_not_searched("run-none", &[dora()]).unwrap();
}

/// schema_v81 on an existing v80 database: a stored run keeps its meta and reports no unsearched sources.
#[test]
fn v81_applies_on_an_existing_v80_database() {
    let old = super::vision_tests::migrate_to(80);
    old.execute("INSERT INTO grid_run_meta (run_id, system_prompt, json_schema) VALUES ('run-v80', 'sys', NULL)", []).unwrap();
    let db = Database::init(old).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);
    let meta = db.grid_run_meta("run-v80").unwrap().unwrap();
    assert_eq!(meta.system_prompt, "sys");
    assert!(meta.sources_not_searched.is_empty());
    db.set_grid_run_not_searched("run-v80", &[dora()]).unwrap();
    assert_eq!(db.grid_run_meta("run-v80").unwrap().unwrap().sources_not_searched, vec![dora()]);
}

/// The list outlives the process: a fresh connection on the same file reads it back.
#[test]
fn unsearched_sources_survive_reopening_the_database() {
    let path = std::env::temp_dir().join(format!("matrix-v81-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let db = Database::open(&path).unwrap();
        db.upsert_grid_run_meta("run-r", "sys", None).unwrap();
        db.set_grid_run_not_searched("run-r", &[dora()]).unwrap();
    }
    let reopened = Database::open(&path).unwrap();
    assert_eq!(reopened.grid_run_meta("run-r").unwrap().unwrap().sources_not_searched, vec![dora()]);
    drop(reopened);
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
}

#[test]
fn meta_without_the_list_still_deserializes() {
    let old: GridRunMeta = serde_json::from_str(r#"{"system_prompt":"s","json_schema":null}"#).unwrap();
    assert!(old.sources_not_searched.is_empty());
}

/// schema_v83: the settings key of a run — absent before it is set, kept by a meta upsert, gone with the run;
/// `grid_run_exists` sees a run by its rows or its meta.
#[test]
fn settings_key_round_trip_and_run_existence() {
    let db = db();
    assert!(!db.grid_run_exists("run-k").unwrap());
    assert_eq!(db.grid_run_settings_key("run-k").unwrap(), None);
    db.upsert_grid_run_meta("run-k", "sys", None).unwrap();
    assert!(db.grid_run_exists("run-k").unwrap());
    assert_eq!(db.grid_run_settings_key("run-k").unwrap(), None);
    db.set_grid_run_settings_key("run-k", "abc").unwrap();
    db.upsert_grid_run_meta("run-k", "sys2", None).unwrap();
    assert_eq!(db.grid_run_settings_key("run-k").unwrap().as_deref(), Some("abc"));
    add_row(&db, "run-rows-only", "p");
    assert!(db.grid_run_exists("run-rows-only").unwrap());
    db.delete_grid_chat_run("run-k").unwrap();
    assert!(!db.grid_run_exists("run-k").unwrap());
}

/// schema_v83 on an existing v82 database: a stored run has no settings key.
#[test]
fn v83_applies_on_an_existing_v82_database() {
    let old = super::vision_tests::migrate_to(82);
    old.execute("INSERT INTO grid_run_meta (run_id, system_prompt, json_schema) VALUES ('run-v82', 'sys', NULL)", []).unwrap();
    let db = Database::init(old).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);
    assert_eq!(db.grid_run_settings_key("run-v82").unwrap(), None);
    assert!(db.grid_run_exists("run-v82").unwrap());
}
