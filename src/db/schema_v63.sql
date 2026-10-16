-- Spreadsheet regions as real SQL tables (tool `query_table`).
--
-- Every table region of an ingested spreadsheet (xlsx/xls/ods/csv/tsv) is
-- also stored as a physical table `sheet_<document_id>_<n>` in this database
-- (created by `sheet_tables.rs::create_sheet_table`, not by a migration).
-- This registry names them: which document/context they belong to, the sheet
-- label and region number (same values as the chunk metadata `sheet`/`region`
-- of the sheet's chunks, region 0 = the sheet has one region), the column
-- list as JSON (original header, SQL name, type, notes) and the row count.
--
-- Deleting the document or context removes the registry row by CASCADE; the
-- physical table is dropped by `delete_document`/`delete_context` (a trigger
-- cannot run DROP TABLE) and, as a safety net, by `drop_orphan_sheet_tables`
-- at every open.

CREATE TABLE sheet_tables (
    id           INTEGER PRIMARY KEY,
    context_id   INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    document_id  INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    sheet_name   TEXT NOT NULL,
    region_index INTEGER NOT NULL DEFAULT 0,
    table_name   TEXT NOT NULL UNIQUE,
    columns_json TEXT NOT NULL,
    row_count    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_sheet_tables_context ON sheet_tables(context_id);
CREATE INDEX idx_sheet_tables_document ON sheet_tables(document_id);
