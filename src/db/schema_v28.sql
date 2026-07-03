CREATE TABLE ontology_edges_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    context_id INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    source_id INTEGER NOT NULL REFERENCES ontology_nodes(id) ON DELETE CASCADE,
    target_id INTEGER NOT NULL REFERENCES ontology_nodes(id) ON DELETE CASCADE,
    relation_type TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    UNIQUE(context_id, source_id, target_id, relation_type COLLATE NOCASE)
);

INSERT INTO ontology_edges_new (id, context_id, source_id, target_id, relation_type, created_at)
SELECT min(id), context_id, source_id, target_id, relation_type, min(created_at)
FROM ontology_edges
GROUP BY context_id, source_id, target_id, relation_type COLLATE NOCASE;

CREATE TABLE ontology_edge_chunks (
    edge_id INTEGER NOT NULL REFERENCES ontology_edges_new(id) ON DELETE CASCADE,
    chunk_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    PRIMARY KEY (edge_id, chunk_id)
);

INSERT OR IGNORE INTO ontology_edge_chunks (edge_id, chunk_id)
SELECT ne.id, oe.chunk_id
FROM ontology_edges oe
JOIN ontology_edges_new ne ON 
    oe.context_id = ne.context_id AND 
    oe.source_id = ne.source_id AND 
    oe.target_id = ne.target_id AND 
    LOWER(oe.relation_type) = LOWER(ne.relation_type);

DROP TABLE ontology_edges;
ALTER TABLE ontology_edges_new RENAME TO ontology_edges;

CREATE INDEX idx_ontology_edges_context ON ontology_edges(context_id);
CREATE INDEX idx_ontology_edges_source ON ontology_edges(source_id);
CREATE INDEX idx_ontology_edges_target ON ontology_edges(target_id);
CREATE INDEX idx_ontology_edge_chunks_edge ON ontology_edge_chunks(edge_id);
