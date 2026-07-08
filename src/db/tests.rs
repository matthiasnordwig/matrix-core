//! Round-trip and constraint tests, all against an in-memory database.

use super::embeddings::{blob_to_vector, vector_to_blob};
use super::models::*;
use super::{CoreError, Database};

fn db() -> Database {
    Database::open_in_memory().expect("open in-memory db")
}

/// Build a model → profile → context → document chain and return their ids.
fn seed(db: &Database) -> (i64, i64, i64, i64) {
    let model = db
        .create_embedding_model(&NewEmbeddingModel {
            identifier: "test-embed".into(),
            kind: ModelKind::LocalOnnx,
            model_path: Some("/models/test.onnx".into()),
            tokenizer_path: Some("/models/tokenizer.json".into()),
            api_config: None,
            execution_provider: Some(ExecutionProvider::Ane),
            is_matryoshka: false,
            native_dim: 4,
            default_dim: 4,
            normalize: true,
            tpm_limit: None,
            rpm_limit: None,
            max_concurrency: 1,
        })
        .unwrap();

    let profile = db
        .create_chunking_profile(&NewChunkingProfile {
            name: "default".into(),
            prompt: "Split this: {{pre_chunk}}".into(),
            overlap_ratio: 0.2,
            max_signature_len: 80,
            metadata_fields: "[]".into(),
            match_strategy: MatchStrategy::ExactForward,
            fuzzy_threshold: None,
        })
        .unwrap();

    let ctx = db
        .create_context(&NewContext {
            name: "Context_A".into(),
            description: Some("test".into()),
            chunking_profile_id: Some(profile.id),
            embedding_model_id: Some(model.id),
            embedding_dim: Some(4),
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
            name: "doc_1.pdf".into(),
            zip_entry: Some("doc_1.pdf".into()),
            byte_size: Some(1234),
            page_count: Some(3),
            content_hash: Some("abc".into()),
            extracted_text: Some("Sentence one. Sentence two.".into()),
        })
        .unwrap();

    (model.id, profile.id, ctx.id, doc.id)
}

#[test]
fn migration_sets_version_and_seeds_settings() {
    let db = db();
    assert_eq!(db.schema_version().unwrap(), 44);
    // seeded defaults from schema_v1.sql
    let top_k: i64 = db.get_setting("top_k_default").unwrap().unwrap();
    assert_eq!(top_k, 5);
    let level: String = db.get_setting("log_level").unwrap().unwrap();
    assert_eq!(level, "info");
}

#[test]
fn registries_and_context_chain_round_trip() {
    let db = db();
    let (model_id, profile_id, ctx_id, _doc) = seed(&db);

    let ctx = db.context(ctx_id).unwrap().unwrap();
    assert_eq!(ctx.embedding_model_id, Some(model_id));
    assert_eq!(ctx.chunking_profile_id, Some(profile_id));
    assert_eq!(ctx.status, ContextStatus::Created);

    db.set_context_status(ctx_id, ContextStatus::Staged).unwrap();
    assert_eq!(db.context(ctx_id).unwrap().unwrap().status, ContextStatus::Staged);

    assert_eq!(db.list_embedding_models().unwrap().len(), 1);
    assert_eq!(db.list_contexts().unwrap().len(), 1);
}

#[test]
fn foreign_keys_are_enforced() {
    let db = db();
    // No such context/document → FK violation.
    let bad = NewChunk {
        context_id: 999,
        document_id: 999,
        chunk_index: 0,
        char_start: 0,
        char_end: 1,
        text: "x".into(),
        signature: None,
        is_omitted: false,
        metadata: "{}".into(),
    };
    assert!(db.create_chunk(&bad).is_err());
}

