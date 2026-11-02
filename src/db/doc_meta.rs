//! Search filter by sender, date and folder (schema_v75): what each document says about them, and
//! the documents a [`DocFilter`] lets through.
//!
//! **Where the facts come from.** Sender and date are read from the header block that
//! `ingest/mail` writes at the top of a mail's Markdown (`**From:** …`, `**Date:** 2001-10-05 14:30
//! -07:00`), never from the body, where forwarded mails have the same bold lines. An attachment
//! (`mail.eml › report.pdf`) carries the sender and date of its mail ([`MailMetaTracker`]: the
//! documents of one upload come mail first, its attachments right after). Other documents have no
//! sender or date yet. The folder is the directory of the document's path in the collection: the
//! path inside a zip, else the folder of the file on disk (`record_source_file`); an attachment
//! sits in its mail's folder. All three are stored lowercased, the folder with `/` separators.
//!
//! **Filtering.** [`Database::narrow_scope`] turns a filter into the document ids that the
//! retrieval's `doc_ids` scope takes (intersected with a file selection). The columns sit behind
//! `extracted_text` in the row, so the query reads only the covering index `idx_documents_filter`
//! (named in the query: without table statistics SQLite would pick `idx_documents_context`
//! and read every row; `doc_meta_tests.rs` checks the plan). Text fields match anywhere (`LIKE`, escaped); a folder
//! written with slashes (`/inbox/`) matches whole path parts only.

use std::collections::HashSet;

use rusqlite::{params, OptionalExtension};

use super::models::{DocFilter, DocMeta};
use super::{CoreError, Database, Result};

/// Between a mail's path and an attachment's name (`ingest/mail::ATTACHMENT_SEP`).
pub const ATTACHMENT_SEP: &str = " › ";
/// Characters of a mail's text read for its header block.
const HEADER_CHARS: usize = 4_000;

/// Sender (lowercased) and date (`YYYY-MM-DD`) from the header block at the top of a mail's Markdown.
pub fn mail_header(markdown: &str) -> (Option<String>, Option<String>) {
    let lines = markdown.lines().map(str::trim).skip_while(|l| l.is_empty() || l.starts_with("# "));
    let (mut sender, mut date) = (None, None);
    for line in lines.take_while(|l| l.starts_with("**")) {
        if let Some(v) = line.strip_prefix("**From:**").map(str::trim).filter(|v| !v.is_empty()) {
            sender = Some(v.to_lowercase());
        } else if let Some(v) = line.strip_prefix("**Date:**").map(str::trim) {
            date = v.get(..10).filter(|d| is_iso_date(d)).map(str::to_string);
        }
    }
    (sender, date)
}

