-- schema_v47: LLM-free retrieval eval (RETRIEVAL_QUALITY_PLAN.md AP0).
-- Golden question-sets + entries, plus per-run + per-entry result rows.
-- Additive only; never touches `chunks`.

CREATE TABLE eval_golden_sets (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE eval_golden_entries (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    set_id      INTEGER NOT NULL REFERENCES eval_golden_sets(id) ON DELETE CASCADE,
    entry_key   TEXT NOT NULL,            -- stable per-set id (the JSON "id" field)
    question    TEXT NOT NULL,
    anchors_any TEXT NOT NULL DEFAULT '[]', -- JSON array of substrings
    note        TEXT NOT NULL DEFAULT ''
);

CREATE INDEX idx_eval_golden_entries_set ON eval_golden_entries(set_id);

CREATE TABLE eval_runs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    set_id      INTEGER NOT NULL REFERENCES eval_golden_sets(id) ON DELETE CASCADE,
    context_ids TEXT NOT NULL DEFAULT '[]', -- JSON array of context ids
    config      TEXT NOT NULL DEFAULT '{}', -- JSON: {k, hybrid, follow_refs, rerank, top_k}
    status      TEXT NOT NULL DEFAULT 'running', -- running | done | error
    started_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    finished_at INTEGER,
    metrics     TEXT NOT NULL DEFAULT '{}'  -- JSON: aggregate Hit@5/Hit@10/MRR + counts
);

CREATE INDEX idx_eval_runs_set ON eval_runs(set_id);

CREATE TABLE eval_run_results (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id          INTEGER NOT NULL REFERENCES eval_runs(id) ON DELETE CASCADE,
    entry_id        INTEGER NOT NULL, -- the eval_golden_entries.id evaluated
    entry_key       TEXT NOT NULL DEFAULT '',
    question        TEXT NOT NULL DEFAULT '',
    resolved_chunks INTEGER NOT NULL DEFAULT 0,
    first_rank      INTEGER,          -- nullable: 1-based rank of first relevant hit, NULL if none
    hit5            INTEGER NOT NULL DEFAULT 0, -- bool
    hit10           INTEGER NOT NULL DEFAULT 0, -- bool
    skipped         INTEGER NOT NULL DEFAULT 0  -- bool
);

CREATE INDEX idx_eval_run_results_run ON eval_run_results(run_id);
