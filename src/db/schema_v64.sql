-- schema_v64: description state of PDF graphics pages, and an FTS trigger that
-- only fires when chunk text changes.
--
-- 1. `page_images.description_state` ('described' | 'declined', NULL = not
--    sent yet) and `description_model` (the image model that answered): the
--    describe job's per-page cost guard. It lives on the page, not on chunks,
--    so LLM re-chunking (which replaces a document's chunks) never makes a
--    page look undescribed and re-billed. Re-ingesting a page (new row)
--    resets it.
-- 2. `chunks_fts_au` fired on EVERY update of `chunks` (also `chunk_index`
--    renumbering and metadata marks) and rewrote the FTS row each time; the
--    index only holds `text`, so it now fires on `UPDATE OF text` only.

ALTER TABLE page_images ADD COLUMN description_state TEXT
    CHECK (description_state IS NULL OR description_state IN ('described', 'declined'));
ALTER TABLE page_images ADD COLUMN description_model TEXT;

DROP TRIGGER chunks_fts_au;
CREATE TRIGGER chunks_fts_au AFTER UPDATE OF text ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES('delete', old.id, old.text);
    INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
END;
