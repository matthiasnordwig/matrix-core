-- schema_v75: what a search can be narrowed to besides collection and file (`doc_meta.rs`).
--   sender   — the mail's From line, lowercased (attachments carry their mail's)
--   doc_date — the mail's own date, YYYY-MM-DD as written in its Date line
--   folder   — directory of the document's path in the collection, lowercased, `/` separators
-- The three columns sit behind `extracted_text` in the row, so reading them from the table walks
-- every text's overflow pages; the covering index holds them, and a filter reads only the index.
-- Existing documents are filled by the Rust hook of this migration (`doc_meta::backfill`).
ALTER TABLE documents ADD COLUMN sender TEXT;
ALTER TABLE documents ADD COLUMN doc_date TEXT;
ALTER TABLE documents ADD COLUMN folder TEXT;
CREATE INDEX idx_documents_filter ON documents(context_id, doc_date, sender, folder);
