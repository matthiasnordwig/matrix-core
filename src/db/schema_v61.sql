-- Image search over graphics pages (handbuch/core.md "Bildsuche in der Hybrid-Fusion").
--
-- `page_images`: the rendered page image (JPEG, ~100 dpi) of every PDF page the
-- ingest classified as a graphics page (charts, org charts, diagrams). Stored
-- regardless of the context's model/setting — the original PDF is not kept, so
-- without these bytes embedding graphics later would be impossible.
-- `char_start`/`char_end`: the page's byte range in `documents.extracted_text`
-- (NULL when unknown) — used to map a page hit onto a chunk of that page.
CREATE TABLE page_images (
    id          INTEGER PRIMARY KEY,
    context_id  INTEGER NOT NULL REFERENCES contexts(id)  ON DELETE CASCADE,
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    page        INTEGER NOT NULL,                 -- 1-based
    char_start  INTEGER,
    char_end    INTEGER,
    width       INTEGER NOT NULL,
    height      INTEGER NOT NULL,
    image       BLOB    NOT NULL,                 -- JPEG bytes
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE UNIQUE INDEX idx_page_images_doc_page ON page_images(document_id, page);
CREATE INDEX idx_page_images_context ON page_images(context_id);

-- One image vector per page image, in the embedding space of the context's
-- model (text model + its vision encoder). Never compared with vectors of
-- another model: retrieval filters on `embedding_model_id` and `dim`.
CREATE TABLE page_image_embeddings (
    page_image_id      INTEGER PRIMARY KEY REFERENCES page_images(id) ON DELETE CASCADE,
    context_id         INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    embedding_model_id INTEGER NOT NULL REFERENCES embedding_models(id) ON DELETE CASCADE,
    dim                INTEGER NOT NULL,
    vector             BLOB    NOT NULL,          -- dim * f32, little-endian
    created_at         INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX idx_page_image_embeddings_context ON page_image_embeddings(context_id);

-- Path of the model's vision encoder (`vision_encoder*.onnx`); NULL = text only.
ALTER TABLE embedding_models ADD COLUMN vision_model_path TEXT;

-- Embed a document's graphics pages automatically after its text chunks.
-- Retrieval uses existing image vectors regardless of this flag.
ALTER TABLE contexts ADD COLUMN embed_images INTEGER NOT NULL DEFAULT 0;
