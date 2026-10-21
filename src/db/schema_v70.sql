-- Drop the dead `grid_sheets` / `grid_rows` tables (created in schema_v1, never
-- written by any command; upload rows live in `grid_chat_results` with
-- `row_ref_type = 'grid_row'`).
DROP INDEX IF EXISTS idx_grid_rows_sheet;
DROP TABLE IF EXISTS grid_rows;
DROP TABLE IF EXISTS grid_sheets;
