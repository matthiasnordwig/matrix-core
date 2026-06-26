CREATE TABLE ontology_dedup_cache (
    context_id INTEGER NOT NULL,
    id1 INTEGER NOT NULL,
    id2 INTEGER NOT NULL,
    identical BOOLEAN NOT NULL,
    PRIMARY KEY (context_id, id1, id2),
    FOREIGN KEY(context_id) REFERENCES contexts(id) ON DELETE CASCADE
);