#[test]
fn staging_chunk_index_is_unique() {
    let db = db();
    let (_m, _p, ctx_id, doc_id) = seed(&db);

    let mk = |idx: i64| NewChunk {
        context_id: ctx_id,
        document_id: doc_id,
        chunk_index: idx,
        char_start: 0,
        char_end: 10,
        text: "chunk".into(),
        signature: Some("Sentence one.".into()),
        is_omitted: false,
        metadata: "{}".into(),
    };

    db.create_chunk(&mk(0)).unwrap();
    assert!(db.create_chunk(&mk(0)).is_err(), "duplicate chunk_index must fail");
    db.create_chunk(&mk(1)).unwrap();
    assert_eq!(db.count_chunks(ctx_id).unwrap(), 2);
}

#[test]
fn chunks_by_ids_batch_fetch() {
    let db = db();
    let (_m, _p, ctx_id, doc_id) = seed(&db);

    let mk = |idx: i64| NewChunk {
        context_id: ctx_id,
        document_id: doc_id,
        chunk_index: idx,
        char_start: 0,
        char_end: 10,
        text: format!("chunk {idx}"),
        signature: None,
        is_omitted: false,
        metadata: "{}".into(),
    };
    let a = db.create_chunk(&mk(0)).unwrap();
    let b = db.create_chunk(&mk(1)).unwrap();
    let _c = db.create_chunk(&mk(2)).unwrap();

    let fetched = db.chunks_by_ids(&[a.id, b.id]).unwrap();
    assert_eq!(fetched.len(), 2);
    let texts: Vec<&str> = fetched.iter().map(|c| c.text.as_str()).collect();
    assert!(texts.contains(&"chunk 0"));
    assert!(texts.contains(&"chunk 1"));

    assert!(db.chunks_by_ids(&[]).unwrap().is_empty());

    // Non-existent ids are silently skipped, not an error.
    let partial = db.chunks_by_ids(&[a.id, 999_999]).unwrap();
    assert_eq!(partial.len(), 1);
}

#[test]
fn prechunk_resume_workflow() {
    let db = db();
    let (_m, _p, _ctx, doc_id) = seed(&db);

    let pc = db
        .create_prechunk(&NewPrechunk {
            document_id: doc_id,
            idx: 0,
            start_sentence: 0,
            end_sentence: 20,
            char_start: 0,
            char_end: 100,
            text: "window text".into(),
        })
        .unwrap();

    assert_eq!(db.pending_prechunks(doc_id).unwrap().len(), 1);
    db.set_prechunk_result(pc.id, PrechunkStatus::Done, Some("{\"boundaries\":[]}"))
        .unwrap();
    assert!(db.pending_prechunks(doc_id).unwrap().is_empty());
    assert_eq!(db.prechunk(pc.id).unwrap().unwrap().attempts, 1);
}

#[test]
fn vector_blob_round_trip_is_bit_exact() {
    let v = vec![0.0f32, 1.5, -2.25, 3.125, f32::MIN, f32::MAX];
    let back = blob_to_vector(&vector_to_blob(&v)).unwrap();
    assert_eq!(v, back);
}

#[test]
fn corrupt_blob_is_rejected() {
    assert!(blob_to_vector(&[0u8, 1, 2]).is_err());
}

#[test]
fn dim_mismatch_is_rejected_on_insert() {
    let db = db();
    let (model_id, _p, ctx_id, doc_id) = seed(&db);
    let chunk = db
        .create_chunk(&NewChunk {
            context_id: ctx_id,
            document_id: doc_id,
            chunk_index: 0,
            char_start: 0,
            char_end: 5,
            text: "c".into(),
            signature: None,
            is_omitted: false,
            metadata: "{}".into(),
        })
        .unwrap();

    let bad = NewEmbedding {
        chunk_id: chunk.id,
        context_id: ctx_id,
        document_id: doc_id,
        embedding_model_id: model_id,
        dim: 4,
        vector: vec![1.0, 0.0], // len 2 != dim 4
    };
    assert!(db.insert_embedding(&bad).is_err());
}

