-- Phase-separated model choice for the ontology pipeline (see BACKLOG.md
-- "Phasen-getrennte Modellwahl in der Ontologie-Pipeline"): extraction and
-- polarity verification are the judgment-critical phases (negation missed,
-- wrong entity types, tautological edges), while dedup/community-summary are
-- more error-tolerant and don't need to run on the same (possibly more
-- expensive) model. Both NULL = today's behavior (one source for
-- everything, driven by contexts.llm_id/ontology_pool_id).
ALTER TABLE contexts ADD COLUMN ontology_extract_llm_id INTEGER REFERENCES llm_endpoints(id) ON DELETE SET NULL;
ALTER TABLE contexts ADD COLUMN ontology_extract_pool_id INTEGER REFERENCES llm_endpoint_pools(id) ON DELETE SET NULL;
