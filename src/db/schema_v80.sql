-- schema_v80: the search memory of Tables (services `commands/grid_search.rs`) and the state of a
-- collection's searchable content it depends on.
--
-- contexts.search_version — raised by every write that can change what a search in the collection
--   finds: passages (insert, delete, text, left out, duplicate marker, moved), passage vectors, page
--   image vectors, the paragraph vectors the reranker reads, a source file going missing or coming
--   back, the collection's embedding model, an edit of that model. Done by the triggers below, so no
--   write path (this program, the stand-alone MCP program on the same file, an older build) can miss
--   it; cascades (a deleted document or collection) fire them too.
-- grid_search_cache — one row search: key = hash of the search text, the sources (collections in
--   order, their search_version and model, the file choice) and every search setting (services
--   `grid_search_key.rs`); hits = the ordered hits as JSON (`ScoredChunk`: chunk id, score, page
--   image, provenance — no texts). Not bound to a run; runs keep their own `retrieved_refs`.
--   last_used orders eviction (least recently used first, cap in services).
-- grid_search_cache_contexts — which collections an entry searched. A collection whose
--   search_version rises or that is deleted takes its entries with it at once (triggers); the key
--   alone would already never match again.
ALTER TABLE contexts ADD COLUMN search_version INTEGER NOT NULL DEFAULT 0;

CREATE TABLE grid_search_cache (
    key       TEXT    PRIMARY KEY,
    hits      TEXT    NOT NULL,
    last_used INTEGER NOT NULL
);
CREATE INDEX idx_grid_search_cache_last_used ON grid_search_cache(last_used);

CREATE TABLE grid_search_cache_contexts (
    context_id INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    key        TEXT    NOT NULL REFERENCES grid_search_cache(key) ON DELETE CASCADE,
    PRIMARY KEY (context_id, key)
);
CREATE INDEX idx_grid_search_cache_contexts_key ON grid_search_cache_contexts(key);

-- An entry goes with any of its collections.
CREATE TRIGGER grid_search_cache_contexts_ad AFTER DELETE ON grid_search_cache_contexts BEGIN
    DELETE FROM grid_search_cache WHERE key = OLD.key;
END;
CREATE TRIGGER contexts_search_version_au AFTER UPDATE OF search_version ON contexts
WHEN NEW.search_version IS NOT OLD.search_version BEGIN
    DELETE FROM grid_search_cache_contexts WHERE context_id = NEW.id;
END;

-- Passages.
CREATE TRIGGER chunks_search_version_ai AFTER INSERT ON chunks BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id = NEW.context_id;
END;
CREATE TRIGGER chunks_search_version_ad AFTER DELETE ON chunks BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id = OLD.context_id;
END;
CREATE TRIGGER chunks_search_version_au AFTER UPDATE OF text, is_omitted, duplicate_of, context_id, document_id ON chunks
WHEN NEW.text IS NOT OLD.text OR NEW.is_omitted IS NOT OLD.is_omitted OR NEW.duplicate_of IS NOT OLD.duplicate_of
  OR NEW.context_id IS NOT OLD.context_id OR NEW.document_id IS NOT OLD.document_id BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id IN (OLD.context_id, NEW.context_id);
END;

-- Passage vectors.
CREATE TRIGGER embeddings_search_version_ai AFTER INSERT ON embeddings BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id = NEW.context_id;
END;
CREATE TRIGGER embeddings_search_version_ad AFTER DELETE ON embeddings BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id = OLD.context_id;
END;
CREATE TRIGGER embeddings_search_version_au AFTER UPDATE ON embeddings BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id IN (OLD.context_id, NEW.context_id);
END;

-- Page image vectors (the image list of the hybrid search).
CREATE TRIGGER page_image_embeddings_search_version_ai AFTER INSERT ON page_image_embeddings BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id = NEW.context_id;
END;
CREATE TRIGGER page_image_embeddings_search_version_ad AFTER DELETE ON page_image_embeddings BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id = OLD.context_id;
END;
CREATE TRIGGER page_image_embeddings_search_version_au AFTER UPDATE ON page_image_embeddings BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id IN (OLD.context_id, NEW.context_id);
END;

-- Paragraph vectors of mail passages (what the reranker reads of them).
CREATE TRIGGER embedding_paragraphs_search_version_ai AFTER INSERT ON embedding_paragraphs BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id = NEW.context_id;
END;
CREATE TRIGGER embedding_paragraphs_search_version_ad AFTER DELETE ON embedding_paragraphs BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id = OLD.context_id;
END;
CREATE TRIGGER embedding_paragraphs_search_version_au AFTER UPDATE ON embedding_paragraphs BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id IN (OLD.context_id, NEW.context_id);
END;

-- Source files: a missing file hides its document from every search (`FILE_PRESENT_SQL`).
CREATE TRIGGER source_files_search_version_ai AFTER INSERT ON source_files
WHEN NEW.missing_since IS NOT NULL BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id = NEW.context_id;
END;
CREATE TRIGGER source_files_search_version_ad AFTER DELETE ON source_files
WHEN OLD.missing_since IS NOT NULL BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id = OLD.context_id;
END;
CREATE TRIGGER source_files_search_version_au AFTER UPDATE OF missing_since, pending, context_id ON source_files
WHEN NEW.missing_since IS NOT OLD.missing_since OR NEW.pending IS NOT OLD.pending OR NEW.context_id IS NOT OLD.context_id BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id IN (OLD.context_id, NEW.context_id);
END;

-- The collection's embedding model, and an edit of a model (path, prefix, dimension …).
CREATE TRIGGER contexts_model_search_version_au AFTER UPDATE OF embedding_model_id ON contexts
WHEN NEW.embedding_model_id IS NOT OLD.embedding_model_id BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE id = NEW.id;
END;
CREATE TRIGGER embedding_models_search_version_au AFTER UPDATE ON embedding_models BEGIN
    UPDATE contexts SET search_version = search_version + 1 WHERE embedding_model_id = NEW.id;
END;

-- A reranker edited or removed: its id is in the key, its settings are not — forget everything.
CREATE TRIGGER reranker_models_search_cache_au AFTER UPDATE ON reranker_models BEGIN
    DELETE FROM grid_search_cache;
END;
CREATE TRIGGER reranker_models_search_cache_ad AFTER DELETE ON reranker_models BEGIN
    DELETE FROM grid_search_cache;
END;
