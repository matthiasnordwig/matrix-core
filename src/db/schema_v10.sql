ALTER TABLE contexts ADD COLUMN chunk_endpoint_id INTEGER REFERENCES llm_endpoints(id) ON DELETE SET NULL;
