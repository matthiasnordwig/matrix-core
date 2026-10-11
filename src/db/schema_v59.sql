-- Embedding task prefixes (asymmetric retrieval models such as EmbeddingGemma).
--
-- `query_prefix` is prepended to every text embedded as a QUERY (retrieval,
-- eval, tools, attribution), `document_prefix` to every text embedded as a
-- stored DOCUMENT (chunks, ontology nodes/types/communities). Both NULL (or
-- empty) = text is embedded verbatim, i.e. the pre-v59 behaviour.
ALTER TABLE embedding_models ADD COLUMN query_prefix TEXT;
ALTER TABLE embedding_models ADD COLUMN document_prefix TEXT;
