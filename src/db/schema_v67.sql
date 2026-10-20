-- schema_v67: provenance of graph vectors.
--
-- `ontology_nodes.vector_blob` and `ontology_communities.vector_blob` did not
-- record which embedding model wrote them. After a collection's search model
-- changed, chunk vectors were dropped but graph vectors stayed in the old space,
-- and graph search compared new-model queries against them (same dimension, so
-- nothing caught it). From here on every graph vector carries the model that
-- wrote it; readers only use vectors whose `vector_model_id` equals the
-- collection's current `contexts.embedding_model_id`.
--
-- FK note: a future rebuild of `embedding_models` (like v51) must keep these two
-- child columns in mind (ON DELETE SET NULL).
--
-- Backfill, conservative (a wrong guess means noise, a dropped vector only means
-- re-embedding, which needs no LLM call):
--   * Node vectors: a model change deletes all chunk vectors of the collection,
--     so the oldest chunk vector of the current model is younger than the last
--     model change. A node created at or after that moment was embedded with the
--     current model (every writer embeds with the collection's model) -> stamp it.
--     All other node vectors (older nodes, collections without a model or
--     without chunk vectors) are unprovable and are dropped.
--   * Community vectors: the summary cache copies a vector into a freshly created
--     row, so `created_at` proves nothing -> all are dropped. Summaries stay.
-- Dropped vectors are re-embedded by the next embedding run of the collection or
-- by "Re-embed" in the Ontology window (no LLM involved).

ALTER TABLE ontology_nodes ADD COLUMN vector_model_id INTEGER REFERENCES embedding_models(id) ON DELETE SET NULL;
ALTER TABLE ontology_communities ADD COLUMN vector_model_id INTEGER REFERENCES embedding_models(id) ON DELETE SET NULL;

UPDATE ontology_nodes
   SET vector_model_id = (SELECT c.embedding_model_id FROM contexts c WHERE c.id = ontology_nodes.context_id)
 WHERE vector_blob IS NOT NULL
   AND created_at >= (
         SELECT MIN(e.created_at) FROM embeddings e
           JOIN contexts c ON c.id = e.context_id
          WHERE e.context_id = ontology_nodes.context_id
            AND e.embedding_model_id = c.embedding_model_id);

UPDATE ontology_nodes SET vector_blob = NULL WHERE vector_model_id IS NULL;
UPDATE ontology_communities SET vector_blob = NULL, vector_model_id = NULL;
