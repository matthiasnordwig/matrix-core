//! Tests for `chunk_kinds.rs`: `sheet_profile` chunks stay out of the
//! extraction work-list and its counts, and the profile of a sheet is found.

use crate::db::chunks::chunks_tests::seed;
use super::{chunk_kind, is_extractable, KIND_SHEET_PROFILE, KIND_SHEET_ROWS};
use crate::db::models::*;
use crate::db::Database;

fn chunk(db: &Database, ctx: i64, doc: i64, idx: i64, metadata: &str) -> i64 {
    db.create_chunk(&NewChunk {
        context_id: ctx,
        document_id: doc,
        chunk_index: idx,
        char_start: 0,
        char_end: 1,
        text: format!("chunk {idx}"),
        signature: None,
        is_omitted: false,
        metadata: metadata.to_string(),
    })
    .unwrap()
    .id
}

fn meta(kind: &str, sheet: &str, region: Option<i64>) -> String {
    match region {
        Some(r) => serde_json::json!({"kind": kind, "sheet": sheet, "region": r}).to_string(),
        None => serde_json::json!({"kind": kind, "sheet": sheet}).to_string(),
    }
}

#[test]
fn kind_is_read_from_json_metadata_only() {
    assert_eq!(chunk_kind(r#"{"kind":"sheet_rows"}"#).as_deref(), Some(KIND_SHEET_ROWS));
    assert_eq!(chunk_kind("{}"), None);
    assert_eq!(chunk_kind("not json"), None);
    assert_eq!(chunk_kind(""), None);
    assert!(!is_extractable(r#"{"kind":"sheet_profile","sheet":"A"}"#));
    assert!(is_extractable(r#"{"kind":"sheet_rows"}"#));
    assert!(is_extractable("not json") && is_extractable("{}") && is_extractable(""));
}

#[test]
fn profile_chunks_are_not_extractable_and_do_not_break_the_counts() {
    let db = Database::open_in_memory().unwrap();
    let (ctx, doc) = seed(&db);
    let profile = chunk(&db, ctx, doc, 0, &meta(KIND_SHEET_PROFILE, "S", None));
    let rows = chunk(&db, ctx, doc, 1, &meta(KIND_SHEET_ROWS, "S", None));
    let plain = chunk(&db, ctx, doc, 2, "{}");
    let broken = chunk(&db, ctx, doc, 3, "not json at all");
    let empty = chunk(&db, ctx, doc, 4, "");

    let ids: Vec<i64> = db.list_extractable_chunks(ctx).unwrap().iter().map(|c| c.id).collect();
    assert_eq!(ids, [rows, plain, broken, empty]);
    assert_eq!(db.count_extractable_chunks(ctx).unwrap(), 4);
    assert_eq!(db.count_unextracted_chunks(ctx).unwrap(), 4);
    assert_eq!(db.list_chunks(ctx, false).unwrap().len(), 5, "retrieval still sees the profile");

    // Extracting every extractable chunk completes the context: the profile is never marked.
    for id in [rows, plain, broken, empty] {
        db.insert_extracted_chunk(ctx, id).unwrap();
    }
    assert_eq!(db.count_unextracted_chunks(ctx).unwrap(), 0);
    assert!(!db.get_chunks_with_ontology(ctx).unwrap().contains(&profile));
}

#[test]
fn the_profile_of_a_sheet_and_region_is_found() {
    let db = Database::open_in_memory().unwrap();
    let (ctx, doc) = seed(&db);
    let single = chunk(&db, ctx, doc, 0, &meta(KIND_SHEET_PROFILE, "Umsatz", None));
    chunk(&db, ctx, doc, 1, &meta(KIND_SHEET_ROWS, "Umsatz", None));
    let r1 = chunk(&db, ctx, doc, 2, &meta(KIND_SHEET_PROFILE, "Zwei", Some(1)));
    let r2 = chunk(&db, ctx, doc, 3, &meta(KIND_SHEET_PROFILE, "Zwei", Some(2)));
    let find = |sheet: &str, region: i64| db.find_sheet_profile_chunk(doc, sheet, region).unwrap().map(|c| c.id);
    assert_eq!(find("Umsatz", 0), Some(single));
    assert_eq!(find("Zwei", 1), Some(r1));
    assert_eq!(find("Zwei", 2), Some(r2));
    assert_eq!(find("Zwei", 0), None);
    assert_eq!(find("Nope", 0), None);
    assert_eq!(db.find_sheet_profile_chunk(doc + 99, "Umsatz", 0).unwrap().map(|c| c.id), None);
}
