-- Per-lens communities + lazy summary cache (see BACKLOG.md "Per-Lens
-- Community-Recompute + lazy gecachte Summaries"): communities are computed
-- per lens instead of single-valued via ontology_nodes.community_id.
-- lens_id NULL = raw/unfiltered view (and all pre-migration legacy rows).
ALTER TABLE ontology_communities ADD COLUMN lens_id INTEGER REFERENCES ontology_lenses(id) ON DELETE CASCADE;

-- Summary-cache key: the community's member node ids, sorted ascending and
-- comma-joined (deterministic, no hash dependency). Cache lookups are keyed
-- by (context_id, lens_id, members_key) — the same node set under two
-- different lenses can legitimately deserve different summaries (different
-- resolved relation types), so the key is per-lens, not global
-- (architecture-review 2026-07-06). NULL on legacy rows (they never
-- cache-hit; harmless). Deliberately NOT a UNIQUE constraint: SQLite treats
-- NULLs as distinct (so it wouldn't constrain the NULL-lens raw view at
-- all), and detection produces disjoint member sets per run anyway.
ALTER TABLE ontology_communities ADD COLUMN members_key TEXT;

-- Per-community membership, replacing the single-valued
-- ontology_nodes.community_id write path (that column stays readable for
-- legacy DBs but is no longer written). ON DELETE CASCADE on node_id means a
-- dedup merge (which hard-deletes the losing node row) or manual node delete
-- silently shrinks stale memberships — safe, because members_key is always
-- re-derived from live node ids at (re)compute time, so a stale key can only
-- cache-miss, never falsely hit.
CREATE TABLE ontology_community_members (
    community_id INTEGER NOT NULL REFERENCES ontology_communities(id) ON DELETE CASCADE,
    node_id INTEGER NOT NULL REFERENCES ontology_nodes(id) ON DELETE CASCADE,
    PRIMARY KEY (community_id, node_id)
);
CREATE INDEX idx_ontology_community_members_node ON ontology_community_members(node_id);

-- Serves both the per-lens scan (prefix) and the members_key cache lookup.
CREATE INDEX idx_ontology_communities_ctx_lens_key ON ontology_communities(context_id, lens_id, members_key);

-- Backfill membership from the legacy single-valued column so existing
-- graphs keep their community coloring until the next recompute (legacy
-- rows keep lens_id NULL = raw view; members_key stays NULL, see above).
INSERT INTO ontology_community_members (community_id, node_id)
SELECT n.community_id, n.id FROM ontology_nodes n
JOIN ontology_communities co ON co.id = n.community_id
WHERE n.community_id IS NOT NULL;
