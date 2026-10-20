//! Restore the graph evidence of a collection from an older backup of
//! `matrix.db` — for collections whose documents were re-split (deleted and
//! added again, re-chunked) before the evidence was carried over automatically.
//! Logic and rules: `db/ontology/evidence_restore.rs` (same placement and
//! checks as the app). Each backup document goes to the target document with
//! the same text; two target documents with that text stop the run.
//!
//!   cargo run --release --example restore_graph_evidence -- \
//!       --backup <old matrix.db> --db <target matrix.db> --context <id> \
//!       [--target-context <id>] [--apply]
//!
//! Default (or `--dry-run`): read-only on both databases, prints the counts.
//! `--apply` writes into `--db` in one transaction and refuses while the app
//! runs (process under `/Applications/Matrix LLM.app`) or the database has a
//! non-empty `-wal` file (open elsewhere or not cleanly closed). The backup is
//! always opened read-only and never migrated. Wrapper:
//! `scripts/restore_graph_evidence.sh`.
use std::process::ExitCode;

use app::db::ontology::evidence_carry::EvidenceCarryReport;
use app::db::ontology::evidence_restore::{restore_document, RestoreOutcome};
use rusqlite::{Connection, OpenFlags};

const APP_BUNDLE: &str = "/Applications/Matrix LLM.app";

struct Args {
    backup: String,
    db: String,
    context: i64,
    target_context: i64,
    apply: bool,
}

fn parse() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let (mut backup, mut db, mut context, mut target, mut apply) = (None, None, None, None, false);
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--backup" => backup = Some(val()?),
            "--db" => db = Some(val()?),
            "--context" => context = Some(val()?.parse::<i64>().map_err(|e| e.to_string())?),
            "--target-context" => target = Some(val()?.parse::<i64>().map_err(|e| e.to_string())?),
            "--apply" => apply = true,
            "--dry-run" => apply = false,
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    let context = context.ok_or("--context is required")?;
    Ok(Args {
        backup: backup.ok_or("--backup is required")?,
        db: db.ok_or("--db is required")?,
        context,
        target_context: target.unwrap_or(context),
        apply,
    })
}

fn open_ro(path: &str) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
}

/// Why writing into `db` now is unsafe, if it is.
fn busy_reason(db: &str) -> Option<String> {
    let running = std::process::Command::new("pgrep")
        .args(["-f", APP_BUNDLE])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if running {
        return Some(format!("the app is running ({APP_BUNDLE}) — quit it first"));
    }
    match std::fs::metadata(format!("{db}-wal")) {
        Ok(m) if m.len() > 0 => Some(format!("{db}-wal is not empty — the database is open elsewhere or was not closed cleanly")),
        _ => None,
    }
}

fn add(t: &mut EvidenceCarryReport, r: &EvidenceCarryReport) {
    t.old_chunks += r.old_chunks;
    t.new_chunks += r.new_chunks;
    t.evidence += r.evidence;
    t.by_text += r.by_text;
    t.by_place += r.by_place;
    t.by_labels += r.by_labels;
    t.by_overlap += r.by_overlap;
    t.unplaced += r.unplaced;
    t.merged += r.merged;
    t.edge_missing += r.edge_missing;
    t.edge_mismatch += r.edge_mismatch;
    t.extracted_before += r.extracted_before;
    t.extracted_after += r.extracted_after;
}

/// One pass over all backup documents; `false` when a document was ambiguous.
fn pass(backup: &Connection, target: &Connection, a: &Args, docs: &[(i64, String)], write: bool, print: bool) -> app::Result<(bool, EvidenceCarryReport)> {
    let mut total = EvidenceCarryReport::default();
    let mut clean = true;
    for (id, name) in docs {
        let out = restore_document(backup, target, *id, a.target_context, write)?;
        match out {
            RestoreOutcome::Nothing => {}
            RestoreOutcome::NoTarget { evidence, extracted } => {
                if print {
                    println!("  backup document {id} \"{name}\": no target document with the same text — {evidence} evidence, {extracted} extracted chunks not restorable");
                }
                total.evidence += evidence;
                total.unplaced += evidence;
            }
            RestoreOutcome::Ambiguous(ids) => {
                clean = false;
                println!("  backup document {id} \"{name}\": several target documents with the same text {ids:?} — not restored, resolve first");
            }
            RestoreOutcome::Done { target_document, report } => {
                if print {
                    println!("  backup document {id} \"{name}\" → target document {target_document}: {}", report.summary());
                }
                add(&mut total, &report);
            }
        }
    }
    Ok((clean, total))
}

fn run(a: &Args) -> Result<bool, Box<dyn std::error::Error>> {
    if let Some(why) = a.apply.then(|| busy_reason(&a.db)).flatten() {
        return Err(format!("not writing: {why}").into());
    }
    let backup = open_ro(&a.backup)?;
    let target = if a.apply { Connection::open(&a.db)? } else { open_ro(&a.db)? };
    target.execute_batch("PRAGMA foreign_keys = ON;")?;
    let docs: Vec<(i64, String)> = {
        let mut stmt = backup.prepare("SELECT id, name FROM documents WHERE context_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map([a.context], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    println!("{} backup context {} → target context {}", if a.apply { "APPLY:" } else { "DRY RUN:" }, a.context, a.target_context);
    let (clean, preview) = pass(&backup, &target, a, &docs, false, !a.apply)?;
    if !clean {
        println!("stopped: ambiguous target documents (nothing written)");
        return Ok(false);
    }
    let total = if a.apply {
        target.execute_batch("BEGIN IMMEDIATE;")?;
        let (_, done) = pass(&backup, &target, a, &docs, true, true)?;
        target.execute_batch("COMMIT;")?;
        done
    } else {
        preview
    };
    println!("total: {}", total.summary());
    println!(
        "  restorable: {} of {} pieces of evidence ({} new rows, {} merged into a row of the same edge and chunk); {} without a place, {} edge/review gone, {} foreign edge skipped",
        total.placed(),
        total.evidence,
        total.placed() - total.merged,
        total.merged,
        total.unplaced,
        total.edge_missing,
        total.edge_mismatch
    );
    if !a.apply {
        println!("nothing written (dry run); --apply writes into {}", a.db);
    }
    Ok(true)
}

fn main() -> ExitCode {
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\nusage: restore_graph_evidence --backup <old.db> --db <target.db> --context <id> [--target-context <id>] [--apply|--dry-run]");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
