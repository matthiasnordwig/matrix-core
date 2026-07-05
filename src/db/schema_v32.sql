-- Persist a context's chosen ontology extraction pool (load-balancer group),
-- analogous to the existing single-endpoint fallback_llm_id/ontology_profile_id
-- columns added in schema_v22.sql. Previously this selection lived only in
-- ephemeral frontend state and was lost on app restart.
ALTER TABLE contexts ADD COLUMN ontology_pool_id INTEGER REFERENCES llm_endpoint_pools(id);