#[test]
fn brute_force_cosine_ranks_nearest() {
    let db = db();
    let (model_id, _p, ctx_id, doc_id) = seed(&db);

    // Three chunks with distinct normalized 4-d vectors.
    let vectors = [
        (vec![1.0, 0.0, 0.0, 0.0]),
        (vec![0.0, 1.0, 0.0, 0.0]),
        (vec![0.0, 0.0, 1.0, 0.0]),
    ];
    let mut chunk_ids = Vec::new();
    for (i, v) in vectors.iter().enumerate() {
        let chunk = db
            .create_chunk(&NewChunk {
                context_id: ctx_id,
                document_id: doc_id,
                chunk_index: i as i64,
                char_start: 0,
                char_end: 1,
                text: format!("chunk {i}"),
                signature: None,
                is_omitted: false,
                metadata: "{}".into(),
            })
            .unwrap();
        db.insert_embedding(&NewEmbedding {
            chunk_id: chunk.id,
            context_id: ctx_id,
            document_id: doc_id,
            embedding_model_id: model_id,
            dim: 4,
            vector: v.clone(),
        })
        .unwrap();
        chunk_ids.push(chunk.id);
    }

    // Query closest to the second vector.
    let hits = db.search_context(ctx_id, &[0.1, 0.9, 0.0, 0.0], 2).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].chunk_id, chunk_ids[1]);
    assert!(hits[0].score > hits[1].score);

    // Round-trip a stored vector exactly.
    assert_eq!(db.embedding_vector(chunk_ids[0]).unwrap().unwrap(), vectors[0]);

    assert_eq!(db.count_embeddings_for_context(ctx_id).unwrap(), 3);
}

#[test]
fn chunks_embeddable_returns_all_non_omitted_regardless_of_embedding() {
    let db = db();
    let (model_id, _p, ctx_id, doc_id) = seed(&db);

    let mk = |idx: i64, omitted: bool| NewChunk {
        context_id: ctx_id,
        document_id: doc_id,
        chunk_index: idx,
        char_start: 0,
        char_end: 1,
        text: format!("chunk {idx}"),
        signature: None,
        is_omitted: omitted,
        metadata: "{}".into(),
    };
    let a = db.create_chunk(&mk(0, false)).unwrap();
    let _b = db.create_chunk(&mk(1, false)).unwrap();
    let _omitted = db.create_chunk(&mk(2, true)).unwrap();

    // Embed only the first chunk.
    db.insert_embedding(&NewEmbedding {
        chunk_id: a.id,
        context_id: ctx_id,
        document_id: doc_id,
        embedding_model_id: model_id,
        dim: 4,
        vector: vec![1.0, 0.0, 0.0, 0.0],
    })
    .unwrap();

    // "needing" excludes the already-embedded chunk (and always the omitted one).
    let needing = db.chunks_needing_embedding(ctx_id, model_id).unwrap();
    assert_eq!(needing.len(), 1, "only the un-embedded, non-omitted chunk");

    // "embeddable" (re-embed path) returns every non-omitted chunk, embedded or
    // not — but never the omitted one — ordered by (document_id, chunk_index).
    let embeddable = db.chunks_embeddable(ctx_id).unwrap();
    assert_eq!(embeddable.len(), 2);
    assert_eq!(embeddable.iter().map(|c| c.chunk_index).collect::<Vec<_>>(), vec![0, 1]);
    assert!(embeddable.iter().all(|c| !c.is_omitted));
}

#[test]
fn grid_chat_result_overwrites_no_history() {
    let db = db();
    let run = "run-1";
    let mut upsert = GridChatUpsert {
        run_id: run.into(),
        row_uid: "grid_row:42".into(),
        prompt_snapshot: Some("{}".to_string()),
        row_ref_type: RowRefType::GridRow,
        row_ref_id: 42,
        prompt: "Summarize".into(),
        columns_context: Some("col data".into()),
        retrieved_refs: None,
        response: None,
        status: ChatStatus::Queued,
        error: None,
        row_source_text: None,
    };
    db.upsert_grid_chat_result(&upsert).unwrap();

    upsert.status = ChatStatus::Done;
    upsert.response = Some("the answer".into());
    let result = db.upsert_grid_chat_result(&upsert).unwrap();

    assert_eq!(result.status, ChatStatus::Done);
    assert_eq!(result.response.as_deref(), Some("the answer"));
    // overwrite, not append
    assert_eq!(db.count_grid_chat_results(run).unwrap(), 1);
    assert_eq!(db.list_grid_chat_results(run).unwrap().len(), 1);
}

