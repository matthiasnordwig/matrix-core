//! Replacing a document's chunks keeps the graph's evidence
//! (`evidence_carry_db.rs`), in one savepoint; a deleted document's evidence is
//! parked without text, expires, and is adopted by the same file added again or
//! a twin copy (`evidence_parking.rs`); restore from a backup with its checks
//! (`evidence_restore.rs`).
use super::fixtures::*;
use crate::db::chunks::ChunkSlot;
use crate::db::models::*;
use crate::db::{CoreError, Database};

const A: &str = "Die KfW fördert den Mittelstand in Deutschland. ";
const B: &str = "Der Bund garantiert die Verbindlichkeiten der Bank.";

fn text() -> String {
    format!("{A}{B}")
}

fn doc(db: &Database, ctx: i64, text: &str) -> i64 {
    db.create_document(&NewDocument {
        context_id: ctx,
        name: "bericht.pdf".into(),
        zip_entry: None,
        byte_size: None,
        page_count: None,
        content_hash: None,
        extracted_text: Some(text.into()),
    })
    .unwrap()
    .id
}

fn chunk(db: &Database, ctx: i64, doc: i64, index: i64, s: usize, e: usize, text: &str) -> i64 {
    db.create_chunk(&nc(ctx, doc, index, s, e, text)).unwrap().id
}

fn nc(ctx: i64, doc: i64, index: i64, s: usize, e: usize, text: &str) -> NewChunk {
    NewChunk {
        context_id: ctx,
        document_id: doc,
        chunk_index: index,
        char_start: s as i64,
        char_end: e as i64,
        text: text.into(),
        signature: None,
        is_omitted: false,
        metadata: "{}".into(),
    }
}

/// Context with one document cut into ONE chunk (A+B), extracted, with an
/// edge KfW→Mittelstand (no quote) and an edge Bund→Bank (quote in B) at it.
fn seeded(db: &Database) -> (i64, i64, [i64; 2]) {
    let (ctx, _) = seed_context_with_chunk(db, "carry");
    let d = doc(db, ctx, &text());
    let c = chunk(db, ctx, d, 0, 0, text().len(), &text());
    db.insert_extracted_chunk(ctx, c).unwrap();
    let kfw = node(db, ctx, "KfW", "Org").id;
    let ms = node(db, ctx, "Mittelstand", "Group").id;
    let bund = node(db, ctx, "Bund", "Org").id;
    let bank = node(db, ctx, "Bank", "Org").id;
    let e1 = edge(db, ctx, c, kfw, ms, "fördert").id;
    let e2 = edge(db, ctx, c, bund, bank, "garantiert").id;
    db.add_ontology_edge_chunk_with_evidence(e2, c, Some("garantiert die Verbindlichkeiten")).unwrap();
    db.insert_ontology_edge_review(ctx, e2, Some(c), "garantiert", Some("garantiert die"), "check").unwrap();
    (ctx, d, [e1, e2])
}

fn split(db: &Database, ctx: i64, d: i64) -> (i64, i64) {
    let a = db.create_chunk(&nc(ctx, d, 0, 0, A.len(), A)).unwrap().id;
    let b = db.create_chunk(&nc(ctx, d, 1, A.len(), text().len(), B)).unwrap().id;
    (a, b)
}

