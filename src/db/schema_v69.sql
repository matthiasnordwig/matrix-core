-- schema_v69: which bundled sample collection a context was imported from
-- (`contexts.sample_key`, e.g. "hudson", "panama", "contract"; NULL = the
-- user's own collection). The app shows the sample's curated example questions
-- and its source/license note by this key, never by the collection name.
-- Travels in the export bundle with the rest of the context row.
ALTER TABLE contexts ADD COLUMN sample_key TEXT;
