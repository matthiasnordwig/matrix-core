-- Lens system: the raw type an extraction produced is now kept permanently
-- (raw_entity_type/raw_relation_type); the active schema becomes a swappable,
-- non-destructively-materialized "Lens" instead of overwriting
-- ontology_nodes.entity_type/ontology_edges.relation_type directly (see
-- BACKLOG.md "Schema-Labeling ohne destruktives Sanitize").
ALTER TABLE ontology_nodes ADD COLUMN raw_entity_type TEXT;
ALTER TABLE ontology_edges ADD COLUMN raw_relation_type TEXT;

-- Backfill: for existing rows, the only known type today already IS the raw
-- type as sanitize_types last left it under the old destructive model — not
-- recoverable, just carried forward so the column is never NULL.
UPDATE ontology_nodes SET raw_entity_type = entity_type WHERE raw_entity_type IS NULL;
UPDATE ontology_edges SET raw_relation_type = relation_type WHERE raw_relation_type IS NULL;

-- A lens materializes one profile's cosine-snap + relation-constraint
-- resolution against a context's raw types, without mutating them. Re-running
-- materialization for the same (context, profile) pair refreshes the existing
-- lens in place (see UNIQUE below) rather than creating a new row each time.
CREATE TABLE ontology_lenses (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    context_id INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    ontology_profile_id INTEGER NOT NULL REFERENCES ontology_profiles(id) ON DELETE CASCADE,
    -- Whether this lens was ever materialized as part of an actual extraction
    -- run (generate_ontology, right after run_extraction) as opposed to a
    -- standalone "Add Lens" re-labeling call against already-stored raw data.
    -- Purely informational (deleting either kind of lens is equally safe —
    -- raw types live on the nodes/edges, not here); used only to show an
    -- extra warning before deleting an extraction lens.
    is_extraction_lens INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(context_id, ontology_profile_id)
);
CREATE INDEX idx_ontology_lenses_context ON ontology_lenses(context_id);

CREATE TABLE ontology_lens_node_types (
    lens_id INTEGER NOT NULL REFERENCES ontology_lenses(id) ON DELETE CASCADE,
    node_id INTEGER NOT NULL REFERENCES ontology_nodes(id) ON DELETE CASCADE,
    resolved_type TEXT NOT NULL,
    PRIMARY KEY (lens_id, node_id)
);

CREATE TABLE ontology_lens_edge_verdicts (
    lens_id INTEGER NOT NULL REFERENCES ontology_lenses(id) ON DELETE CASCADE,
    edge_id INTEGER NOT NULL REFERENCES ontology_edges(id) ON DELETE CASCADE,
    verdict TEXT NOT NULL CHECK(verdict IN ('valid','reversed','deleted')),
    resolved_relation_type TEXT,
    PRIMARY KEY (lens_id, edge_id)
);

-- NULL = show raw/unfiltered (the only possible state for any pre-lens
-- context, and the state a context falls back to if its active lens is
-- deleted).
ALTER TABLE contexts ADD COLUMN active_lens_id INTEGER REFERENCES ontology_lenses(id) ON DELETE SET NULL;