fn evidence_rows(db: &Database, ctx: i64) -> Vec<(i64, i64, Option<String>)> {
    let mut stmt = db
        .conn
        .prepare(
            "SELECT ec.edge_id, ec.chunk_id, ec.evidence FROM ontology_edge_chunks ec
               JOIN ontology_edges e ON e.id = ec.edge_id WHERE e.context_id = ?1 ORDER BY 1, 2",
        )
        .unwrap();
    let rows = stmt.query_map([ctx], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
}

#[test]
fn replace_carries_evidence_reviews_and_extracted_marks() {
    let db = db();
    let (ctx, d, [e1, e2]) = seeded(&db);
    let ((a, b), r) = db.replace_document_chunks(d, || Ok::<_, CoreError>(split(&db, ctx, d))).unwrap();
    assert_eq!(evidence_rows(&db, ctx), vec![(e1, a, None), (e2, b, Some("garantiert die Verbindlichkeiten".into()))]);
    let review = &db.list_ontology_edge_reviews(ctx).unwrap()[0];
    assert_eq!(review.chunk_id, Some(b));
    assert_eq!(db.get_chunks_with_ontology(ctx).unwrap(), [a, b].into_iter().collect());
    assert_eq!((r.evidence, r.placed(), r.unplaced, r.by_labels, r.by_text), (3, 3, 0, 1, 2));
    assert_eq!((r.extracted_before, r.extracted_after, r.new_chunks), (1, 2, 2));
}

#[test]
fn failed_build_rolls_everything_back() {
    let db = db();
    let (ctx, d, _) = seeded(&db);
    let before = evidence_rows(&db, ctx);
    let res = db.replace_document_chunks(d, || {
        split(&db, ctx, d);
        Err::<(), _>(CoreError::InvalidState("boom".into()))
    });
    assert!(res.is_err());
    assert_eq!(evidence_rows(&db, ctx), before);
    assert_eq!(db.count_chunks(ctx).unwrap(), 2); // fixture chunk + the original one
}

#[test]
fn insert_chunks_in_order_nests_inside_a_replacement() {
    let db = db();
    let (ctx, d, _) = seeded(&db);
    let (_, r) = db
        .replace_document_chunks(d, || {
            let (a, b) = split(&db, ctx, d);
            let extra = nc(ctx, d, 0, 0, 0, "Seite 1: Bild");
            let order = [ChunkSlot::New(&extra), ChunkSlot::Existing(a), ChunkSlot::Existing(b)];
            db.insert_chunks_in_order(d, &order)
        })
        .unwrap();
    assert_eq!((r.new_chunks, r.placed()), (3, 3));
}

#[test]
fn deleted_document_is_parked_and_adopted_by_the_same_file() {
    let db = db();
    let (ctx, d, [e1, e2]) = seeded(&db);
    let del = db.delete_document_parking(d).unwrap();
    assert_eq!((del.deleted, del.parked_evidence, del.parked_extracted, del.adopted_by), (true, 3, 1, None));
    assert!(evidence_rows(&db, ctx).is_empty());
    assert_eq!(db.count_parked_documents(ctx).unwrap(), 1);

    // Same text again, not chunked yet: nothing adopted, rows stay.
    let d2 = doc(&db, ctx, &text());
    assert_eq!(db.adopt_parked_graph(d2).unwrap(), None);
    // Another text never adopts.
    let other = doc(&db, ctx, "ganz anderer Text");
    chunk(&db, ctx, other, 0, 0, 5, "ganz anderer Text");
    assert!(db.adopt_parked_graphs(ctx).unwrap().is_empty());

    let (a, b) = split(&db, ctx, d2);
    let adopted = db.adopt_parked_graphs(ctx).unwrap();
    assert_eq!(adopted.len(), 1);
    assert_eq!(adopted[0].0, d2);
    assert_eq!(adopted[0].1.placed(), 3);
    assert_eq!(evidence_rows(&db, ctx), vec![(e1, a, None), (e2, b, Some("garantiert die Verbindlichkeiten".into()))]);
    assert_eq!(db.list_ontology_edge_reviews(ctx).unwrap()[0].chunk_id, Some(b));
    assert!(db.get_chunks_with_ontology(ctx).unwrap().contains(&a));
    assert_eq!(db.count_parked_documents(ctx).unwrap(), 0);
}

#[test]
fn nothing_parked_without_graph_and_parked_rows_go_with_the_graph() {
    let db = db();
    let (ctx, _) = seed_context_with_chunk(&db, "plain");
    let d = doc(&db, ctx, "Text ohne Graph");
    chunk(&db, ctx, d, 0, 0, 4, "Text");
    let del = db.delete_document_parking(d).unwrap();
    assert_eq!((del.deleted, del.parked_evidence, del.parked_extracted), (true, 0, 0));
    assert_eq!(db.count_parked_documents(ctx).unwrap(), 0);

    let db = super::fixtures::db();
    let (ctx, d, _) = seeded(&db);
    db.delete_document(d).unwrap();
    assert_eq!(db.count_parked_documents(ctx).unwrap(), 1);
    db.delete_ontology_for_context(ctx).unwrap();
    assert_eq!(db.count_parked_documents(ctx).unwrap(), 0);
}

#[test]
fn image_text_replacement_carries_by_content() {
    let db = db();
    let (ctx, d, [_, e2]) = seeded(&db);
    let new_text = format!("Beschreibung: {B} Neu.");
    let (n, r) = db.replace_image_document_text(d, &new_text, &[nc(ctx, d, 0, 0, new_text.len(), &new_text)]).unwrap();
    assert_eq!(n, 1);
    // e2's quote is in the new text; e1's old chunk text (A+B) is not.
    assert_eq!(r.by_text, 2);
    assert_eq!(r.unplaced, 1);
    assert!(evidence_rows(&db, ctx).iter().any(|(e, _, _)| *e == e2));
    // The new text holds more than the old extracted chunk.
    assert_eq!(r.extracted_after, 0);
}

/// Every text value stored in the parked tables.
fn parked_texts(db: &Database) -> Vec<String> {
    let mut out = Vec::new();
    for t in ["ontology_parked_documents", "ontology_parked_chunks", "ontology_parked_evidence"] {
        let mut stmt = db.conn.prepare(&format!("SELECT * FROM {t}")).unwrap();
        let n = stmt.column_count();
        let mut rows = stmt.query([]).unwrap();
        while let Some(r) = rows.next().unwrap() {
            for i in 0..n {
                if let Ok(Some(v)) = r.get::<_, Option<String>>(i) {
                    out.push(v);
                }
            }
        }
    }
    out
}

#[test]
fn parking_keeps_no_document_text() {
    let db = db();
    let (_, d, _) = seeded(&db);
    db.delete_document(d).unwrap();
    let texts = parked_texts(&db);
    assert!(!texts.is_empty());
    // Only the fingerprint and the quote that was already stored as evidence.
    for v in &texts {
        assert!(!v.contains("Mittelstand in Deutschland") && !v.contains("Bund garantiert"), "text parked: {v}");
        assert!(!v.contains("bericht.pdf"));
    }
}

#[test]
fn parked_evidence_expires() {
    let db = db();
    let (ctx, d, _) = seeded(&db);
    db.delete_document(d).unwrap();
    let age = |days: i64| {
        db.conn
            .execute("UPDATE ontology_parked_documents SET parked_at = unixepoch() - ?1 * 86400", [days])
            .unwrap();
    };
    age(super::super::evidence_parking::PARKED_EVIDENCE_MAX_AGE_DAYS - 1);
    assert_eq!(db.purge_expired_parked_evidence().unwrap(), 0);
    age(super::super::evidence_parking::PARKED_EVIDENCE_MAX_AGE_DAYS + 1);
    assert_eq!(db.purge_expired_parked_evidence().unwrap(), 1);
    assert_eq!(db.count_parked_documents(ctx).unwrap(), 0);
    assert!(parked_texts(&db).is_empty());
}

#[test]
fn parking_the_same_text_again_merges() {
    let db = db();
    let (ctx, d, [e1, e2]) = seeded(&db);
    db.delete_document(d).unwrap();
    // Re-added, chunked and given its own evidence, but not adopted; deleted again.
    let d2 = doc(&db, ctx, &text());
    let (a, _) = split(&db, ctx, d2);
    let x = node(&db, ctx, "Neu", "Org").id;
    let e3 = edge(&db, ctx, a, x, x, "neu").id;
    db.delete_document(d2).unwrap();
    assert_eq!(db.count_parked_documents(ctx).unwrap(), 1);
    let d3 = doc(&db, ctx, &text());
    let (a3, b3) = split(&db, ctx, d3);
    assert_eq!(db.adopt_parked_graph(d3).unwrap().unwrap().placed(), 4);
    let edges: Vec<(i64, i64)> = evidence_rows(&db, ctx).into_iter().map(|(e, c, _)| (e, c)).collect();
    assert_eq!(edges, vec![(e1, a3), (e2, b3), (e3, a3)]);
}

#[test]
fn deleting_one_of_two_copies_hands_the_evidence_to_the_other() {
    let db = db();
    let (ctx, d, [e1, e2]) = seeded(&db);
    let twin = doc(&db, ctx, &text());
    let (a, b) = split(&db, ctx, twin);
    let del = db.delete_document_parking(d).unwrap();
    let (by, r) = del.adopted_by.expect("twin adopts");
    assert_eq!((by, r.placed()), (twin, 3));
    assert_eq!(evidence_rows(&db, ctx), vec![(e1, a, None), (e2, b, Some("garantiert die Verbindlichkeiten".into()))]);
    assert_eq!(db.count_parked_documents(ctx).unwrap(), 0);
}

#[test]
fn restore_from_backup_skips_foreign_edges_and_ambiguous_targets() {
    use super::super::evidence_restore::{match_target, restore_document, RestoreOutcome};
    let backup = db();
    let (_, d, _) = seeded(&backup);

    // Target: same collection name, the same file re-added and split; edge 1 joins
    // the same nodes, edge 2 has the same id but joins other nodes.
    let target = super::fixtures::db();
    let (ctx, fixture_chunk) = seed_context_with_chunk(&target, "carry");
    let td = doc(&target, ctx, &text());
    let (a, _) = split(&target, ctx, td);
    let kfw = node(&target, ctx, "KfW", "Org").id;
    let ms = node(&target, ctx, "Mittelstand", "Group").id;
    let x = node(&target, ctx, "Anderes", "Org").id;
    let y = node(&target, ctx, "Fremdes", "Org").id;
    let e1 = edge(&target, ctx, fixture_chunk, kfw, ms, "fördert").id;
    edge(&target, ctx, fixture_chunk, x, y, "anders");

    let preview = restore_document(&backup.conn, &target.conn, d, ctx, false).unwrap();
    let RestoreOutcome::Done { target_document, report } = restore_document(&backup.conn, &target.conn, d, ctx, true).unwrap()
    else {
        panic!("expected a restore")
    };
    assert_eq!(preview, RestoreOutcome::Done { target_document, report });
    assert_eq!(target_document, td);
    // e1 placed; e2's evidence row and review hit a foreign edge.
    assert_eq!((report.placed(), report.edge_mismatch, report.edge_missing), (1, 2, 0));
    assert!(evidence_rows(&target, ctx).contains(&(e1, a, None)));
    assert!(target.get_chunks_with_ontology(ctx).unwrap().contains(&a));

    // Edge id from another collection: also foreign.
    let target2 = super::fixtures::db();
    let (other, other_chunk) = seed_context_with_chunk(&target2, "other");
    let (ctx2, _) = seed_context_with_chunk(&target2, "carry2");
    let td2 = doc(&target2, ctx2, &text());
    split(&target2, ctx2, td2);
    let n1 = node(&target2, other, "KfW", "Org").id;
    let n2 = node(&target2, other, "Mittelstand", "Group").id;
    edge(&target2, other, other_chunk, n1, n2, "fördert");
    let RestoreOutcome::Done { report, .. } = restore_document(&backup.conn, &target2.conn, d, ctx2, true).unwrap() else {
        panic!("expected a restore")
    };
    assert_eq!((report.placed(), report.edge_mismatch + report.edge_missing), (0, 3));

    // Two target documents with the same text: never guessed.
    doc(&target, ctx, &text());
    assert!(matches!(restore_document(&backup.conn, &target.conn, d, ctx, false).unwrap(), RestoreOutcome::Ambiguous(ids) if ids.len() == 2));
    assert_eq!(match_target("f", &[(1, "g".into())]), RestoreOutcome::NoTarget { evidence: 0, extracted: 0 });
}
