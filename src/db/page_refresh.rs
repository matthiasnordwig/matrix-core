//! PDF documents read before the structural chunker recorded page changes inside a
//! chunk (`chunking/pages.rs`, `metadata.page_breaks`): their passages name only the
//! start page, so `[p. N]` and the page of a quote (`verify_citations`) stay
//! approximate. The PDF bytes are not stored, so only reading the file again helps —
//! which also brings page pictures to documents read before image search existed
//! (schema_v61, 2026-10-11; every such document is older than `page_breaks` too).
//!
//! [`is_outdated_pdf`] decides (pure); [`Database::outdated_pdf_documents`] applies it to
//! one collection. Readers: the update of a collection from its folders in the MCP
//! stdio program (services `pipeline/resync.rs`, automatic) and the hint
//! "N older documents can be updated" in the app (services `pipeline/refresh.rs`).
//!
//! **Rule:** a PDF (by file name) in a structural collection (other chunkers never
//! write `page_breaks`, reading again would gain nothing) with at least one chunk,
//! none of which has the key `page_breaks`. Every structural PDF chunk carries the key
//! today, also on one page (`[]`, `chunking/pages.rs`; OCR chunks of scanned pages too,
//! services `pdf_page_chunks.rs`), so a document read now never counts as old — the
//! key is a positive mark, not a date: a PDF read after 2026-10-23 by an app or plugin
//! that did not write it yet is found as well. Documents of the short time when only
//! chunks over a page break got the key, none of which runs over a page, are read
//! again once. A document without chunks gains no pages and stays out. Missing page
//! pictures are no criterion of their own: whether a document has graphics pages
//! cannot be told without its file.
use rusqlite::params;

use crate::db::{Database, Result};

/// What [`is_outdated_pdf`] looks at for one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfFacts<'a> {
    /// File name (`zip_entry` when set, else `name`).
    pub file_name: &'a str,
    /// Number of chunks of the document.
    pub chunks: i64,
    /// Some chunk of the document carries the key `page_breaks`.
    pub has_page_breaks: bool,
    /// The collection chunks structurally.
    pub structural: bool,
}

/// Whether reading the file again would give the document its page changes (module comment).
pub fn is_outdated_pdf(f: &PdfFacts) -> bool {
    f.structural && f.file_name.to_ascii_lowercase().ends_with(".pdf") && f.chunks > 0 && !f.has_page_breaks
}

impl Database {
    /// `(document id, file name)` of the outdated PDF documents of a collection, by id.
    pub fn outdated_pdf_documents(&self, context_id: i64) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT d.id, COALESCE(d.zip_entry, d.name),
                    (SELECT COUNT(*) FROM chunks c WHERE c.context_id = d.context_id AND c.document_id = d.id),
                    EXISTS (SELECT 1 FROM chunks c
                             WHERE c.context_id = d.context_id AND c.document_id = d.id
                               AND c.metadata LIKE '%\"page_breaks\"%'),
                    x.chunking_strategy = 'structural'
               FROM documents d JOIN contexts x ON x.id = d.context_id
              WHERE d.context_id = ?1
              ORDER BY d.id",
        )?;
        let rows = stmt.query_map(params![context_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, file_name, chunks, has_page_breaks, structural) = row?;
            let facts = PdfFacts { file_name: &file_name, chunks, has_page_breaks, structural };
            if is_outdated_pdf(&facts) {
                out.push((id, file_name));
            }
        }
        Ok(out)
    }
}
