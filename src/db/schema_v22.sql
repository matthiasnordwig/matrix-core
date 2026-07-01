ALTER TABLE contexts RENAME COLUMN chunk_endpoint_id TO llm_id;
ALTER TABLE contexts ADD COLUMN fallback_llm_id INTEGER REFERENCES llm_endpoints(id);
ALTER TABLE contexts ADD COLUMN ontology_profile_id INTEGER REFERENCES ontology_profiles(id);
