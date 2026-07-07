-- Persistent ontology run log: per-phase success/failure counters that survive
-- app restart (unlike the in-memory applog ring buffer, CAPACITY=500). Initially
-- ONLY the community-summarization phase writes rows (the failure case that
-- motivated this — see BACKLOG.md). Data source for the Ontology admin window's
-- optional status lines. Additive; context_id FK cascades on context delete.
-- run_id groups phases of one pipeline invocation (only one phase writes for
-- now; kept for forward generalization to the other phases).
CREATE TABLE ontology_run_log (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id       TEXT NOT NULL,
    context_id   INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    phase        TEXT NOT NULL,
    started_at   INTEGER NOT NULL,
    finished_at  INTEGER,
    attempted    INTEGER NOT NULL DEFAULT 0,
    succeeded    INTEGER NOT NULL DEFAULT 0,
    failed       INTEGER NOT NULL DEFAULT 0,
    sample_error TEXT
);
CREATE INDEX idx_ontology_run_log_context ON ontology_run_log(context_id, started_at DESC);
