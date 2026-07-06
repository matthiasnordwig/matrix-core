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