/// Two rows with the same `(run_id, row_ref_id)` but distinct `row_uid`
/// (as produced by a JSON explode) must persist as two separate records.
#[test]
fn grid_chat_result_distinct_row_uid_keeps_both() {
    let db = db();
    let run = "run-explode";
    let base = GridChatUpsert {
        run_id: run.into(),
        row_uid: String::new(),
        prompt_snapshot: None,
        row_ref_type: RowRefType::Chunk,
        row_ref_id: 7,
        prompt: "p".into(),
        columns_context: None,
        retrieved_refs: None,
        response: Some("first".into()),
        status: ChatStatus::Done,
        error: None,
        row_source_text: None,
    };

    let mut a = base.clone();
    a.row_uid = "7:e0".into();
    db.upsert_grid_chat_result(&a).unwrap();

    let mut b = base.clone();
    b.row_uid = "7:e1".into();
    b.response = Some("second".into());
    db.upsert_grid_chat_result(&b).unwrap();

    assert_eq!(db.count_grid_chat_results(run).unwrap(), 2);
    let rows = db.list_grid_chat_results(run).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.row_ref_id == 7));
}

/// A second upsert with the same `(run_id, row_uid)` updates in place.
#[test]
fn grid_chat_result_same_row_uid_updates() {
    let db = db();
    let run = "run-retry";
    let mut upsert = GridChatUpsert {
        run_id: run.into(),
        row_uid: "chunk:9".into(),
        prompt_snapshot: None,
        row_ref_type: RowRefType::Chunk,
        row_ref_id: 9,
        prompt: "p".into(),
        columns_context: None,
        retrieved_refs: None,
        response: None,
        status: ChatStatus::Error,
        error: Some("boom".into()),
        row_source_text: None,
    };
    db.upsert_grid_chat_result(&upsert).unwrap();

    upsert.status = ChatStatus::Done;
    upsert.error = None;
    upsert.response = Some("recovered".into());
    let result = db.upsert_grid_chat_result(&upsert).unwrap();

    assert_eq!(result.status, ChatStatus::Done);
    assert_eq!(result.response.as_deref(), Some("recovered"));
    assert_eq!(db.count_grid_chat_results(run).unwrap(), 1);
}

/// `grid_run_meta` holds the run's system prompt once; a second upsert for
/// the same `run_id` replaces it in place rather than erroring or duplicating.
#[test]
fn delete_grid_chat_run_also_drops_run_meta() {
    let db = db();
    db.upsert_grid_run_meta("run-x", "sys", None).unwrap();
    assert!(db.grid_run_system_prompt("run-x").unwrap().is_some());
    db.delete_grid_chat_run("run-x").unwrap();
    assert!(db.grid_run_system_prompt("run-x").unwrap().is_none());
}

#[test]
fn grid_run_meta_stores_and_reads_json_schema() {
    let db = db();
    db.upsert_grid_run_meta("run-s", "sys", Some("{\"mode\":\"array\"}")).unwrap();
    let m = db.grid_run_meta("run-s").unwrap().unwrap();
    assert_eq!(m.system_prompt, "sys");
    assert_eq!(m.json_schema.as_deref(), Some("{\"mode\":\"array\"}"));
    // plain profile → NULL schema
    db.upsert_grid_run_meta("run-p", "sys2", None).unwrap();
    assert!(db.grid_run_meta("run-p").unwrap().unwrap().json_schema.is_none());
}

