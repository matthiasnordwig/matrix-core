//! `llm_endpoints` CRUD (capability flags `stream_fallback`, `tools_advanced`,
//! `supports_vision` round-trips + defaults), endpoint pools (gguf-per-pool
//! rule, ordering, cascades) and reasoning-effort lists (FK SET NULL). All
//! against an in-memory database.

use super::models::*;
use super::{CoreError, Database};

fn db() -> Database {
    Database::open_in_memory().expect("open in-memory db")
}

// --- llm_endpoint_pools -----------------------------------------------------

pub(super) fn seed_endpoint(db: &Database, name: &str, provider: &str) -> i64 {
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
        supports_tools: false,
        supports_vision: false,
        tools_advanced: false,
        stream_fallback: true,
        kv_quantization: None,
        cpu_threads: None,
        reasoning_list_id: None,
    })
    .unwrap()
    .id
}

#[test]
fn llm_endpoint_stream_fallback_roundtrip() {
    let db = db();
    // create → default (seeded as true here) is persisted and read back
    let id = seed_endpoint(&db, "ep", "openai");
    let created = db.llm_endpoint(id).unwrap().unwrap();
    assert!(created.stream_fallback);

    // update → toggle off round-trips
    let mut upd: NewLlmEndpoint = NewLlmEndpoint {
        name: created.name.clone(),
        base_url: created.base_url.clone(),
        model_id: created.model_id.clone(),
        api_key_ref: created.api_key_ref.clone(),
        timeout_ms: created.timeout_ms,
        max_retries: created.max_retries,
        provider: created.provider.clone(),
        window_tokens: created.window_tokens,
        context_window: created.context_window,
        output_reserve_tokens: created.output_reserve_tokens,
        tpm_limit: created.tpm_limit,
        rpm_limit: created.rpm_limit,
        max_concurrency: created.max_concurrency,
        is_reasoning: created.is_reasoning,
        supports_structured_output: created.supports_structured_output,
        supports_tools: created.supports_tools,
        supports_vision: created.supports_vision,
        tools_advanced: created.tools_advanced,
        stream_fallback: false,
        kv_quantization: created.kv_quantization.clone(),
        cpu_threads: created.cpu_threads,
        reasoning_list_id: created.reasoning_list_id,
    };
    let updated = db.update_llm_endpoint(id, &upd).unwrap();
    assert!(!updated.stream_fallback);
    assert!(!db.llm_endpoint(id).unwrap().unwrap().stream_fallback);

    // toggle back on
    upd.stream_fallback = true;
    let re = db.update_llm_endpoint(id, &upd).unwrap();
    assert!(re.stream_fallback);
}

