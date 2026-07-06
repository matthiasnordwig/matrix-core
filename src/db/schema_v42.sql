-- Excel/CSV upload rows in the Grid tab are never backed by a `chunks` row
-- (client-side import, no context/chunking run) — `row_ref_type = 'grid_row'`
-- carries a synthetic `row_ref_id` instead. History loading needs the row's
-- source text to rebuild a synthetic `Chunk`, so store it alongside the
-- result. NULL for ordinary chunk-backed rows.
ALTER TABLE grid_chat_results ADD COLUMN row_source_text TEXT;
