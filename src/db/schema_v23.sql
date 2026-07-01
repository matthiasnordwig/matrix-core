CREATE TABLE IF NOT EXISTS ontology_quarantine (
    chunk_id INTEGER PRIMARY KEY,
    context_id INTEGER NOT NULL,
    graph_json TEXT NOT NULL,
    error_reason TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (chunk_id) REFERENCES chunks(id) ON DELETE CASCADE,
    FOREIGN KEY (context_id) REFERENCES contexts(id) ON DELETE CASCADE
);
