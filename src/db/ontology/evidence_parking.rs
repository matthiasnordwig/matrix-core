//! A deleted document's graph evidence, kept until the same file comes back
//! (schema_v68). Deleting a document and adding it again is the only way to
//! re-split a structural (PDF layout) collection — the PDF bytes are not stored —
//! and used to lose every piece of evidence of the graph built from it, while
//! its nodes and edges stayed.
//!
//! **What is kept (no text).** Deleting must still delete the document's
//! text, so only this is parked: the fingerprint of the document text
//! (FNV-1a + length), the byte ranges and extracted marks of the old chunks
//! that were extracted or carried evidence, and the evidence itself (edge and
//! review ids plus the quote already stored with the evidence row). Adoption
//! needs no text: a document with the same fingerprint has the same text, so
//! the old ranges are valid there (`plan_carry` with `same_text`).
//!
//! **Flow.** [`Database::delete_document_parking`] (via `delete_document`)
//! parks inside its own savepoint; when another document of the context has
//! the same text and chunks, it takes the evidence over at once (two copies of
//! one file, one deleted). Otherwise [`Database::adopt_parked_graph`] carries
//! it onto the same file added again — called by the app after a document got
//! its chunks, and for the whole context before an extraction run
//! ([`Database::adopt_parked_graphs`]). A document without chunks adopts
//! nothing. Parking the same text again merges into the existing parked rows.
//!
//! **How long.** [`PARKED_EVIDENCE_MAX_AGE_DAYS`]; older rows are removed at
//! every `Database::open` ([`Database::purge_expired_parked_evidence`]).
//! Parked rows also go with their edges/reviews (FK cascade), the context, and
//! `delete_ontology_for_context`.
use rusqlite::{params, OptionalExtension};

use super::evidence_carry::{text_fingerprint, DocumentGraph, EvidenceCarryReport, EvidenceKind, OldChunk};
use super::evidence_carry_db::{read_chunk_spans, read_document_graph, row_to_evidence};
use crate::db::{Database, Result};

/// Days a deleted document's evidence waits for the same file to come back.
pub const PARKED_EVIDENCE_MAX_AGE_DAYS: i64 = 30;

/// What [`Database::delete_document_parking`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeletedDocument {
    pub deleted: bool,
    /// Pieces of evidence and extracted chunks parked.
    pub parked_evidence: usize,
    pub parked_extracted: usize,
    /// Another document of the context with the same text took them over at
    /// once: its id and the carry-over counts.
    pub adopted_by: Option<(i64, EvidenceCarryReport)>,
}

impl Database {
    /// Delete a document (sheet tables in the same savepoint), parking what
    /// hangs at its chunks first (module comment) and handing it to a twin
    /// document of the same text right away if there is one.
    pub fn delete_document_parking(&self, id: i64) -> Result<DeletedDocument> {
        self.with_sheet_drop("document_id = ?1", id, || {
            let twin_key: Option<(i64, String)> = self
                .conn
                .query_row("SELECT context_id, extracted_text FROM documents WHERE id = ?1", [id], |r| {
                    Ok((r.get(0)?, r.get::<_, Option<String>>(1)?))
                })
                .optional()?
                .map(|(c, t)| (c, text_fingerprint(t.as_deref().unwrap_or(""))));
            let (parked_evidence, parked_extracted) = self.park_document_graph(id)?;
            let deleted = self.conn.execute("DELETE FROM documents WHERE id = ?1", [id])? > 0;
            let mut adopted_by = None;
            if let (Some((context_id, fingerprint)), true) = (twin_key, parked_evidence + parked_extracted > 0) {
                for twin in self.documents_with_fingerprint(context_id, &fingerprint)? {
                    if let Some(r) = self.adopt_parked_graph(twin)? {
                        adopted_by = Some((twin, r));
                        break;
                    }
                }
            }
            Ok(DeletedDocument { deleted, parked_evidence, parked_extracted, adopted_by })
        })
    }

    /// Park what hangs at the chunks of `document_id` (no text, module
    /// comment); an older parking of the same text in the context is merged
    /// in. Returns (evidence pieces, extracted chunks) parked; (0, 0) writes
    /// nothing.
    pub(crate) fn park_document_graph(&self, document_id: i64) -> Result<(usize, usize)> {
        let Some(graph) = read_document_graph(&self.conn, document_id)? else { return Ok((0, 0)) };
        if graph.is_empty() {
            return Ok((0, 0));
        }
        self.conn.execute(
            "INSERT INTO ontology_parked_documents (context_id, text_fingerprint) VALUES (?1, ?2)",
            params![graph.context_id, graph.text_fingerprint],
        )?;
        let parked_doc = self.conn.last_insert_rowid();
        // Same text parked before: its chunks join this parking (same offsets).
        self.conn.execute(
            "UPDATE ontology_parked_chunks SET parked_document_id = ?1
              WHERE parked_document_id IN (SELECT id FROM ontology_parked_documents
                                            WHERE context_id = ?2 AND text_fingerprint = ?3 AND id <> ?1)",
            params![parked_doc, graph.context_id, graph.text_fingerprint],
        )?;
        self.conn.execute(
            "DELETE FROM ontology_parked_documents WHERE context_id = ?1 AND text_fingerprint = ?2 AND id <> ?3",
            params![graph.context_id, graph.text_fingerprint, parked_doc],
        )?;
        let mut parked_chunk = std::collections::HashMap::new();
        for c in &graph.chunks {
            if !c.extracted && !graph.evidence.iter().any(|e| e.chunk_id == c.id) {
                continue;
            }
            self.conn.execute(
                "INSERT INTO ontology_parked_chunks (parked_document_id, char_start, char_end, extracted)
                 VALUES (?1, ?2, ?3, ?4)",
                params![parked_doc, c.char_start, c.char_end, c.extracted],
            )?;
            parked_chunk.insert(c.id, self.conn.last_insert_rowid());
        }
        for e in &graph.evidence {
            let review = match e.kind {
                EvidenceKind::Review(id) => Some(id),
                EvidenceKind::Edge => None,
            };
            self.conn.execute(
                "INSERT INTO ontology_parked_evidence (parked_chunk_id, edge_id, review_id, evidence) VALUES (?1, ?2, ?3, ?4)",
                params![parked_chunk[&e.chunk_id], e.edge_id, review, e.evidence],
            )?;
        }
        Ok((graph.evidence.len(), graph.chunks.iter().filter(|c| c.extracted).count()))
    }

