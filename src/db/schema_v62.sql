-- Image understanding (FORMATE_PLAN AP6).
--
--   * llm_endpoints.supports_vision — endpoint capability flag (analog to
--     supports_tools, schema_v54): the model behind this endpoint accepts
--     images in a chat request. Set by hand in the endpoint form; default 0.
--   * contexts.image_llm_id — the endpoint that writes generated image
--     descriptions for this context's pictures (NULL = no descriptions).
--     Same pattern as llm_id / ontology_extract_llm_id (schema_v36): a nullable
--     FK added via ALTER TABLE, enforced with ON DELETE SET NULL, so deleting
--     the endpoint turns descriptions off instead of leaving a dangling id.
--
-- Plain ALTER TABLE ADD COLUMN, runs through db/mod.rs::migrate's default arm.

ALTER TABLE llm_endpoints ADD COLUMN supports_vision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE contexts ADD COLUMN image_llm_id INTEGER NULL REFERENCES llm_endpoints(id) ON DELETE SET NULL;
