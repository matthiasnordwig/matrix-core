-- Matrix Rust Core — schema v1
-- Applied in a single transaction when PRAGMA user_version = 0.
-- Constraints: 100% offline, statically linkable for aarch64-apple-ios.
-- Vectors are raw f32 BLOBs (no sqlite-vec / HNSW C-extensions).

-- ---------------------------------------------------------------------------
-- A. Registries (administered from the Settings tab)
-- ---------------------------------------------------------------------------

CREATE TABLE embedding_models (
    id                 INTEGER PRIMARY KEY,
    identifier         TEXT    NOT NULL UNIQUE,          -- e.g. "nomic-embed-text-v1.5"
    kind               TEXT    NOT NULL CHECK (kind IN ('local_onnx', 'remote_api')),
    model_path         TEXT,                             -- local ONNX path (local_onnx)
    tokenizer_path     TEXT,                             -- matching tokenizer.json (local_onnx)
    api_config         TEXT,                             -- JSON: {base_url, model, key_ref} (remote_api)
    execution_provider TEXT    CHECK (execution_provider IN ('ane', 'coreml', 'cpu')),
    is_matryoshka      INTEGER NOT NULL DEFAULT 0,       -- 1 ⇒ truncation to default_dim is valid
    native_dim         INTEGER NOT NULL,                 -- full model dimension (e.g. 384, 768)
    default_dim        INTEGER NOT NULL,                 -- effective dim (128 only if is_matryoshka)
    normalize          INTEGER NOT NULL DEFAULT 1,       -- L2-normalize on store/query
    created_at         INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE llm_endpoints (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL UNIQUE,
    base_url    TEXT    NOT NULL,
    model_id    TEXT    NOT NULL,
    api_key_ref TEXT,                                    -- reference/handle, not the raw secret
    timeout_ms  INTEGER NOT NULL DEFAULT 60000,
    max_retries INTEGER NOT NULL DEFAULT 2,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

-- ---------------------------------------------------------------------------
-- B. Profiles & Contexts
-- ---------------------------------------------------------------------------

CREATE TABLE chunking_profiles (
    id                INTEGER PRIMARY KEY,
    name              TEXT    NOT NULL UNIQUE,
    prompt            TEXT    NOT NULL,                  -- Chunking Profile Prompt (uses {{pre_chunk}})
    window_sentences  INTEGER NOT NULL,                 -- pre-chunk window, in sentences
    overlap_ratio     REAL    NOT NULL DEFAULT 0.2,      -- sliding-window overlap (0..1)
    max_signature_len INTEGER NOT NULL DEFAULT 80,
    llm_endpoint_id   INTEGER REFERENCES llm_endpoints(id) ON DELETE SET NULL,
    metadata_fields   TEXT    NOT NULL DEFAULT '[]',     -- JSON array of user-defined field defs
    match_strategy    TEXT    NOT NULL DEFAULT 'exact_forward'
                              CHECK (match_strategy IN ('exact_forward', 'fuzzy')),
    fuzzy_threshold   REAL,                              -- only when match_strategy = 'fuzzy'
    created_at        INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at        INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE contexts (
    id                  INTEGER PRIMARY KEY,
    name                TEXT    NOT NULL UNIQUE,
    description         TEXT,
    chunking_profile_id INTEGER REFERENCES chunking_profiles(id) ON DELETE SET NULL,
    embedding_model_id  INTEGER REFERENCES embedding_models(id) ON DELETE SET NULL,
    embedding_dim       INTEGER,                         -- snapshot of dim used at ingest time
    status              TEXT    NOT NULL DEFAULT 'created'
                                CHECK (status IN ('created', 'ingesting', 'staged', 'embedded', 'error')),
    created_at          INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at          INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE documents (
    id             INTEGER PRIMARY KEY,
    context_id     INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    name           TEXT    NOT NULL,
    zip_entry      TEXT,                                 -- path inside the ingested .zip
    byte_size      INTEGER,
    page_count     INTEGER,
    content_hash   TEXT,
    extracted_text TEXT,                                 -- raw source text for offset-based segmentation
    ingested_at    INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX idx_documents_context ON documents(context_id);

-- ---------------------------------------------------------------------------
-- C. Pipeline staging (producer-consumer, fault-tolerant / resumable)
-- ---------------------------------------------------------------------------

CREATE TABLE prechunks (
    id             INTEGER PRIMARY KEY,
    document_id    INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    idx            INTEGER NOT NULL,                     -- sliding-window order within the document
    start_sentence INTEGER NOT NULL,
    end_sentence   INTEGER NOT NULL,
    char_start     INTEGER NOT NULL,
    char_end       INTEGER NOT NULL,
    text           TEXT    NOT NULL,
    llm_status     TEXT    NOT NULL DEFAULT 'pending'
                           CHECK (llm_status IN ('pending', 'sent', 'done', 'error')),
    llm_response   TEXT,                                 -- raw JSON of boundary signatures
    attempts       INTEGER NOT NULL DEFAULT 0,
    updated_at     INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX idx_prechunks_doc_status ON prechunks(document_id, llm_status);

-- The STAGING TABLE: physical chunks written immediately after assembly.
CREATE TABLE chunks (
    id          INTEGER PRIMARY KEY,
    context_id  INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,                        -- chronological order
    char_start  INTEGER NOT NULL,
    char_end    INTEGER NOT NULL,
    text        TEXT    NOT NULL,
    signature   TEXT,                                    -- boundary signature that opened this chunk
    is_omitted  INTEGER NOT NULL DEFAULT 0,              -- leave_out boundary, kept for audit
    metadata    TEXT    NOT NULL DEFAULT '{}',           -- JSON: LLM-extracted user-defined metadata
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (context_id, document_id, chunk_index)
);
CREATE INDEX idx_chunks_context ON chunks(context_id);

-- ---------------------------------------------------------------------------
-- D. Vectors (raw f32 BLOBs + pure-Rust brute-force cosine)
-- ---------------------------------------------------------------------------

CREATE TABLE embeddings (
    chunk_id           INTEGER PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
    context_id         INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    document_id        INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    embedding_model_id INTEGER NOT NULL REFERENCES embedding_models(id) ON DELETE CASCADE,
    dim                INTEGER NOT NULL,                 -- vector length (matches context.embedding_dim)
    vector             BLOB    NOT NULL,                 -- dim * f32, little-endian, L2-normalized
    created_at         INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX idx_embeddings_context  ON embeddings(context_id);
CREATE INDEX idx_embeddings_document ON embeddings(document_id);

-- ---------------------------------------------------------------------------
-- E. Grid & async matrix-chat (supports the Grid tab)
-- ---------------------------------------------------------------------------

CREATE TABLE grid_sheets (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    columns    TEXT    NOT NULL DEFAULT '[]',            -- JSON column definitions
    row_count  INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE grid_rows (
    id              INTEGER PRIMARY KEY,
    sheet_id        INTEGER REFERENCES grid_sheets(id) ON DELETE CASCADE,
    source_chunk_id INTEGER REFERENCES chunks(id) ON DELETE CASCADE,
    row_index       INTEGER NOT NULL,
    data            TEXT    NOT NULL DEFAULT '{}'        -- JSON: per-column cell values
);
CREATE INDEX idx_grid_rows_sheet ON grid_rows(sheet_id);

-- One row per (run, grid line): chat is NOT history-aware, so results overwrite.
CREATE TABLE grid_chat_results (
    id              INTEGER PRIMARY KEY,
    run_id          TEXT    NOT NULL,
    row_ref_type    TEXT    NOT NULL CHECK (row_ref_type IN ('chunk', 'grid_row')),
    row_ref_id      INTEGER NOT NULL,
    prompt          TEXT    NOT NULL,                    -- per-line prompt (may be edited)
    columns_context TEXT,                                -- snapshot of selected-column content
    retrieved_refs  TEXT,                                -- JSON: [{chunk_id, score}, ...]
    response        TEXT,
    status          TEXT    NOT NULL DEFAULT 'queued'
                            CHECK (status IN ('queued', 'retrieving', 'llm', 'done', 'error')),
    error           TEXT,
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (run_id, row_ref_type, row_ref_id)
);

-- ---------------------------------------------------------------------------
-- F. Settings (key/value)
-- ---------------------------------------------------------------------------

CREATE TABLE app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL                                  -- JSON-encoded value
);

INSERT INTO app_settings (key, value) VALUES
    ('log_level',         '"info"'),
    ('max_parallel_chats', '8'),
    ('top_k_default',      '5');
