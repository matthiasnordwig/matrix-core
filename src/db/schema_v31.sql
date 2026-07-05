-- Endpoint pools: named groups of llm_endpoints for load-balanced dispatch.
-- Invariant "at most one gguf (local/on-device) member per pool" is enforced
-- in application code (Database::set_pool_members), not here — SQLite CHECK
-- constraints can't see across rows.

CREATE TABLE llm_endpoint_pools (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE llm_endpoint_pool_members (
    pool_id     INTEGER NOT NULL REFERENCES llm_endpoint_pools(id) ON DELETE CASCADE,
    endpoint_id INTEGER NOT NULL REFERENCES llm_endpoints(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    PRIMARY KEY (pool_id, endpoint_id)
);
