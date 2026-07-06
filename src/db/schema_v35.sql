-- Persistent raw-type -> vector cache (see BACKLOG.md "Rohtyp-Embeddings in
-- materialize_lens/schema_suggest batchen + cachen"): materialize_lens
-- embedded every not-yet-allowed raw type SERIALLY, with no cache across
-- runs (~150 sequential round-trips observed on a real context, redone on
-- every re-materialization). schema_suggest.rs embedded a similar/overlapping
-- set again separately. This table lets both share one persistent cache,
-- keyed per embedding model since vectors are model-specific (not
-- comparable across models/dims). Catalog names from schema_suggest.rs's
-- GENERIC_ENTITY_CATALOG live here too, as ordinary raw_type rows.
CREATE TABLE ontology_type_vector_cache (
    embedding_model_id INTEGER NOT NULL REFERENCES embedding_models(id) ON DELETE CASCADE,
    raw_type TEXT NOT NULL,
    vector BLOB NOT NULL,
    PRIMARY KEY (embedding_model_id, raw_type)
);