#[test]
fn llm_endpoint_tools_advanced_roundtrip_and_default() {
    let db = db();

    // create with tools_advanced=true persists and reads back true.
    let created = db
        .create_llm_endpoint(&NewLlmEndpoint {
            name: "advanced-ep".into(),
            base_url: "http://localhost:11434".into(),
            model_id: "test-model".into(),
            api_key_ref: None,
            timeout_ms: 30_000,
            max_retries: 1,
            provider: "openai".into(),
            window_tokens: 1500,
            context_window: 8192,
            output_reserve_tokens: 512,
            tpm_limit: None,
            rpm_limit: None,
            max_concurrency: 2,
            is_reasoning: false,
            supports_structured_output: false,
            supports_tools: true,
            supports_vision: false,
            tools_advanced: true,
            stream_fallback: true,
            kv_quantization: None,
            cpu_threads: None,
            reasoning_list_id: None,
        })
        .unwrap();
    assert!(created.tools_advanced);
    assert!(db.llm_endpoint(created.id).unwrap().unwrap().tools_advanced);

    // update → toggle off round-trips.
    let mut upd = NewLlmEndpoint {
        name: created.name.clone(),
        base_url: created.base_url.clone(),
        model_id: created.model_id.clone(),
        api_key_ref: created.api_key_ref.clone(),
        timeout_ms: created.timeout_ms,
        max_retries: created.max_retries,
        provider: created.provider.clone(),
        window_tokens: created.window_tokens,
        context_window: created.context_window,
        output_reserve_tokens: created.output_reserve_tokens,
        tpm_limit: created.tpm_limit,
        rpm_limit: created.rpm_limit,
        max_concurrency: created.max_concurrency,
        is_reasoning: created.is_reasoning,
        supports_structured_output: created.supports_structured_output,
        supports_tools: created.supports_tools,
        supports_vision: created.supports_vision,
        tools_advanced: false,
        stream_fallback: created.stream_fallback,
        kv_quantization: created.kv_quantization.clone(),
        cpu_threads: created.cpu_threads,
        reasoning_list_id: created.reasoning_list_id,
    };
    let updated = db.update_llm_endpoint(created.id, &upd).unwrap();
    assert!(!updated.tools_advanced);

    upd.tools_advanced = true;
    let re = db.update_llm_endpoint(created.id, &upd).unwrap();
    assert!(re.tools_advanced);

    // Default false for a row that predates the column: simulate the ALTER
    // TABLE ... DEFAULT 0 migration by inserting a row directly via SQL
    // without specifying `tools_advanced`, mirroring what happens to an
    // existing `llm_endpoints` row when schema_v55 runs.
    db.conn
        .execute(
            "INSERT INTO llm_endpoints
                (name, base_url, model_id, timeout_ms, max_retries, provider,
                 window_tokens, context_window, output_reserve_tokens, max_concurrency,
                 is_reasoning, supports_structured_output, supports_tools, stream_fallback)
             VALUES ('legacy-ep', 'http://localhost:11434', 'legacy-model', 30000, 1, 'openai',
                     1500, 8192, 512, 2, 0, 0, 1, 1)",
            [],
        )
        .unwrap();
    let legacy_id = db.conn.last_insert_rowid();
    let legacy = db.llm_endpoint(legacy_id).unwrap().unwrap();
    assert!(!legacy.tools_advanced);
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

// --- reasoning_effort_lists -------------------------------------------------

#[test]
fn reasoning_effort_list_crud_roundtrip() {
    let db = db();
    let created = db
        .create_reasoning_effort_list(&NewReasoningEffortList {
            title: "OpenAI GPT-5".into(),
            description: Some("gpt-5 family".into()),
            allowed_efforts: vec!["minimal".into(), "low".into(), "medium".into(), "high".into()],
        })
        .unwrap();
    assert_eq!(created.allowed_efforts.len(), 4);
    // JSON round-trips through TEXT storage
    let fetched = db.reasoning_effort_list(created.id).unwrap().unwrap();
    assert_eq!(fetched.allowed_efforts, created.allowed_efforts);
    assert_eq!(db.list_reasoning_effort_lists().unwrap().len(), 1);

    let updated = db
        .update_reasoning_effort_list(
            created.id,
            &NewReasoningEffortList {
                title: "OpenAI o-series".into(),
                description: None,
                allowed_efforts: vec!["low".into(), "medium".into(), "high".into()],
            },
        )
        .unwrap();
    assert_eq!(updated.title, "OpenAI o-series");
    assert_eq!(updated.allowed_efforts, vec!["low", "medium", "high"]);
    assert!(!updated.allowed_efforts.contains(&"minimal".to_string()));

    assert!(db.delete_reasoning_effort_list(created.id).unwrap());
    assert!(db.reasoning_effort_list(created.id).unwrap().is_none());
}

#[test]
fn deleting_a_reasoning_list_nulls_endpoint_fk() {
    let db = db();
    let list = db
        .create_reasoning_effort_list(&NewReasoningEffortList {
            title: "L".into(),
            description: None,
            allowed_efforts: vec!["low".into()],
        })
        .unwrap();
    let ep = db
        .create_llm_endpoint(&NewLlmEndpoint {
            name: "ep".into(),
            base_url: "http://localhost:11434".into(),
            model_id: "m".into(),
            api_key_ref: None,
            timeout_ms: 30_000,
            max_retries: 1,
            provider: "openai".into(),
            window_tokens: 1500,
            context_window: 8192,
            output_reserve_tokens: 512,
            tpm_limit: None,
            rpm_limit: None,
            max_concurrency: 2,
            is_reasoning: true,
            supports_structured_output: false,
            supports_tools: false,
            supports_vision: false,
            tools_advanced: false,
            stream_fallback: true,
            kv_quantization: None,
            cpu_threads: None,
            reasoning_list_id: Some(list.id),
        })
        .unwrap();
    assert_eq!(ep.reasoning_list_id, Some(list.id));

    // ON DELETE SET NULL: deleting the list unassigns it, endpoint survives.
    assert!(db.delete_reasoning_effort_list(list.id).unwrap());
    let after = db.llm_endpoint(ep.id).unwrap().unwrap();
    assert_eq!(after.reasoning_list_id, None);
}
