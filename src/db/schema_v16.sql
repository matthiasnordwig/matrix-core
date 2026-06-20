CREATE TABLE IF NOT EXISTS ontology_profiles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    entity_types_json TEXT NOT NULL,
    relation_types_json TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE TABLE IF NOT EXISTS ontology_nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    context_id INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    description TEXT NOT NULL,
    vector_blob BLOB,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_ontology_nodes_context ON ontology_nodes(context_id);
CREATE INDEX IF NOT EXISTS idx_ontology_nodes_type ON ontology_nodes(entity_type);

CREATE TABLE IF NOT EXISTS ontology_edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    context_id INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    source_id INTEGER NOT NULL REFERENCES ontology_nodes(id) ON DELETE CASCADE,
    target_id INTEGER NOT NULL REFERENCES ontology_nodes(id) ON DELETE CASCADE,
    relation_type TEXT NOT NULL,
    chunk_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_ontology_edges_context ON ontology_edges(context_id);
CREATE INDEX IF NOT EXISTS idx_ontology_edges_source ON ontology_edges(source_id);
CREATE INDEX IF NOT EXISTS idx_ontology_edges_target ON ontology_edges(target_id);