    /// Carry the parked evidence of a deleted document with the same text in
    /// the same context onto `document_id`'s chunks, then drop the parked rows
    /// (one savepoint). `None`: nothing parked for it, or it has no chunks yet.
    pub fn adopt_parked_graph(&self, document_id: i64) -> Result<Option<EvidenceCarryReport>> {
        let doc: Option<(i64, Option<String>)> = self
            .conn
            .query_row("SELECT context_id, extracted_text FROM documents WHERE id = ?1", [document_id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        let Some((context_id, text)) = doc else { return Ok(None) };
        let fingerprint = text_fingerprint(text.as_deref().unwrap_or(""));
        let parked: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM ontology_parked_documents WHERE context_id = ?1 AND text_fingerprint = ?2
                  ORDER BY id DESC LIMIT 1",
                params![context_id, fingerprint],
                |r| r.get(0),
            )
            .optional()?;
        let Some(parked) = parked else { return Ok(None) };
        if read_chunk_spans(&self.conn, document_id)?.is_empty() {
            return Ok(None);
        }
        self.in_savepoint("adopt_parked", || {
            let graph = self.parked_graph(parked, context_id, document_id, fingerprint)?;
            let report = self.carry_onto_current_chunks(&graph, Some(true))?;
            self.conn.execute("DELETE FROM ontology_parked_documents WHERE id = ?1", [parked])?;
            Ok(Some(report))
        })
    }

    /// [`Self::adopt_parked_graph`] for every document of the context that has
    /// parked evidence waiting; `(document_id, report)` per adoption.
    pub fn adopt_parked_graphs(&self, context_id: i64) -> Result<Vec<(i64, EvidenceCarryReport)>> {
        let waiting: Vec<String> = {
            let mut stmt =
                self.conn.prepare("SELECT DISTINCT text_fingerprint FROM ontology_parked_documents WHERE context_id = ?1")?;
            let rows = stmt.query_map([context_id], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut out = Vec::new();
        for fingerprint in waiting {
            for id in self.documents_with_fingerprint(context_id, &fingerprint)? {
                if let Some(r) = self.adopt_parked_graph(id)? {
                    out.push((id, r));
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Number of deleted documents with parked evidence in a context.
    pub fn count_parked_documents(&self, context_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM ontology_parked_documents WHERE context_id = ?1",
            [context_id],
            |r| r.get(0),
        )?)
    }

    /// Remove parked evidence older than [`PARKED_EVIDENCE_MAX_AGE_DAYS`]
    /// (run at every `Database::open`). Returns the parked documents removed.
    pub fn purge_expired_parked_evidence(&self) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM ontology_parked_documents WHERE parked_at < unixepoch() - ?1 * 86400",
            [PARKED_EVIDENCE_MAX_AGE_DAYS],
        )?)
    }

    /// Documents of a context whose text has `fingerprint`, by id.
    fn documents_with_fingerprint(&self, context_id: i64, fingerprint: &str) -> Result<Vec<i64>> {
        let docs: Vec<(i64, Option<String>)> = {
            let mut stmt = self.conn.prepare("SELECT id, extracted_text FROM documents WHERE context_id = ?1 ORDER BY id")?;
            let rows = stmt.query_map([context_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(docs
            .into_iter()
            .filter(|(_, t)| text_fingerprint(t.as_deref().unwrap_or("")) == fingerprint)
            .map(|(id, _)| id)
            .collect())
    }

    /// The parked rows of `parked` as a [`DocumentGraph`] for `document_id`
    /// (old chunk ids = parked chunk ids, no texts).
    fn parked_graph(&self, parked: i64, context_id: i64, document_id: i64, fingerprint: String) -> Result<DocumentGraph> {
        let chunks = {
            let mut stmt = self.conn.prepare(
                "SELECT id, char_start, char_end, extracted FROM ontology_parked_chunks
                  WHERE parked_document_id = ?1 ORDER BY char_start, id",
            )?;
            let rows = stmt.query_map([parked], |r| {
                Ok(OldChunk { id: r.get(0)?, char_start: r.get(1)?, char_end: r.get(2)?, text: String::new(), extracted: r.get(3)? })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let evidence = {
            let mut stmt = self.conn.prepare(
                "SELECT p.review_id, p.edge_id, p.parked_chunk_id, p.evidence, s.label, t.label
                   FROM ontology_parked_evidence p
                   JOIN ontology_parked_chunks c ON c.id = p.parked_chunk_id
                   JOIN ontology_edges e ON e.id = p.edge_id
                   JOIN ontology_nodes s ON s.id = e.source_id
                   JOIN ontology_nodes t ON t.id = e.target_id
                  WHERE c.parked_document_id = ?1
                  ORDER BY p.edge_id, p.parked_chunk_id",
            )?;
            let rows = stmt.query_map([parked], row_to_evidence)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(DocumentGraph { context_id, document_id, text_fingerprint: fingerprint, chunks, evidence })
    }
}