#[test]
fn grid_run_meta_upsert_replaces_in_place() {
    let db = db();
    db.upsert_grid_run_meta("run-meta", "You are a helpful assistant.", None).unwrap();
    assert_eq!(
        db.grid_run_system_prompt("run-meta").unwrap().as_deref(),
        Some("You are a helpful assistant.")
    );

    db.upsert_grid_run_meta("run-meta", "Updated prompt.", Some("{\"mode\":\"array\"}")).unwrap();
    assert_eq!(
        db.grid_run_system_prompt("run-meta").unwrap().as_deref(),
        Some("Updated prompt.")
    );
}

/// Upload rows (`RowRefType::GridRow`, schema_v42) carry their source text
/// alongside the chat result since there's no `chunks` row to reconstruct it
/// from on history load; ordinary chunk-backed rows keep it NULL.
#[test]
fn grid_chat_result_round_trips_row_source_text() {
    let db = db();
    let run = "run-upload";
    let upsert = GridChatUpsert {
        run_id: run.into(),
        row_uid: "upload:-1".into(),
        prompt_snapshot: None,
        row_ref_type: RowRefType::GridRow,
        row_ref_id: -1,
        prompt: "Summarize this row".into(),
        columns_context: None,
        retrieved_refs: None,
        response: Some("done".into()),
        status: ChatStatus::Done,
        error: None,
        row_source_text: Some("Alice, 34, Berlin".into()),
    };
    let result = db.upsert_grid_chat_result(&upsert).unwrap();
    assert_eq!(result.row_source_text.as_deref(), Some("Alice, 34, Berlin"));

    let listed = db.list_grid_chat_results(run).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].row_source_text.as_deref(), Some("Alice, 34, Berlin"));
    assert_eq!(listed[0].row_ref_type, RowRefType::GridRow);

    // Ordinary chunk-backed row stays NULL.
    let mut chunk_row = upsert.clone();
    chunk_row.row_uid = "chunk:5".into();
    chunk_row.row_ref_type = RowRefType::Chunk;
    chunk_row.row_ref_id = 5;
    chunk_row.row_source_text = None;
    let chunk_result = db.upsert_grid_chat_result(&chunk_row).unwrap();
    assert!(chunk_result.row_source_text.is_none());
}

#[test]
fn grid_run_system_prompt_missing_run_returns_none() {
    let db = db();
    assert_eq!(db.grid_run_system_prompt("no-such-run").unwrap(), None);
}

#[test]
fn settings_round_trip_any_serde_type() {
    let db = db();
    db.set_setting("max_parallel_chats", &16i64).unwrap();
    let n: i64 = db.get_setting("max_parallel_chats").unwrap().unwrap();
    assert_eq!(n, 16);
    assert!(db.get_setting::<String>("missing").unwrap().is_none());
}

// --- llm_endpoint_pools -----------------------------------------------------

fn seed_endpoint(db: &Database, name: &str, provider: &str) -> i64 {
    db.create_llm_endpoint(&NewLlmEndpoint {
        name: name.into(),
        base_url: "http://localhost:11434".into(),
        model_id: "test-model".into(),
        api_key_ref: None,
        timeout_ms: 30_000,
        max_retries: 1,
        provider: provider.into(),
        window_tokens: 1500,
        context_window: 8192,
        output_reserve_tokens: 512,
        tpm_limit: None,
        rpm_limit: None,
        max_concurrency: 2,
        is_reasoning: false,
        supports_structured_output: false,
        kv_quantization: None,
        cpu_threads: None,
    })
    .unwrap()
    .id
}

#[test]
fn create_list_delete_pool_roundtrip() {
    let db = db();
    let pool = db.create_pool(&NewLlmEndpointPool { name: "Pool A".into() }).unwrap();
    assert_eq!(db.list_pools().unwrap().len(), 1);
    assert_eq!(db.pool(pool.id).unwrap().unwrap().name, "Pool A");

    let renamed = db.rename_pool(pool.id, "Pool A renamed").unwrap();
    assert_eq!(renamed.name, "Pool A renamed");

    assert!(db.delete_pool(pool.id).unwrap());
    assert!(db.pool(pool.id).unwrap().is_none());
}

