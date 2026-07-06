-- Grid chat results: identify rows by a stable `row_uid` instead of
-- `(run_id, row_ref_type, row_ref_id)`. After a JSON explode (one LLM array
-- answer fans out into several rows) or when several runs share a chunk id,
-- multiple rows carry the same `row_ref_id`; the old unique constraint made
-- their upserts clobber each other. The unique constraint lives in the
-- CREATE TABLE DDL, so a table rebuild is required (SQLite cannot drop a
-- table constraint in place). Backfill existing rows with the value the old
-- key would have produced.
CREATE TABLE grid_chat_results_new (
    id              INTEGER PRIMARY KEY,
    run_id          TEXT    NOT NULL,
    row_uid         TEXT    NOT NULL DEFAULT '',
    row_ref_type    TEXT    NOT NULL CHECK (row_ref_type IN ('chunk', 'grid_row')),
    row_ref_id      INTEGER NOT NULL,
    prompt          TEXT    NOT NULL,
    columns_context TEXT,
    retrieved_refs  TEXT,
    response        TEXT,
    status          TEXT    NOT NULL DEFAULT 'queued'
                            CHECK (status IN ('queued', 'retrieving', 'llm', 'done', 'error')),
    error           TEXT,
    prompt_snapshot TEXT,
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (run_id, row_uid)
);

INSERT INTO grid_chat_results_new
    (id, run_id, row_uid, row_ref_type, row_ref_id, prompt, columns_context,
     retrieved_refs, response, status, error, prompt_snapshot, updated_at)
SELECT id, run_id, row_ref_type || ':' || row_ref_id, row_ref_type, row_ref_id,
       prompt, columns_context, retrieved_refs, response, status, error,
       prompt_snapshot, updated_at
FROM grid_chat_results;

DROP TABLE grid_chat_results;
ALTER TABLE grid_chat_results_new RENAME TO grid_chat_results;
