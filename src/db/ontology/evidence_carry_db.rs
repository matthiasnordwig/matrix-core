//! Carrying a document's graph evidence over onto its new chunks — the DB half
//! (rules: `evidence_carry.rs`). [`Database::replace_document_chunks`] is the
//! one way to make a document's chunks anew: in one savepoint it reads what
//! hangs at the old chunks, deletes them (the `ON DELETE CASCADE` takes the
//! evidence rows, extracted marks, quarantine and batch states along), lets the
//! caller create the new chunks, and writes the evidence and the extracted
//! marks onto them — no state in between is ever visible or left behind.
//! Quarantine and batch states are not carried: their chunks count as not
//! extracted and are simply done again.
//!
//! The readers and the writer are free functions over a plain connection, so
//! the restore tool (`core/examples/restore_graph_evidence.rs`) can read an old
//! backup read-only (without migrating it) and write into another database.
use rusqlite::{params, Connection, OptionalExtension};

use super::evidence_carry::{
    plan_carry, text_fingerprint, CarryPlan, DocumentGraph, EvidenceCarryReport, EvidenceKind, MatchTier,
    NewChunkSpan, OldChunk, OldEvidence,
};
use crate::db::{CoreError, Database, Result};
use crate::text_match::fold;

/// What hangs at the chunks of `document_id`: chunks with their extracted
/// mark, edge evidence rows and review chunk links, with the edges' node
/// labels. `None` for an unknown document.
pub fn read_document_graph(conn: &Connection, document_id: i64) -> Result<Option<DocumentGraph>> {
    let doc: Option<(i64, Option<String>)> = conn
        .query_row("SELECT context_id, extracted_text FROM documents WHERE id = ?1", [document_id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let Some((context_id, text)) = doc else { return Ok(None) };
    let chunks = {
        let mut stmt = conn.prepare(
            "SELECT c.id, c.char_start, c.char_end, c.text,
                    EXISTS (SELECT 1 FROM ontology_extracted_chunks x
                             WHERE x.chunk_id = c.id AND x.context_id = c.context_id)
               FROM chunks c WHERE c.document_id = ?1 ORDER BY c.chunk_index",
        )?;
        let rows = stmt.query_map([document_id], |r| {
            Ok(OldChunk { id: r.get(0)?, char_start: r.get(1)?, char_end: r.get(2)?, text: r.get(3)?, extracted: r.get(4)? })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let evidence = {
        let mut stmt = conn.prepare(
            "SELECT NULL, ec.edge_id, ec.chunk_id, ec.evidence, s.label, t.label
               FROM ontology_edge_chunks ec
               JOIN chunks c ON c.id = ec.chunk_id
               JOIN ontology_edges e ON e.id = ec.edge_id
               JOIN ontology_nodes s ON s.id = e.source_id
               JOIN ontology_nodes t ON t.id = e.target_id
              WHERE c.document_id = ?1
             UNION ALL
             SELECT r.id, r.edge_id, r.chunk_id, r.evidence, s.label, t.label
               FROM ontology_edge_reviews r
               JOIN chunks c ON c.id = r.chunk_id
               JOIN ontology_edges e ON e.id = r.edge_id
               JOIN ontology_nodes s ON s.id = e.source_id
               JOIN ontology_nodes t ON t.id = e.target_id
              WHERE c.document_id = ?1
             ORDER BY 2, 3",
        )?;
        let rows = stmt.query_map([document_id], row_to_evidence)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(Some(DocumentGraph {
        context_id,
        document_id,
        text_fingerprint: text_fingerprint(text.as_deref().unwrap_or("")),
        chunks,
        evidence,
    }))
}

/// Row `(review id or NULL, edge, chunk, evidence, source label, target label)`.
pub(super) fn row_to_evidence(r: &rusqlite::Row) -> rusqlite::Result<OldEvidence> {
    let review: Option<i64> = r.get(0)?;
    Ok(OldEvidence {
        kind: review.map_or(EvidenceKind::Edge, EvidenceKind::Review),
        edge_id: r.get(1)?,
        chunk_id: r.get(2)?,
        evidence: r.get(3)?,
        labels: [r.get(4)?, r.get(5)?],
    })
}

/// The chunks `document_id` has now, in order.
pub fn read_chunk_spans(conn: &Connection, document_id: i64) -> Result<Vec<NewChunkSpan>> {
    let mut stmt = conn.prepare(
        "SELECT id, char_start, char_end, text, is_omitted FROM chunks WHERE document_id = ?1 ORDER BY chunk_index",
    )?;
    let rows = stmt.query_map([document_id], |r| {
        Ok(NewChunkSpan { id: r.get(0)?, char_start: r.get(1)?, char_end: r.get(2)?, text: r.get(3)?, is_omitted: r.get(4)? })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Write `plan` (made for `graph` against `new_chunks` chunks): evidence rows
/// (an edge that already has evidence at the chunk keeps its row, a missing
/// quote is filled in), review chunk links (only where none is set), extracted
/// marks (in the new chunk's own context). Evidence whose edge is missing, or
/// lives in another context, or joins other nodes (labels) than in `graph`, is
/// skipped and counted (`edge_missing`, `edge_mismatch`). Runs in the caller's transaction, if any.
pub fn apply_carry(conn: &Connection, graph: &DocumentGraph, plan: &CarryPlan, new_chunks: usize) -> Result<EvidenceCarryReport> {
    carry(conn, graph, plan, new_chunks, true)
}

/// The counts [`apply_carry`] would report, without writing (read-only
/// connections work).
pub fn preview_carry(conn: &Connection, graph: &DocumentGraph, plan: &CarryPlan, new_chunks: usize) -> Result<EvidenceCarryReport> {
    carry(conn, graph, plan, new_chunks, false)
}

fn exists(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> Result<bool> {
    Ok(conn.query_row(sql, p, |x| x.get(0))?)
}

fn carry(conn: &Connection, graph: &DocumentGraph, plan: &CarryPlan, new_chunks: usize, write: bool) -> Result<EvidenceCarryReport> {
    let mut r = EvidenceCarryReport {
        old_chunks: graph.chunks.len(),
        new_chunks,
        evidence: graph.evidence.len(),
        unplaced: plan.unplaced.len(),
        extracted_before: graph.chunks.iter().filter(|c| c.extracted).count(),
        extracted_after: plan.extracted.len(),
        ..Default::default()
    };
    // (edge, chunk) pairs a preview has already "written".
    let mut seen = std::collections::HashSet::new();
    for &(ei, chunk_id, tier) in &plan.placed {
        let ev = &graph.evidence[ei];
        // The edge must live in the target chunk's context and still join the
        // same two nodes (by label) as where the evidence was read; a restore
        // from a backup could otherwise hit an unrelated edge with the same id.
        let edge: Option<(bool, String, String)> = conn
            .query_row(
                "SELECT e.context_id = (SELECT context_id FROM chunks WHERE id = ?2), s.label, t.label
                   FROM ontology_edges e
                   JOIN ontology_nodes s ON s.id = e.source_id
                   JOIN ontology_nodes t ON t.id = e.target_id
                  WHERE e.id = ?1",
                [ev.edge_id, chunk_id],
                |x| Ok((x.get::<_, Option<bool>>(0)?.unwrap_or(false), x.get(1)?, x.get(2)?)),
            )
            .optional()?;
        let review_there = match ev.kind {
            EvidenceKind::Edge => true,
            EvidenceKind::Review(id) => exists(
                conn,
                "SELECT EXISTS (SELECT 1 FROM ontology_edge_reviews WHERE id = ?1 AND edge_id = ?2)",
                [id, ev.edge_id],
            )?,
        };
        match edge {
            None => {
                r.edge_missing += 1;
                continue;
            }
            Some((same_context, s, t)) if !same_context || fold(&s) != fold(&ev.labels[0]) || fold(&t) != fold(&ev.labels[1]) || !review_there => {
                r.edge_mismatch += 1;
                continue;
            }
            Some(_) => {}
        }
        let fresh = match (ev.kind, write) {
            (EvidenceKind::Edge, true) => {
                let n = conn.execute(
                    "INSERT OR IGNORE INTO ontology_edge_chunks (edge_id, chunk_id, evidence) VALUES (?1, ?2, ?3)",
                    params![ev.edge_id, chunk_id, ev.evidence],
                )?;
                if n == 0 && ev.evidence.is_some() {
                    conn.execute(
                        "UPDATE ontology_edge_chunks SET evidence = COALESCE(evidence, ?3) WHERE edge_id = ?1 AND chunk_id = ?2",
                        params![ev.edge_id, chunk_id, ev.evidence],
                    )?;
                }
                n > 0
            }
            (EvidenceKind::Edge, false) => {
                seen.insert((ev.edge_id, chunk_id))
                    && !exists(
                        conn,
                        "SELECT EXISTS (SELECT 1 FROM ontology_edge_chunks WHERE edge_id = ?1 AND chunk_id = ?2)",
                        [ev.edge_id, chunk_id],
                    )?
            }
            (EvidenceKind::Review(id), true) => {
                conn.execute(
                    "UPDATE ontology_edge_reviews SET chunk_id = ?2 WHERE id = ?1 AND chunk_id IS NULL",
                    params![id, chunk_id],
                )? > 0
            }
            (EvidenceKind::Review(id), false) => {
                seen.insert((-id, 0))
                    && exists(conn, "SELECT EXISTS (SELECT 1 FROM ontology_edge_reviews WHERE id = ?1 AND chunk_id IS NULL)", [id])?
            }
        };
        if !fresh {
            r.merged += 1;
        }
        match tier {
            MatchTier::EvidenceText => r.by_text += 1,
            MatchTier::SinglePlace => r.by_place += 1,
            MatchTier::Labels => r.by_labels += 1,
            MatchTier::Overlap => r.by_overlap += 1,
        }
    }
    if write {
        for &chunk_id in &plan.extracted {
            conn.execute(
                "INSERT OR IGNORE INTO ontology_extracted_chunks (context_id, chunk_id)
                 SELECT context_id, id FROM chunks WHERE id = ?1",
                [chunk_id],
            )?;
        }
    }
    Ok(r)
}

impl Database {
    /// See [`read_document_graph`].
    pub fn document_graph(&self, document_id: i64) -> Result<Option<DocumentGraph>> {
        read_document_graph(&self.conn, document_id)
    }

    /// Make the chunks of `document_id` anew while keeping the graph's
    /// evidence (module comment): one savepoint reads what hangs at the old
    /// chunks, deletes them, runs `build` (which creates the new chunks and may
    /// change the document text), and carries everything over. An `Err` from
    /// `build` rolls all of it back. Nests inside an open transaction.
    pub fn replace_document_chunks<T, E: From<CoreError>>(
        &self,
        document_id: i64,
        build: impl FnOnce() -> std::result::Result<T, E>,
    ) -> std::result::Result<(T, EvidenceCarryReport), E> {
        self.in_savepoint("replace_chunks", || {
            let graph = read_document_graph(&self.conn, document_id)?;
            self.conn
                .execute("DELETE FROM chunks WHERE document_id = ?1", [document_id])
                .map_err(CoreError::from)?;
            let out = build()?;
            let report = match graph {
                Some(g) => self.carry_onto_current_chunks(&g, None)?,
                None => EvidenceCarryReport::default(),
            };
            Ok((out, report))
        })
    }

    /// Carry `graph` onto the chunks its document has now. `same_text`:
    /// `None` = compare the document's current text with the graph's
    /// fingerprint. Nothing to carry → only the chunk counts.
    pub(super) fn carry_onto_current_chunks(&self, graph: &DocumentGraph, same_text: Option<bool>) -> Result<EvidenceCarryReport> {
        let new = read_chunk_spans(&self.conn, graph.document_id)?;
        if graph.is_empty() {
            return Ok(EvidenceCarryReport { old_chunks: graph.chunks.len(), new_chunks: new.len(), ..Default::default() });
        }
        let same = match same_text {
            Some(s) => s,
            None => {
                let text: Option<String> = self
                    .conn
                    .query_row("SELECT extracted_text FROM documents WHERE id = ?1", [graph.document_id], |r| r.get(0))
                    .optional()?
                    .flatten();
                text_fingerprint(text.as_deref().unwrap_or("")) == graph.text_fingerprint
            }
        };
        let plan = plan_carry(graph, same, &new);
        apply_carry(&self.conn, graph, &plan, new.len())
    }
}
