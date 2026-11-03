-- schema_v77: one vector per paragraph of a mail passage (`paragraph_vectors.rs`), written by the
-- embedding job next to the passage's own vector in `embeddings`. Read only to choose what the reranker
-- reads of a long mail passage; the vector search does not use them. `start_byte`/`end_byte` = the
-- paragraph in the chunk text.
CREATE TABLE embedding_paragraphs (
    chunk_id           INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    part               INTEGER NOT NULL,
    context_id         INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    document_id        INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    embedding_model_id INTEGER NOT NULL REFERENCES embedding_models(id) ON DELETE CASCADE,
    start_byte         INTEGER NOT NULL,
    end_byte           INTEGER NOT NULL,
    vector             BLOB    NOT NULL,
    PRIMARY KEY (chunk_id, part)
);
CREATE INDEX idx_embedding_paragraphs_context ON embedding_paragraphs(context_id);
