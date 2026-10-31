-- schema_v73: chunks of one document. The UNIQUE constraint starts with
-- context_id, so `WHERE document_id = ?` read the whole table: a search paid
-- that once per hit, and the time grew with the number of passages.
-- IF NOT EXISTS: a second process that read version 72 before the first one
-- committed would otherwise stop with "index already exists".
CREATE INDEX IF NOT EXISTS idx_chunks_document ON chunks(document_id, chunk_index);
