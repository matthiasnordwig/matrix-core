CREATE TABLE IF NOT EXISTS ontology_chunk_states (
    context_id INTEGER NOT NULL,
    chunk_id INTEGER NOT NULL,
    completed_batches_json TEXT NOT NULL,
    partial_graph_json TEXT NOT NULL,
    PRIMARY KEY (context_id, chunk_id),
    FOREIGN KEY (context_id) REFERENCES contexts(id) ON DELETE CASCADE,
    FOREIGN KEY (chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
);
