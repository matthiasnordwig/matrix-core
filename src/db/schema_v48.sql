-- schema_v48: FTS5 keyword index over chunks(text) for hybrid retrieval.
--
-- External-content FTS5 table mirroring `chunks(text)` (content=chunks,
-- content_rowid=id). Tokenizer strips diacritics so German umlaut/base-form
-- queries match. Additive only: `chunks` itself is untouched; INSERT/UPDATE/
-- DELETE triggers keep the index in sync, and a one-time backfill loads any
-- rows that already exist.

CREATE VIRTUAL TABLE chunks_fts USING fts5(
    text,
    content='chunks',
    content_rowid='id',
    tokenize='unicode61 remove_diacritics 2'
);

-- Backfill existing rows (external-content: 'rebuild' rescans `chunks`).
INSERT INTO chunks_fts(chunks_fts) VALUES('rebuild');

-- Keep the index in sync. For external-content tables the delete/update
-- triggers must post a 'delete' row (special rowid = old.id) before inserting
-- the new content, otherwise the stored index and content drift apart.
CREATE TRIGGER chunks_fts_ai AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER chunks_fts_ad AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES('delete', old.id, old.text);
END;

CREATE TRIGGER chunks_fts_au AFTER UPDATE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES('delete', old.id, old.text);
    INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
END;
