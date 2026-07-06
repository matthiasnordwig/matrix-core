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
}
