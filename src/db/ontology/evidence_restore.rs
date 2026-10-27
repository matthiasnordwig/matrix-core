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
//!
//! The same carry ([`carry_graph`]) hands a document's evidence to the new
//! version that replaces it ([`Database::replace_document_by`]: a PDF read
//! again from its file, services `pipeline/resync.rs` and `pipeline/refresh.rs`).
//! There the new version is placed by content (`plan_carry` without
//! `same_text`): its chunks come from a newer chunker, so the old byte ranges
//! need not mean the same text even when the document text is unchanged —
//! the chunk texts and stored quotes are what both versions share.
use rusqlite::Connection;

use super::evidence_carry::{plan_carry, text_fingerprint, DocumentGraph, EvidenceCarryReport};
use super::evidence_carry_db::{apply_carry, preview_carry, read_chunk_spans, read_document_graph};
use crate::db::{CoreError, Database, Result};

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
    let report = carry_graph(target, &graph, target_document, true, write)?;
    Ok(RestoreOutcome::Done { target_document, report })
}

/// Place `graph` (read from any connection) onto the chunks `target_document`
/// has now in `target`; `same_text`: the old byte ranges are valid there.
/// `write = false` only counts.
pub fn carry_graph(target: &Connection, graph: &DocumentGraph, target_document: i64, same_text: bool, write: bool) -> Result<EvidenceCarryReport> {
    let new = read_chunk_spans(target, target_document)?;
    let plan = plan_carry(graph, same_text, &new);
    if write {
        apply_carry(target, graph, &plan, new.len())
    } else {
        preview_carry(target, graph, &plan, new.len())
    }
}

impl Database {
    /// Replace document `old` by its new version `new` (same collection, chunks
    /// already made): in one savepoint read what hangs at `old`'s chunks, delete
    /// `old` (its sheet tables too; nothing is parked), place the evidence,
    /// review links and extracted marks onto `new` by content (module comment)
    /// and confirm `new`'s `pending` source row (`source_files`). One savepoint,
    /// so a crash leaves either `old` with a still `pending` `new` (removed by the
    /// next update, `resync::leftovers`) or `new` alone — never both as final.
    /// What finds no place is counted in the report (`unplaced`), never silent.
    pub fn replace_document_by(&self, old: i64, new: i64) -> Result<EvidenceCarryReport> {
        self.check_replacement(old, new)?;
        self.with_sheet_drop("document_id = ?1", old, || {
            let graph = read_document_graph(&self.conn, old)?;
            self.conn.execute("DELETE FROM documents WHERE id = ?1", [old])?;
            self.conn.execute("UPDATE source_files SET pending = 0 WHERE document_id = ?1", [new])?;
            match graph {
                Some(g) if !g.is_empty() => carry_graph(&self.conn, &g, new, false, true),
                _ => Ok(EvidenceCarryReport { new_chunks: read_chunk_spans(&self.conn, new)?.len(), ..Default::default() }),
            }
        })
    }

    /// What [`Self::replace_document_by`] would carry, without writing (to tell
    /// a wrong file from the new version before anything is replaced).
    pub fn preview_replace_document_by(&self, old: i64, new: i64) -> Result<EvidenceCarryReport> {
        self.check_replacement(old, new)?;
        match read_document_graph(&self.conn, old)? {
            Some(g) if !g.is_empty() => carry_graph(&self.conn, &g, new, false, false),
            _ => Ok(EvidenceCarryReport { new_chunks: read_chunk_spans(&self.conn, new)?.len(), ..Default::default() }),
        }
    }

    fn check_replacement(&self, old: i64, new: i64) -> Result<()> {
        let contexts: Vec<i64> = {
            let mut stmt = self.conn.prepare("SELECT context_id FROM documents WHERE id IN (?1, ?2)")?;
            let rows = stmt.query_map([old, new], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        if old == new || contexts.len() != 2 || contexts[0] != contexts[1] {
            return Err(CoreError::InvalidState(format!("documents {old} and {new} are not two documents of one collection")));
        }
        Ok(())
    }
}
