CREATE TABLE IF NOT EXISTS ontology_extracted_chunks (
    context_id INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    chunk_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    PRIMARY KEY (context_id, chunk_id)
);

INSERT OR IGNORE INTO ontology_extracted_chunks (context_id, chunk_id)
SELECT DISTINCT context_id, chunk_id FROM ontology_edges;