#[test]
fn set_pool_members_rejects_two_gguf_endpoints() {
    let db = db();
    let pool = db.create_pool(&NewLlmEndpointPool { name: "Pool".into() }).unwrap();
    let gguf_a = seed_endpoint(&db, "local-a", "gguf");
    let gguf_b = seed_endpoint(&db, "local-b", "gguf");

    let err = db.set_pool_members(pool.id, &[gguf_a, gguf_b]).unwrap_err();
    assert!(matches!(err, CoreError::InvalidPoolMembers(_)));
    // rejected call must not have written anything
    assert!(db.list_pool_members(pool.id).unwrap().is_empty());
}

#[test]
fn set_pool_members_allows_one_gguf_plus_remote_endpoints() {
    let db = db();
    let pool = db.create_pool(&NewLlmEndpointPool { name: "Pool".into() }).unwrap();
    let gguf = seed_endpoint(&db, "local", "gguf");
    let remote_a = seed_endpoint(&db, "remote-a", "ollama");
    let remote_b = seed_endpoint(&db, "remote-b", "openai");

    let members = db.set_pool_members(pool.id, &[gguf, remote_a, remote_b]).unwrap();
    assert_eq!(members.len(), 3);
    assert_eq!(db.list_pool_members(pool.id).unwrap().len(), 3);
}

#[test]
fn set_pool_members_replaces_full_list_atomically() {
    let db = db();
    let pool = db.create_pool(&NewLlmEndpointPool { name: "Pool".into() }).unwrap();
    let a = seed_endpoint(&db, "a", "ollama");
    let b = seed_endpoint(&db, "b", "ollama");
    let gguf_1 = seed_endpoint(&db, "gguf-1", "gguf");
    let gguf_2 = seed_endpoint(&db, "gguf-2", "gguf");

    db.set_pool_members(pool.id, &[a, b]).unwrap();
    assert_eq!(db.list_pool_members(pool.id).unwrap().len(), 2);

    // second call violates the gguf constraint -> must leave first list intact
    let err = db.set_pool_members(pool.id, &[gguf_1, gguf_2]).unwrap_err();
    assert!(matches!(err, CoreError::InvalidPoolMembers(_)));
    let members = db.list_pool_members(pool.id).unwrap();
    assert_eq!(members.iter().map(|e| e.id).collect::<Vec<_>>(), vec![a, b]);
}

#[test]
fn set_pool_members_preserves_order_via_position() {
    let db = db();
    let pool = db.create_pool(&NewLlmEndpointPool { name: "Pool".into() }).unwrap();
    let a = seed_endpoint(&db, "a", "ollama");
    let b = seed_endpoint(&db, "b", "ollama");
    let c = seed_endpoint(&db, "c", "ollama");

    db.set_pool_members(pool.id, &[c, a, b]).unwrap();
    let members = db.list_pool_members(pool.id).unwrap();
    assert_eq!(members.iter().map(|e| e.id).collect::<Vec<_>>(), vec![c, a, b]);
}

#[test]
fn delete_pool_cascades_to_members() {
    let db = db();
    let pool = db.create_pool(&NewLlmEndpointPool { name: "Pool".into() }).unwrap();
    let a = seed_endpoint(&db, "a", "ollama");
    db.set_pool_members(pool.id, &[a]).unwrap();

    db.delete_pool(pool.id).unwrap();
    // pool is gone, so listing its members returns empty rather than erroring
    assert!(db.list_pool_members(pool.id).unwrap().is_empty());
}

#[test]
fn delete_llm_endpoint_cascades_out_of_pools() {
    let db = db();
    let pool = db.create_pool(&NewLlmEndpointPool { name: "Pool".into() }).unwrap();
    let a = seed_endpoint(&db, "a", "ollama");
    let b = seed_endpoint(&db, "b", "ollama");
    db.set_pool_members(pool.id, &[a, b]).unwrap();

    db.delete_llm_endpoint(a).unwrap();
    let members = db.list_pool_members(pool.id).unwrap();
    assert_eq!(members.iter().map(|e| e.id).collect::<Vec<_>>(), vec![b]);
}
