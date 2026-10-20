//! Restoring graph evidence from an older backup of `matrix.db` onto the
//! chunks a collection has today — the logic behind
//! `core/examples/restore_graph_evidence.rs` (wrapper
//! `scripts/restore_graph_evidence.sh`), for evidence lost before it was carried
//! over automatically. Works on two plain connections: the backup is only read
//! (never migrated), the target is written only by [`restore_document`] with
//! `write`, inside the caller's transaction.
//!
//! A backup document is matched to the target document of the target
//! collection with the same text fingerprint; two target documents with that
//! text are ambiguous and reported, never guessed. Placement and checks are
//! the app's (`evidence_carry.rs`, `apply_carry`): evidence of an edge that is
//! missing in the target, lives in another collection or joins other nodes
//! (labels) is skipped and counted.
use rusqlite::Connection;

use super::evidence_carry::{plan_carry, text_fingerprint, EvidenceCarryReport};
use super::evidence_carry_db::{apply_carry, preview_carry, read_chunk_spans, read_document_graph};
use crate::db::Result;

/// What happened to one backup document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// Unknown in the backup, or nothing hangs at it.
    Nothing,
    /// No target document with the same text: pieces of evidence and
    /// extracted chunks that cannot be restored.
    NoTarget { evidence: usize, extracted: usize },
    /// Several target documents with the same text (ids) — not restored.
    Ambiguous(Vec<i64>),
    /// Placed onto `target_document` (written only with `write`).
    Done { target_document: i64, report: EvidenceCarryReport },
}

/// The target document for a text fingerprint among `(id, fingerprint)`.
pub fn match_target(fingerprint: &str, targets: &[(i64, String)]) -> RestoreOutcome {
    let hits: Vec<i64> = targets.iter().filter(|(_, f)| f == fingerprint).map(|(id, _)| *id).collect();
    match hits.as_slice() {
        [] => RestoreOutcome::NoTarget { evidence: 0, extracted: 0 },
        [one] => RestoreOutcome::Done { target_document: *one, report: EvidenceCarryReport::default() },
        _ => RestoreOutcome::Ambiguous(hits),
    }
}

/// `(document id, text fingerprint)` of every document of a collection.
pub fn document_fingerprints(conn: &Connection, context_id: i64) -> Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare("SELECT id, COALESCE(extracted_text, '') FROM documents WHERE context_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map([context_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.map(|r| r.map(|(id, t)| (id, text_fingerprint(&t)))).collect::<rusqlite::Result<_>>()?)
}

/// Restore what hung at `backup_document` (in `backup`) onto the matching
/// document of `target_context` (in `target`); `write = false` only counts.
pub fn restore_document(
    backup: &Connection,
    target: &Connection,
    backup_document: i64,
    target_context: i64,
    write: bool,
) -> Result<RestoreOutcome> {
    let Some(graph) = read_document_graph(backup, backup_document)? else { return Ok(RestoreOutcome::Nothing) };
    if graph.is_empty() {
        return Ok(RestoreOutcome::Nothing);
    }
    let target_document = match match_target(&graph.text_fingerprint, &document_fingerprints(target, target_context)?) {
        RestoreOutcome::Done { target_document, .. } => target_document,
        RestoreOutcome::NoTarget { .. } => {
            return Ok(RestoreOutcome::NoTarget {
                evidence: graph.evidence.len(),
                extracted: graph.chunks.iter().filter(|c| c.extracted).count(),
            })
        }
        other => return Ok(other),
    };
    let new = read_chunk_spans(target, target_document)?;
    let plan = plan_carry(&graph, true, &new);
    let report = if write { apply_carry(target, &graph, &plan, new.len())? } else { preview_carry(target, &graph, &plan, new.len())? };
    Ok(RestoreOutcome::Done { target_document, report })
}
