-- Add community_id to ontology_nodes (NULL = not yet assigned)
ALTER TABLE ontology_nodes ADD COLUMN community_id INTEGER;

-- Community summaries (one per detected community per context)
CREATE TABLE IF NOT EXISTS ontology_communities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    context_id INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    community_label TEXT NOT NULL,
    node_count INTEGER NOT NULL DEFAULT 0,
    summary_text TEXT NOT NULL,
    vector_blob BLOB,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_ontology_communities_context ON ontology_communities(context_id);
