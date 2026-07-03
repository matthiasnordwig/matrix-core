CREATE TABLE ontology_edge_chunks_new (
    edge_id INTEGER NOT NULL REFERENCES ontology_edges(id) ON DELETE CASCADE,
    chunk_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    PRIMARY KEY (edge_id, chunk_id)
);

INSERT INTO ontology_edge_chunks_new (edge_id, chunk_id)
SELECT chunk.edge_id, chunk.chunk_id
FROM ontology_edge_chunks chunk
JOIN ontology_edges e ON chunk.edge_id = e.id
JOIN chunks c ON chunk.chunk_id = c.id;

DROP TABLE ontology_edge_chunks;
ALTER TABLE ontology_edge_chunks_new RENAME TO ontology_edge_chunks;

CREATE INDEX idx_ontology_edge_chunks_edge_v29 ON ontology_edge_chunks(edge_id);
