-- TOOL_TIER_PLAN.md Teil B / AP4 — Reference-Abbreviations registry.
--
-- A data-driven, ProfilesTab-maintained set of law/act Kürzel + their long
-- forms, so `refs::parse_refs_with` can recognize both "§ 14 EnWG" (Kürzel)
-- and "§ 14 des Energiewirtschaftsgesetzes" (long form) without a core
-- rebuild. Seeded idempotently from the built-in list (`refs::RefLexicon::
-- builtin()`) by `services::seed::seed_defaults` — this migration only
-- creates the empty table; an empty/missing-seed table falls back to the
-- built-in lexicon at read time (`Database::ref_lexicon`), so old/unseeded
-- DBs behave exactly as before.
--
-- `long_names` is a JSON array of strings (same TEXT-column-as-JSON pattern
-- as `reasoning_effort_lists.allowed_efforts`, schema_v46).
--
-- Generic migration path (no Rust rebuild hook): a plain CREATE TABLE runs
-- through db/mod.rs::migrate's default arm, same as schema_v55.

CREATE TABLE IF NOT EXISTS ref_abbreviations (
    id INTEGER PRIMARY KEY,
    kuerzel TEXT NOT NULL UNIQUE COLLATE NOCASE,
    long_names TEXT NOT NULL DEFAULT '[]', -- JSON array of long-form strings
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
