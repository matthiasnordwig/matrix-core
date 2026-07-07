use super::fixtures::{db, seed_context_with_chunk};

// --- metrics (rolling last-3-runs window) -----------------------------------

#[test]
fn phase_metrics_keep_only_last_three_runs() {
    let db = db();
    for ms in [100.0, 200.0, 300.0, 400.0] {
        db.insert_phase_metric("extract", "model-a", ms).unwrap();
    }
    let avg = db.get_phase_averages("model-a").unwrap()["extract"];
    // Only the last 3 (200, 300, 400) must survive the rolling window.
    assert!((avg - 300.0).abs() < 1e-6, "expected avg of last 3 runs, got {avg}");
}

// --- dedup cache --------------------------------------------------------------

#[test]
fn dedup_cache_round_trip() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "Ctx10");
    db.cache_dedup_decision(ctx, 1, 2, true).unwrap();
    db.cache_dedup_decision(ctx, 3, 4, false).unwrap();

    let cache = db.get_dedup_cache(ctx).unwrap();
    assert_eq!(cache[&(1, 2)], true);
    assert_eq!(cache[&(3, 4)], false);
}

// --- quarantine + chunk-state resumability ----------------------------------

#[test]
fn quarantine_round_trip() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx11");
    db.insert_quarantined_chunk(ctx, chunk_id, "{\"nodes\":[]}", "JSON parse error").unwrap();

    let rows = db.get_quarantined_chunks(ctx).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].error_reason, "JSON parse error");

    db.delete_quarantined_chunk(chunk_id).unwrap();
    assert!(db.get_quarantined_chunks(ctx).unwrap().is_empty());
}

#[test]
fn chunk_state_save_load_delete_round_trip() {
    let db = db();
    let (ctx, chunk_id) = seed_context_with_chunk(&db, "Ctx12");
    assert_eq!(db.load_chunk_state(ctx, chunk_id).unwrap(), None);

    db.save_chunk_state(ctx, chunk_id, "[0,1]", "{\"nodes\":[]}").unwrap();
    assert_eq!(
        db.load_chunk_state(ctx, chunk_id).unwrap(),
        Some(("[0,1]".to_string(), "{\"nodes\":[]}".to_string()))
    );

    // Upsert overwrites, doesn't duplicate.
    db.save_chunk_state(ctx, chunk_id, "[0,1,2]", "{\"nodes\":[1]}").unwrap();
    assert_eq!(
        db.load_chunk_state(ctx, chunk_id).unwrap(),
        Some(("[0,1,2]".to_string(), "{\"nodes\":[1]}".to_string()))
    );

    db.delete_chunk_state(ctx, chunk_id).unwrap();
    assert_eq!(db.load_chunk_state(ctx, chunk_id).unwrap(), None);
}

// --- persistent ontology run log (schema_v43) -------------------------------

#[test]
fn run_log_round_trip() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "runlog");
    db.insert_ontology_run_log(
        "run-1", ctx, "community_summarization",
        1000, Some(1050),
        10, 7, 3,
        Some("boom"),
    ).unwrap();

    let rows = db.list_ontology_run_log(ctx).unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.run_id, "run-1");
    assert_eq!(row.context_id, ctx);
    assert_eq!(row.phase, "community_summarization");
    assert_eq!(row.started_at, 1000);
    assert_eq!(row.finished_at, Some(1050));
    assert_eq!(row.attempted, 10);
    assert_eq!(row.succeeded, 7);
    assert_eq!(row.failed, 3);
    assert_eq!(row.sample_error, Some("boom".to_string()));
}

#[test]
fn run_log_orders_most_recent_first() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "runlog2");
    db.insert_ontology_run_log("run-a", ctx, "community_summarization", 1000, Some(1010), 1, 1, 0, None).unwrap();
    db.insert_ontology_run_log("run-b", ctx, "community_summarization", 2000, Some(2010), 1, 1, 0, None).unwrap();

    let rows = db.list_ontology_run_log(ctx).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].started_at, 2000);
    assert_eq!(rows[0].run_id, "run-b");
    assert_eq!(rows[1].started_at, 1000);
    assert_eq!(rows[1].run_id, "run-a");
}

#[test]
fn run_log_nullable_fields_round_trip_as_none() {
    let db = db();
    let (ctx, _chunk) = seed_context_with_chunk(&db, "runlog3");
    db.insert_ontology_run_log("run-c", ctx, "community_summarization", 3000, None, 0, 0, 0, None).unwrap();

    let rows = db.list_ontology_run_log(ctx).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].finished_at, None);
    assert_eq!(rows[0].sample_error, None);
}
