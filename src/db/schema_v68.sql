-- Graph evidence of a deleted document, kept until the same document text is
-- added to the context again, at most 30 days (core
-- db/ontology/evidence_parking.rs). No document or chunk text is kept: only the
-- fingerprint of the text (FNV-1a + length), the byte ranges of the old chunks
-- that were extracted or carried evidence, and the evidence itself (edge and
-- review references, the quote already stored with the evidence row).
-- Edges, reviews and the context take their parked rows with them when deleted.
CREATE TABLE ontology_parked_documents (
    id               INTEGER PRIMARY KEY,
    context_id       INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    text_fingerprint TEXT    NOT NULL,
    parked_at        INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX idx_parked_documents_ctx ON ontology_parked_documents(context_id, text_fingerprint);

CREATE TABLE ontology_parked_chunks (
    id                 INTEGER PRIMARY KEY,
    parked_document_id INTEGER NOT NULL REFERENCES ontology_parked_documents(id) ON DELETE CASCADE,
    char_start         INTEGER NOT NULL,
    char_end           INTEGER NOT NULL,
    extracted          INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_parked_chunks_doc ON ontology_parked_chunks(parked_document_id);

CREATE TABLE ontology_parked_evidence (
    parked_chunk_id INTEGER NOT NULL REFERENCES ontology_parked_chunks(id) ON DELETE CASCADE,
    edge_id         INTEGER NOT NULL REFERENCES ontology_edges(id) ON DELETE CASCADE,
    review_id       INTEGER REFERENCES ontology_edge_reviews(id) ON DELETE CASCADE,
    evidence        TEXT
);
CREATE INDEX idx_parked_evidence_chunk ON ontology_parked_evidence(parked_chunk_id);
CREATE INDEX idx_parked_evidence_edge ON ontology_parked_evidence(edge_id);
CREATE INDEX idx_parked_evidence_review ON ontology_parked_evidence(review_id);
