-- Non-blocking review list for edges whose evidence quote hinted at a
-- possible negation/polarity error (see ontology/extract/negation.rs) but
-- the stage-2 LLM verification (ontology/extract/verify.rs) came back
-- "unclear" or failed outright. Deliberately separate from
-- ontology_quarantine, which blocks the whole pipeline until resolved —
-- inappropriate for a mere suspicion that may well be a false positive.
CREATE TABLE ontology_edge_reviews (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    context_id INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    edge_id INTEGER NOT NULL REFERENCES ontology_edges(id) ON DELETE CASCADE,
    chunk_id INTEGER REFERENCES chunks(id) ON DELETE SET NULL,
    relation_type TEXT NOT NULL,
    evidence TEXT,
    reason TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX idx_ontology_edge_reviews_context ON ontology_edge_reviews(context_id);
