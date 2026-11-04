//! Which passages count as repeats of one already held in the collection (`Database::create_chunk_once`).
//!
//! Two kinds do: passages of a mail (`mail::duplicate_key`: a quoted block, the same mail filed
//! twice) and passages of a document that came attached to a mail (`mail.eml › report.doc`), where
//! the same attachment shows up once per mailbox. Documents added on their own never count, however
//! alike their pages are: two PDFs that share a page are two sources.

use super::mail::{duplicate_key, MIN_DUPLICATE_CHARS};
use crate::db::doc_meta::{is_mail_name, ATTACHMENT_SEP};
use crate::db::models::NewChunk;

/// Whether `source` (the document's file name or zip path) is a file attached to a mail: a mail
/// file in front of the separator. A picture inside an Office file is no mail attachment.
pub fn is_mail_attachment(source: &str) -> bool {
    let parts: Vec<&str> = source.split(ATTACHMENT_SEP).collect();
    parts[..parts.len() - 1].iter().any(|p| is_mail_name(p))
}

/// The key under which `chunk` of the document `source` counts as a repeat; `None` = never.
/// An attachment's passage is keyed by its text with white space folded; one too short to say
/// anything alone has no key.
pub fn repeat_key(chunk: &NewChunk, source: &str) -> Option<String> {
    duplicate_key(chunk).or_else(|| {
        if !is_mail_attachment(source) {
            return None;
        }
        let folded = chunk.text.split_whitespace().collect::<Vec<_>>().join(" ");
        (folded.chars().count() >= MIN_DUPLICATE_CHARS).then(|| crate::db::ontology::evidence_carry::text_fingerprint(&folded))
    })
}
