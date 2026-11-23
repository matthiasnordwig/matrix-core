-- schema_v86: where a document was made and what its pictures show (scene labels).
--
--   * documents.lat / lon — position of the recording or photo (WGS 84 degrees) from the file's own metadata
--     (QuickTime/MP4 location, EXIF GPS); NULL = unknown. documents.place_id — GeoNames id of the nearest
--     place in the offline place index (`cities1000`), NULL until resolved. documents.device — the camera
--     or phone that made it ("Apple iPhone 15 Pro"). Written through `doc_place.rs`.
--   * scene_labels — fixed labels of a picture (a video's scene still, a photo): `indoors`, `outdoors`,
--     `water`, … (`media/scene_labels.rs`), one row per label and source (`vectors` = from the image vector
--     of the collection's model, `vision` = Apple's image classification). document_id repeats the page
--     image's, so the search filter reads one index.
--   * scene_label_state — which pictures have been labelled by which source (a picture may get no label
--     from a source, so the labels alone cannot tell): the embedding model whose vector was used, and
--     whether Vision ran.
--
-- Plain ALTER TABLE / CREATE TABLE, runs through db/mod.rs::migrate's default arm.

ALTER TABLE documents ADD COLUMN lat      REAL NULL;
ALTER TABLE documents ADD COLUMN lon      REAL NULL;
ALTER TABLE documents ADD COLUMN place_id INTEGER NULL;
ALTER TABLE documents ADD COLUMN device   TEXT NULL;

-- Search filter "at this place" and the documents whose place is still to be resolved.
CREATE INDEX idx_documents_place ON documents(context_id, place_id) WHERE place_id IS NOT NULL;
CREATE INDEX idx_documents_unplaced ON documents(id) WHERE lat IS NOT NULL AND place_id IS NULL;

CREATE TABLE scene_labels (
    page_image_id INTEGER NOT NULL REFERENCES page_images(id) ON DELETE CASCADE,
    document_id   INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    source        TEXT NOT NULL,            -- vectors | vision
    label         TEXT NOT NULL,
    PRIMARY KEY (page_image_id, source, label)
);
CREATE INDEX idx_scene_labels_doc ON scene_labels(document_id, label);

CREATE TABLE scene_label_state (
    page_image_id      INTEGER PRIMARY KEY REFERENCES page_images(id) ON DELETE CASCADE,
    vectors_model_id   INTEGER NULL,        -- embedding model whose image vector gave the `vectors` labels
    vision             INTEGER NOT NULL DEFAULT 0
);
