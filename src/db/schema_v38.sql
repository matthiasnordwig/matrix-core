-- Merge-log for ontology dedup (see ISSUES.md "ontology_dedup_cache — keine
-- Merge-Historie ..."): merge_ontology_nodes hard-deletes the losing node
-- row, so label/type were unrecoverable afterwards, making any later dedup
-- quality/recall analysis structurally impossible. This table records the
-- loser's label/type right before the DELETE — purely additive, never read
-- by the pipeline itself, only for later retrieval/eval.
--
-- Deliberately NO foreign key on winner_id/loser_id -> ontology_nodes: the
-- loser is being deleted in the very same transaction this row is written
-- in, and an FK would either block that DELETE or cascade-delete this log
-- row right along with it — exactly the information loss this table exists
-- to prevent. Only context_id is a real FK (CASCADE, so the log doesn't
-- outlive its context).
CREATE TABLE ontology_merge_log (
    id                INTEGER PRIMARY KEY,
    context_id        INTEGER NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    winner_id         INTEGER NOT NULL,
    loser_id          INTEGER NOT NULL,
    loser_label       TEXT    NOT NULL,
    loser_entity_type TEXT    NOT NULL,
    merged_at         INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX idx_ontology_merge_log_context ON ontology_merge_log(context_id);
