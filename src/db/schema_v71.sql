-- schema_v71: where the documents of a collection came from on disk, so the
-- stand-alone MCP program can keep the index up to date (`add_files` writes
-- these rows; the app's uploads have none and are never compared with a disk).
--
-- `source_roots`: a file or folder given to `add_files` (canonical path), with
-- `recursive` as asked and the time of the last comparison with the disk.
-- `source_files`: one row per document read from a file (a zip gives several
-- rows of the same path): size, modification time (ns) and SHA-256 of the
-- file as read. `missing_since` = the file was gone at a check (the document
-- and its chunks stay; NULL = present). `pending` = written together with the
-- document while its file is read (`add_files`) or until an update has finished:
-- a run that was cut off leaves only such rows behind, and the next update of
-- the collection removes their documents (an older version was never touched;
-- a document without any row is never removed).
-- `source_leases`: who updates a collection right now (`add_files` or an
-- update; holder = one running program, `expires_at` renewed while it works),
-- so two programs on the same data folder never work on one collection at once.
CREATE TABLE source_roots (
    id          INTEGER PRIMARY KEY,
    context_id  INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    path        TEXT    NOT NULL,
    is_dir      INTEGER NOT NULL,
    recursive   INTEGER NOT NULL,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    checked_at  INTEGER,
    UNIQUE (context_id, path)
);

CREATE TABLE source_files (
    document_id   INTEGER PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
    context_id    INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    path          TEXT    NOT NULL,
    byte_size     INTEGER NOT NULL,
    mtime_ns      INTEGER NOT NULL,
    sha256        TEXT    NOT NULL,
    missing_since INTEGER,
    pending       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_source_files_context_path ON source_files(context_id, path);

CREATE TABLE source_leases (
    context_id  INTEGER PRIMARY KEY REFERENCES contexts(id) ON DELETE CASCADE,
    holder      TEXT    NOT NULL,
    expires_at  INTEGER NOT NULL
);
