-- Attempt counter for edge reviews, used only by the "verification call failed"
-- class (see ontology/extract/verify.rs): each in-run failure or manual
-- re-verify increments it, and once it reaches MAX_VERIFY_ATTEMPTS the review
-- is ejected from the human queue automatically (an unverifiable edge stays in
-- the graph unchanged — the polarity check is a soft precision pass, not a
-- blocker). Additive; existing rows default to 0.
ALTER TABLE ontology_edge_reviews ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