fn is_iso_date(d: &str) -> bool {
    let b = d.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// The folder of a document path: the directory of its file (an attachment's is its mail's),
/// lowercased with `/` separators; `None` at the top level.
pub fn folder_of(path: &str) -> Option<String> {
    let file = path.split(ATTACHMENT_SEP).next().unwrap_or(path).replace('\\', "/");
    let dir = file.rsplit_once('/').map(|(d, _)| d.trim())?;
    (!dir.is_empty()).then(|| dir.to_lowercase())
}

/// A path whose last part is a mail file (a mail attached to a mail is a mail, too).
pub fn is_mail_name(path: &str) -> bool {
    let last = path.rsplit(ATTACHMENT_SEP).next().unwrap_or(path).to_lowercase();
    last.ends_with(".eml") || last.ends_with(".msg")
}

/// A date bound as `YYYY-MM-DD`: `YYYY` and `YYYY-MM` widen to the first day (`upper = false`) or
/// the last possible one (`upper = true`; `-31` is fine for comparing).
pub fn date_bound(raw: &str, upper: bool) -> std::result::Result<String, String> {
    let bad = || format!("date \"{raw}\" is not YYYY, YYYY-MM or YYYY-MM-DD");
    let parts: Vec<&str> = raw.trim().split('-').collect();
    let num = |s: &str, len: usize, max: u32| -> Option<u32> {
        (s.len() == len && s.bytes().all(|c| c.is_ascii_digit())).then(|| s.parse().ok()).flatten().filter(|n| (1..=max).contains(n))
    };
    let year = parts.first().and_then(|y| num(y, 4, 9999)).ok_or_else(bad)?;
    let month = match parts.get(1) {
        Some(m) => num(m, 2, 12).ok_or_else(bad)?,
        None => if upper { 12 } else { 1 },
    };
    let day = match parts.get(2) {
        Some(d) => num(d, 2, 31).ok_or_else(bad)?,
        None => if upper { 31 } else { 1 },
    };
    if parts.len() > 3 {
        return Err(bad());
    }
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

impl DocFilter {
    /// Nothing set (blank text counts as unset).
    pub fn is_empty(&self) -> bool {
        [&self.sender, &self.date_from, &self.date_to, &self.folder].iter().all(|f| f.as_deref().is_none_or(|v| v.trim().is_empty()))
    }

    /// The filter as the query compares it: trimmed and lowercased text, dates as full bounds,
    /// blank fields gone. Errors name a malformed date or a start after the end.
    pub fn normalized(&self) -> std::result::Result<DocFilter, String> {
        let text = |v: &Option<String>| v.as_deref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
        let date = |v: &Option<String>, upper| text(v).map(|d| date_bound(&d, upper)).transpose();
        let f = DocFilter {
            sender: text(&self.sender),
            date_from: date(&self.date_from, false)?,
            date_to: date(&self.date_to, true)?,
            folder: text(&self.folder).map(|f| f.replace('\\', "/")),
        };
        if let (Some(a), Some(b)) = (&f.date_from, &f.date_to) && a > b {
            return Err(format!("date_from {a} is after date_to {b}"));
        }
        Ok(f)
    }
}

/// `%needle%` for `LIKE … ESCAPE '\'`, with the needle's own `%`, `_` and `\` taken literally.
pub fn like_pattern(needle: &str) -> String {
    let mut out = String::from("%");
    for c in needle.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// The conditions of a normalized filter on `documents` and their parameters, in order.
pub fn filter_sql(f: &DocFilter) -> (String, Vec<String>) {
    let mut sql = Vec::new();
    let mut args = Vec::new();
    if let Some(d) = &f.date_from {
        sql.push("doc_date >= ?");
        args.push(d.clone());
    }
    if let Some(d) = &f.date_to {
        sql.push("doc_date <= ?");
        args.push(d.clone());
    }
    if let Some(s) = &f.sender {
        sql.push("sender LIKE ? ESCAPE '\\'");
        args.push(like_pattern(s));
    }
    if let Some(p) = &f.folder {
        // Slashes around the stored folder: `/inbox/` matches the part `inbox`, not `inbox_old`.
        sql.push("('/' || folder || '/') LIKE ? ESCAPE '\\'");
        args.push(like_pattern(p));
    }
    (sql.join(" AND "), args)
}

/// Sender and date of each document of one upload, in upload order: a mail's own, an attachment
/// its mail's (the nearest open mail whose path starts its path).
#[derive(Default)]
pub struct MailMetaTracker {
    open: Vec<(String, Option<String>, Option<String>)>,
}

impl MailMetaTracker {
    /// The facts of the document at `path`; `mail_text` is its Markdown when it is a mail.
    pub fn next(&mut self, path: &str, mail_text: Option<&str>) -> DocMeta {
        self.open.retain(|(prefix, ..)| path.starts_with(prefix.as_str()));
        let (sender, date) = match mail_text {
            Some(text) => mail_header(text),
            None => self.open.last().map(|(_, s, d)| (s.clone(), d.clone())).unwrap_or_default(),
        };
        if mail_text.is_some() {
            self.open.push((format!("{path}{ATTACHMENT_SEP}"), sender.clone(), date.clone()));
        }
        DocMeta { sender, date, folder: folder_of(path) }
    }
}

impl Database {
    /// Store the filterable facts of a document (`None` clears).
    pub fn set_document_meta(&self, document_id: i64, m: &DocMeta) -> Result<()> {
        self.conn.execute(
            "UPDATE documents SET sender = ?2, doc_date = ?3, folder = ?4 WHERE id = ?1",
            params![document_id, m.sender, m.date, m.folder],
        )?;
        Ok(())
    }

    /// A document read from a file on disk without a folder of its own (not from inside a zip)
    /// lies in the folder of that file.
    pub(super) fn set_folder_from_source(&self, document_id: i64, path: &str) -> Result<()> {
        if let Some(folder) = folder_of(path) {
            self.conn.execute("UPDATE documents SET folder = ?2 WHERE id = ?1 AND folder IS NULL", params![document_id, folder])?;
        }
        Ok(())
    }

    /// The facts stored for a document (tests, checks).
    pub fn document_meta(&self, document_id: i64) -> Result<Option<DocMeta>> {
        Ok(self
            .conn
            .query_row("SELECT sender, doc_date, folder FROM documents WHERE id = ?1", [document_id], |r| {
                Ok(DocMeta { sender: r.get(0)?, date: r.get(1)?, folder: r.get(2)? })
            })
            .optional()?)
    }

    /// Documents of `context_ids` that `filter` lets through, by id. An empty filter is an error:
    /// the caller means "no restriction" and should not ask.
    pub fn filtered_document_ids(&self, context_ids: &[i64], filter: &DocFilter) -> Result<Vec<i64>> {
        let f = filter.normalized().map_err(CoreError::InvalidState)?;
        if f.is_empty() {
            return Err(CoreError::InvalidState("empty filter".into()));
        }
        let (cond, args) = filter_sql(&f);
        let mut stmt = self.conn.prepare(&format!("SELECT id FROM documents INDEXED BY idx_documents_filter WHERE context_id = ? AND {cond} ORDER BY id"))?;
        let mut out = Vec::new();
        for &cid in context_ids {
            let mut binds: Vec<&dyn rusqlite::ToSql> = vec![&cid];
            binds.extend(args.iter().map(|a| a as &dyn rusqlite::ToSql));
            let rows = stmt.query_map(rusqlite::params_from_iter(binds), |r| r.get::<_, i64>(0))?;
            for id in rows {
                out.push(id?);
            }
        }
        Ok(out)
    }

    /// The search scope with `filter` applied: `doc_ids` unchanged when the filter is empty, else
    /// the filtered documents (within `doc_ids` when a file selection is given).
    pub fn narrow_scope(&self, context_ids: &[i64], doc_ids: Option<Vec<i64>>, filter: &DocFilter) -> Result<Option<Vec<i64>>> {
        if filter.is_empty() {
            return Ok(doc_ids);
        }
        let mut ids = self.filtered_document_ids(context_ids, filter)?;
        if let Some(chosen) = doc_ids {
            let chosen: HashSet<i64> = chosen.into_iter().collect();
            ids.retain(|id| chosen.contains(id));
        }
        Ok(Some(ids))
    }
}

/// Migration v75: sender, date and folder of every existing document, as an upload would have
/// set them. Reads each mail's first [`HEADER_CHARS`] characters; documents in id order per
/// collection, so an attachment follows its mail as at upload.
pub(super) fn backfill(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    struct Row {
        id: i64,
        context_id: i64,
        path: String,
        source: Option<String>,
        mail_text: Option<String>,
    }
    let mut stmt = tx.prepare(&format!(
        "SELECT d.id, d.context_id, COALESCE(d.zip_entry, d.name), s.path, d.chunker,
                substr(d.extracted_text, 1, {HEADER_CHARS})
           FROM documents d LEFT JOIN source_files s ON s.document_id = d.id
          ORDER BY d.context_id, d.id"
    ))?;
    let rows = stmt.query_map([], |r| {
        let path: String = r.get(2)?;
        let chunker: Option<String> = r.get(4)?;
        let text: Option<String> = r.get(5)?;
        let mail = chunker.as_deref() == Some("mail") || is_mail_name(&path);
        Ok(Row { id: r.get(0)?, context_id: r.get(1)?, path, source: r.get(3)?, mail_text: text.filter(|_| mail) })
    })?;
    let mut update = tx.prepare("UPDATE documents SET sender = ?2, doc_date = ?3, folder = ?4 WHERE id = ?1")?;
    let mut tracker = MailMetaTracker::default();
    let mut context = None;
    for row in rows {
        let row = row?;
        if context != Some(row.context_id) {
            tracker = MailMetaTracker::default();
            context = Some(row.context_id);
        }
        let mut m = tracker.next(&row.path, row.mail_text.as_deref());
        if m.folder.is_none() {
            m.folder = row.source.as_deref().and_then(folder_of);
        }
        if m != DocMeta::default() {
            update.execute(params![row.id, m.sender, m.date, m.folder])?;
        }
    }
    Ok(())
}
