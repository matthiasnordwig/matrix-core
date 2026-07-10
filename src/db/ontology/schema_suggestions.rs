//! Read-only frequency analysis over permanently-kept raw types, feeding
//! schema-gap suggestions ("this context's raw output keeps producing a type
//! your profile has no slot for") — see BACKLOG.md "Schema-Typ-Vorschläge aus
//! Rohtyp-Häufigkeiten". Pure SQL, no LLM; canonicalization of the resulting
//! raw-type clusters against a broad reference catalog lives in
//! `app/src-tauri/src/ontology/extract/schema_suggest.rs`.
use crate::db::{Database, Result};

impl Database {
    pub fn raw_entity_type_frequencies(&self, context_id: i64, min_count: i64) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT raw_entity_type, COUNT(*) as cnt FROM ontology_nodes
             WHERE context_id = ?1 GROUP BY raw_entity_type HAVING cnt > ?2 ORDER BY cnt DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![context_id, min_count], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn raw_relation_type_frequencies(&self, context_id: i64, min_count: i64) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT raw_relation_type, COUNT(*) as cnt FROM ontology_edges
             WHERE context_id = ?1 GROUP BY raw_relation_type HAVING cnt > ?2 ORDER BY cnt DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![context_id, min_count], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Groups edges the *active* lens marked `deleted` by
    /// `(relation_type, resolved_source_type, resolved_target_type)` — feeds
    /// "constraint-gap" suggestions (see BACKLOG.md "Constraint-Lücken-
    /// Vorschläge"): a relation type is known and allowed, but its profile
    /// constraint list has no entry for this source/target type pair, so the
    /// lens drops the edge. `relation_type`/the two type columns are each
    /// `COALESCE(lens-resolved value, raw value)` — same fallback pattern as
    /// `edges.rs::list_ontology_edges_for_active_lens`/`sanitize.rs`, since a
    /// `deleted` verdict can still carry a `resolved_relation_type` (the
    /// relation snap happens before the constraint check in
    /// `materialize_lens`) and node type rows are always present once a lens
    /// is materialized (exhaustive resolution, see `sanitize.rs`).
    ///
    /// Deliberately NOT restricted to constraint-caused deletions: a `deleted`
    /// verdict can also come from `verify_edge_polarity`'s negation check
    /// (`upsert_lens_edge_verdict(..., "deleted", None)`), and the two causes
    /// aren't distinguishable from the verdict row alone (no `reason`/source
    /// column on `ontology_lens_edge_verdicts`). Grouping by
    /// (relation_type, source_type, target_type) is still meaningful either
    /// way — it surfaces exactly the "this type combination keeps getting
    /// dropped" signal the suggestion is meant to catch — but a high count
    /// here does not guarantee every one of those edges failed on a
    /// constraint rather than a negation verdict.
    pub fn deleted_relation_constraint_frequencies(&self, context_id: i64, min_count: i64) -> Result<Vec<(String, String, String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                COALESCE(v.resolved_relation_type, e.raw_relation_type) AS relation_type,
                COALESCE(sn.resolved_type, s.raw_entity_type) AS source_type,
                COALESCE(tn.resolved_type, t.raw_entity_type) AS target_type,
                COUNT(*) as cnt
             FROM ontology_edges e
             JOIN contexts ctx ON ctx.id = e.context_id
             JOIN ontology_lens_edge_verdicts v ON v.edge_id = e.id AND v.lens_id = ctx.active_lens_id
             JOIN ontology_nodes s ON s.id = e.source_id
             JOIN ontology_nodes t ON t.id = e.target_id
             LEFT JOIN ontology_lens_node_types sn ON sn.node_id = s.id AND sn.lens_id = ctx.active_lens_id
             LEFT JOIN ontology_lens_node_types tn ON tn.node_id = t.id AND tn.lens_id = ctx.active_lens_id
             WHERE e.context_id = ?1 AND v.verdict = 'deleted'
             GROUP BY relation_type, source_type, target_type
             HAVING cnt > ?2
             ORDER BY cnt DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![context_id, min_count], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use crate::db::models::*;
    use crate::db::Database;

    fn db() -> Database {
        Database::open_in_memory().expect("open in-memory db")
    }

    fn seed_context(db: &Database, name: &str) -> (i64, i64) {
        let model = db
            .create_embedding_model(&NewEmbeddingModel {
                identifier: format!("test-embed-{name}"),
                kind: ModelKind::LocalOnnx,
                model_path: Some("/models/test.onnx".into()),
                tokenizer_path: None,
                api_config: None,
                execution_provider: Some(ExecutionProvider::Ane),
                is_matryoshka: false,
                native_dim: 2,
                default_dim: 2,
                normalize: true,
                tpm_limit: None,
                rpm_limit: None,
                max_concurrency: 1,
            })
            .unwrap();
        let ctx = db
            .create_context(&NewContext {
                name: name.into(),
                description: None,
                chunking_profile_id: None,
                embedding_model_id: Some(model.id),
                embedding_dim: Some(2),
                llm_id: None,
                fallback_llm_id: None,
                ontology_profile_id: None,
                ontology_pool_id: None,
                ontology_extract_llm_id: None,
                ontology_extract_pool_id: None,
                ontology_extract_reasoning_effort: None,
                extract_title_llm: false,
                auto_merge_ontology: false,
                chunking_strategy: "Semantic".into(),
                structural_profile_id: None,
            })
            .unwrap();
        let doc = db
            .create_document(&NewDocument {
                context_id: ctx.id,
                name: "doc.pdf".into(),
                zip_entry: None,
                byte_size: None,
                page_count: None,
                content_hash: None,
                extracted_text: None,
            })
            .unwrap();
        let chunk = db
            .create_chunk(&NewChunk {
                context_id: ctx.id,
                document_id: doc.id,
                chunk_index: 0,
                char_start: 0,
                char_end: 1,
                text: "chunk".into(),
                signature: None,
                is_omitted: false,
                metadata: "{}".into(),
            })
            .unwrap();
        (ctx.id, chunk.id)
    }

    #[test]
    fn raw_entity_type_frequencies_counts_and_filters_by_threshold() {
        let db = db();
        let (ctx, _) = seed_context(&db, "Ctx1");
        for _ in 0..3 {
            db.create_ontology_node(&NewOntologyNode { context_id: ctx, label: "x".into(), entity_type: "COUNTRY".into(), description: String::new() }).unwrap();
        }
        db.create_ontology_node(&NewOntologyNode { context_id: ctx, label: "y".into(), entity_type: "ORGANIZATION".into(), description: String::new() }).unwrap();

        let freq = db.raw_entity_type_frequencies(ctx, 1).unwrap();
        assert_eq!(freq, vec![("COUNTRY".to_string(), 3)], "ORGANIZATION has count 1, must be filtered out by min_count=1 (HAVING cnt > 1)");

        let freq_all = db.raw_entity_type_frequencies(ctx, 0).unwrap();
        assert_eq!(freq_all.len(), 2);
    }

    #[test]
    fn raw_relation_type_frequencies_counts_per_context() {
        let db = db();
        let (ctx, chunk_id) = seed_context(&db, "Ctx2");
        let a = db.create_ontology_node(&NewOntologyNode { context_id: ctx, label: "A".into(), entity_type: "ORG".into(), description: String::new() }).unwrap();
        let b = db.create_ontology_node(&NewOntologyNode { context_id: ctx, label: "B".into(), entity_type: "ORG".into(), description: String::new() }).unwrap();
        let c = db.create_ontology_node(&NewOntologyNode { context_id: ctx, label: "C".into(), entity_type: "ORG".into(), description: String::new() }).unwrap();
        db.create_ontology_edge(&NewOntologyEdge { context_id: ctx, source_id: a.id, target_id: b.id, relation_type: "APPLIES_TO".into(), chunk_id, evidence: None }).unwrap();
        db.create_ontology_edge(&NewOntologyEdge { context_id: ctx, source_id: b.id, target_id: c.id, relation_type: "APPLIES_TO".into(), chunk_id, evidence: None }).unwrap();

        let freq = db.raw_relation_type_frequencies(ctx, 1).unwrap();
        assert_eq!(freq, vec![("APPLIES_TO".to_string(), 2)]);
    }

    /// Lens setup shared by the `deleted_relation_constraint_frequencies`
    /// tests: a profile + materialized lens (mirrors what
    /// `sanitize::materialize_lens` would have written), activated on the
    /// context so the query's `ctx.active_lens_id` join resolves.
    fn seed_active_lens(db: &Database, ctx: i64) -> i64 {
        let profile = db
            .create_ontology_profile(&NewOntologyProfile {
                name: "Compliance".into(),
                entity_types_json: "[\"ORGANIZATION\",\"REGULATION\"]".into(),
                relation_types_json: "[\"TRIGGERS\"]".into(),
                extract_prompt: None,
                dedup_prompt: None,
                community_prompt: None,
            })
            .unwrap();
        let lens = db.get_or_create_lens(ctx, "Compliance", profile.id, true).unwrap();
        db.set_context_active_lens(ctx, Some(lens.id)).unwrap();
        lens.id
    }

    #[test]
    fn deleted_relation_constraint_frequencies_groups_by_relation_and_resolved_types() {
        let db = db();
        let (ctx, chunk_id) = seed_context(&db, "Ctx3");
        let lens_id = seed_active_lens(&db, ctx);

        // Three distinct REGULATION->ORGANIZATION TRIGGERS edges deleted for
        // the same resolved type pair (constraint only allows the reverse
        // direction) — distinct source nodes, since (context, source,
        // target, relation_type) is UNIQUE (schema_v28.sql) and would
        // collapse three identical edges into one. Plus one unrelated
        // APPLIES_TO deletion that must land in its own group.
        let b1 = db.create_ontology_node(&NewOntologyNode { context_id: ctx, label: "Org1".into(), entity_type: "ORGANIZATION".into(), description: String::new() }).unwrap();
        db.upsert_lens_node_type(lens_id, b1.id, "ORGANIZATION").unwrap();

        let mut last_source = 0;
        for i in 0..3 {
            let a = db.create_ontology_node(&NewOntologyNode { context_id: ctx, label: format!("Reg{i}"), entity_type: "REGULATION".into(), description: String::new() }).unwrap();
            db.upsert_lens_node_type(lens_id, a.id, "REGULATION").unwrap();
            last_source = a.id;
            let e = db.create_ontology_edge(&NewOntologyEdge { context_id: ctx, source_id: a.id, target_id: b1.id, relation_type: "TRIGGERS".into(), chunk_id, evidence: None }).unwrap();
            db.upsert_lens_edge_verdict(lens_id, e.id, "deleted", Some("TRIGGERS")).unwrap();
        }

        let c = db.create_ontology_node(&NewOntologyNode { context_id: ctx, label: "Thing".into(), entity_type: "CONCEPT".into(), description: String::new() }).unwrap();
        db.upsert_lens_node_type(lens_id, c.id, "CONCEPT").unwrap();
        let e_other = db.create_ontology_edge(&NewOntologyEdge { context_id: ctx, source_id: last_source, target_id: c.id, relation_type: "APPLIES_TO".into(), chunk_id, evidence: None }).unwrap();
        db.upsert_lens_edge_verdict(lens_id, e_other.id, "deleted", Some("APPLIES_TO")).unwrap();

        let freq = db.deleted_relation_constraint_frequencies(ctx, 2).unwrap();
        assert_eq!(freq, vec![("TRIGGERS".to_string(), "REGULATION".to_string(), "ORGANIZATION".to_string(), 3)],
            "APPLIES_TO group has count 1, must be filtered out by min_count=2 (HAVING cnt > 2)");

        let freq_all = db.deleted_relation_constraint_frequencies(ctx, 0).unwrap();
        assert_eq!(freq_all.len(), 2);
    }

    #[test]
    fn deleted_relation_constraint_frequencies_ignores_valid_and_reversed_verdicts() {
        let db = db();
        let (ctx, chunk_id) = seed_context(&db, "Ctx4");
        let lens_id = seed_active_lens(&db, ctx);

        let a = db.create_ontology_node(&NewOntologyNode { context_id: ctx, label: "Reg".into(), entity_type: "REGULATION".into(), description: String::new() }).unwrap();
        let b = db.create_ontology_node(&NewOntologyNode { context_id: ctx, label: "Org".into(), entity_type: "ORGANIZATION".into(), description: String::new() }).unwrap();
        db.upsert_lens_node_type(lens_id, a.id, "REGULATION").unwrap();
        db.upsert_lens_node_type(lens_id, b.id, "ORGANIZATION").unwrap();

        let e_valid = db.create_ontology_edge(&NewOntologyEdge { context_id: ctx, source_id: a.id, target_id: b.id, relation_type: "TRIGGERS".into(), chunk_id, evidence: None }).unwrap();
        db.upsert_lens_edge_verdict(lens_id, e_valid.id, "valid", Some("TRIGGERS")).unwrap();
        let e_reversed = db.create_ontology_edge(&NewOntologyEdge { context_id: ctx, source_id: a.id, target_id: b.id, relation_type: "TRIGGERS".into(), chunk_id, evidence: None }).unwrap();
        db.upsert_lens_edge_verdict(lens_id, e_reversed.id, "reversed", Some("TRIGGERS")).unwrap();

        assert!(db.deleted_relation_constraint_frequencies(ctx, 0).unwrap().is_empty(), "only 'deleted' verdicts should be counted, not 'valid'/'reversed'");
    }

    #[test]
    fn deleted_relation_constraint_frequencies_falls_back_to_raw_type_without_lens_node_row() {
        // Defensive case: a deleted edge whose endpoint somehow has no
        // ontology_lens_node_types row for the active lens (shouldn't happen
        // once materialize_lens's exhaustive resolution has run, but the
        // LEFT JOIN + COALESCE must not silently drop the row).
        let db = db();
        let (ctx, chunk_id) = seed_context(&db, "Ctx5");
        let lens_id = seed_active_lens(&db, ctx);

        let b = db.create_ontology_node(&NewOntologyNode { context_id: ctx, label: "Org".into(), entity_type: "RAW_ORG".into(), description: String::new() }).unwrap();
        // Deliberately no upsert_lens_node_type for either node.

        for i in 0..2 {
            let a = db.create_ontology_node(&NewOntologyNode { context_id: ctx, label: format!("Reg{i}"), entity_type: "RAW_REG".into(), description: String::new() }).unwrap();
            let e = db.create_ontology_edge(&NewOntologyEdge { context_id: ctx, source_id: a.id, target_id: b.id, relation_type: "TRIGGERS".into(), chunk_id, evidence: None }).unwrap();
            db.upsert_lens_edge_verdict(lens_id, e.id, "deleted", Some("TRIGGERS")).unwrap();
        }

        let freq = db.deleted_relation_constraint_frequencies(ctx, 1).unwrap();
        assert_eq!(freq, vec![("TRIGGERS".to_string(), "RAW_REG".to_string(), "RAW_ORG".to_string(), 2)]);
    }
}
