-- schema_v74: the chunker a document was cut with, and passages that repeat one already held.
-- A repeat keeps its row (the document stays whole) but is omitted from search and has no vector;
-- `duplicate_of` names the passage that stands for it, `text_key` finds that passage at ingest.
ALTER TABLE documents ADD COLUMN chunker TEXT;
ALTER TABLE chunks ADD COLUMN text_key TEXT;
ALTER TABLE chunks ADD COLUMN duplicate_of INTEGER REFERENCES chunks(id) ON DELETE SET NULL;
CREATE INDEX idx_chunks_text_key ON chunks(context_id, text_key) WHERE text_key IS NOT NULL;
CREATE INDEX idx_chunks_duplicate_of ON chunks(duplicate_of) WHERE duplicate_of IS NOT NULL;

-- Search vectors of the parts of a long passage. `embeddings` keeps one vector per passage (their
-- mean), so everything that reads it stays as it is; search compares these as well.
CREATE TABLE embedding_parts (
    chunk_id    INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    part        INTEGER NOT NULL,
    context_id  INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    vector      BLOB    NOT NULL,
    UNIQUE (chunk_id, part)
);
CREATE INDEX idx_embedding_parts_context ON embedding_parts(context_id);
