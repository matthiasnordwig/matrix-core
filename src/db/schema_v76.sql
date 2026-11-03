-- schema_v76: where each answer cell of a grid row came from (`grid_cell_sources`, services crate).
--   cell_sources — JSON, one object per answer fragment of the row, keyed by answer column:
--                  {"from":"own"|"other"|"missing", "source": n, "chunk_id": id}.
--                  Computed from the finished answer and the row's source texts, without a model.
--                  NULL = not computed (rows from before this version, cancelled or failed rows).
ALTER TABLE grid_chat_results ADD COLUMN cell_sources TEXT;
