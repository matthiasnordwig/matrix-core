//! Tests of API-key sealing in the registries (`secret_fields.rs`): no
//! plaintext key in the stored rows after any write, plaintext on read, the
//! idempotent start-up conversion, and a missing/wrong key leaving the raw
//! `enc:` value in place.

use super::super::models::*;
use super::super::Database;
use super::*;
use crate::secrets::{self, SecretKey};

const LLM_KEY: &str = "sk-llm-PLAINTEXT-1";
const EMB_KEY: &str = "sk-emb-PLAINTEXT-2";
const RR_KEY: &str = "sk-rr-PLAINTEXT-3";

fn endpoint(key: Option<&str>) -> NewLlmEndpoint {
    NewLlmEndpoint {
        name: "Gemini".into(),
        base_url: "https://example.invalid/v1".into(),
        model_id: "m".into(),
        api_key_ref: key.map(Into::into),
        timeout_ms: 1,
        max_retries: 0,
        provider: "openai".into(),
        window_tokens: 1,
        context_window: 1,
        output_reserve_tokens: 1,
        tpm_limit: None,
        rpm_limit: None,
        max_concurrency: 1,
        is_reasoning: false,
        supports_structured_output: false,
        supports_tools: false,
        supports_vision: false,
        tools_advanced: false,
        stream_fallback: true,
        kv_quantization: None,
        cpu_threads: None,
        reasoning_list_id: None,
    }
}

fn embed_model(cfg: Option<String>) -> NewEmbeddingModel {
    NewEmbeddingModel {
        identifier: "remote-emb".into(),
        kind: ModelKind::RemoteApi,
        model_path: None,
        tokenizer_path: None,
        api_config: cfg,
        execution_provider: None,
        is_matryoshka: false,
        native_dim: 4,
        default_dim: 4,
        normalize: true,
        tpm_limit: None,
        rpm_limit: None,
        max_concurrency: 1,
        query_prefix: None,
        document_prefix: None,
    }
}

fn reranker(cfg: Option<String>) -> NewRerankerModel {
    NewRerankerModel {
        name: "remote-rr".into(),
        kind: ModelKind::RemoteApi,
        model_dir: None,
        api_config: cfg,
        execution_provider: None,
    }
}

fn emb_cfg(key: &str) -> String {
    format!(r#"{{"base_url":"https://e.invalid","api_key":"{key}","model":"x"}}"#)
}
fn rr_cfg(key: &str) -> String {
    format!(r#"{{"base_url":"https://r.invalid","key_ref":"{key}","model":"y"}}"#)
}

/// Every stored value of the three secret columns, raw from SQL.
fn raw_secret_columns(db: &Database) -> Vec<String> {
    let mut out = Vec::new();
    for sql in [
        "SELECT api_key_ref FROM llm_endpoints WHERE api_key_ref IS NOT NULL",
        "SELECT api_config FROM embedding_models WHERE api_config IS NOT NULL",
        "SELECT api_config FROM reranker_models WHERE api_config IS NOT NULL",
    ] {
        let mut stmt = db.conn.prepare(sql).unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
        out.extend(rows.map(|r| r.unwrap()));
    }
    out
}

fn assert_no_plaintext(db: &Database) {
    for v in raw_secret_columns(db) {
        for k in [LLM_KEY, EMB_KEY, RR_KEY] {
            assert!(!v.contains(k), "plaintext key in stored value {v}");
        }
    }
}

fn keyed_db() -> Database {
    let mut db = Database::open_in_memory().unwrap();
    db.set_secret_key(Some(SecretKey::generate()));
    db
}

/// Rows of all three kinds; returns (endpoint id, embedding id, reranker id).
fn fill(db: &Database) -> (i64, i64, i64) {
    let e = db.create_llm_endpoint(&endpoint(Some(LLM_KEY))).unwrap();
    let m = db.create_embedding_model(&embed_model(Some(emb_cfg(EMB_KEY)))).unwrap();
    let r = db.create_reranker_model(&reranker(Some(rr_cfg(RR_KEY)))).unwrap();
    (e.id, m.id, r.id)
}

fn json_field(cfg: &Option<String>, field: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(cfg.as_deref().unwrap()).unwrap();
    v[field].as_str().unwrap().to_string()
}

#[test]
fn writes_store_no_plaintext_and_reads_return_plaintext() {
    let db = keyed_db();
    let (e, m, r) = fill(&db);
    assert_no_plaintext(&db);

    // Create returns (and every reader yields) plaintext.
    assert_eq!(db.llm_endpoint(e).unwrap().unwrap().api_key_ref.as_deref(), Some(LLM_KEY));
    assert_eq!(db.list_llm_endpoints().unwrap()[0].api_key_ref.as_deref(), Some(LLM_KEY));
    assert_eq!(json_field(&db.embedding_model(m).unwrap().unwrap().api_config, "api_key"), EMB_KEY);
    assert_eq!(json_field(&db.list_embedding_models().unwrap()[0].api_config, "api_key"), EMB_KEY);
    assert_eq!(json_field(&db.reranker_model(r).unwrap().unwrap().api_config, "key_ref"), RR_KEY);
    assert_eq!(json_field(&db.list_reranker_models().unwrap()[0].api_config, "key_ref"), RR_KEY);

    // Pool members are read through the same path.
    let pool = db.create_pool(&NewLlmEndpointPool { name: "p".into() }).unwrap();
    db.set_pool_members(pool.id, &[e]).unwrap();
    assert_eq!(db.list_pool_members(pool.id).unwrap()[0].api_key_ref.as_deref(), Some(LLM_KEY));

    // The rest of the JSON stays readable.
    let raw = raw_secret_columns(&db);
    assert!(raw.iter().any(|v| v.contains("https://e.invalid") && v.contains("\"api_key\":\"enc:v1:")));
    assert!(raw.iter().any(|v| v.contains("https://r.invalid") && v.contains("\"key_ref\":\"enc:v1:")));

    // Updates seal too; re-saving a read row does not double-encrypt.
    let ep = db.llm_endpoint(e).unwrap().unwrap();
    let mut upd = endpoint(ep.api_key_ref.as_deref());
    upd.name = "Gemini 2".into();
    db.update_llm_endpoint(e, &upd).unwrap();
    db.update_embedding_model(m, &embed_model(Some(emb_cfg(EMB_KEY)))).unwrap();
    db.update_reranker_model(r, &reranker(Some(rr_cfg(RR_KEY)))).unwrap();
    assert_no_plaintext(&db);
    assert_eq!(db.llm_endpoint(e).unwrap().unwrap().api_key_ref.as_deref(), Some(LLM_KEY));
    assert_eq!(json_field(&db.embedding_model(m).unwrap().unwrap().api_config, "api_key"), EMB_KEY);
    assert_eq!(json_field(&db.reranker_model(r).unwrap().unwrap().api_config, "key_ref"), RR_KEY);
}

#[test]
fn already_encrypted_value_is_stored_unchanged() {
    let db = keyed_db();
    let foreign = secrets::encrypt(&SecretKey::generate(), AAD_LLM_KEY, "x");
    let e = db.create_llm_endpoint(&endpoint(Some(&foreign))).unwrap();
    let raw: String = db
        .conn
        .query_row("SELECT api_key_ref FROM llm_endpoints WHERE id = ?1", [e.id], |r| r.get(0))
        .unwrap();
    assert_eq!(raw, foreign);
}

#[test]
fn without_key_values_stay_plaintext() {
    let db = Database::open_in_memory().unwrap();
    let (e, m, _) = fill(&db);
    assert!(raw_secret_columns(&db).iter().any(|v| v == LLM_KEY));
    assert_eq!(db.llm_endpoint(e).unwrap().unwrap().api_key_ref.as_deref(), Some(LLM_KEY));
    assert_eq!(db.embedding_model(m).unwrap().unwrap().api_config, Some(emb_cfg(EMB_KEY)));
    assert_eq!(db.encrypt_plaintext_secrets().unwrap(), 0);
}

#[test]
fn startup_conversion_is_idempotent() {
    let mut db = Database::open_in_memory().unwrap();
    let (e, m, r) = fill(&db);
    // Empty/absent keys and JSON without the field are left alone.
    db.create_llm_endpoint(&NewLlmEndpoint { name: "empty".into(), ..endpoint(Some("")) }).unwrap();
    db.create_llm_endpoint(&NewLlmEndpoint { name: "none".into(), ..endpoint(None) }).unwrap();
    db.create_embedding_model(&NewEmbeddingModel {
        identifier: "no-key".into(),
        ..embed_model(Some(r#"{"base_url":"https://n.invalid"}"#.into()))
    })
    .unwrap();

    db.set_secret_key(Some(SecretKey::generate()));
    assert_eq!(db.encrypt_plaintext_secrets().unwrap(), 3);
    assert_no_plaintext(&db);
    let after_first = raw_secret_columns(&db);
    assert_eq!(db.encrypt_plaintext_secrets().unwrap(), 0);
    assert_eq!(raw_secret_columns(&db), after_first);
    assert!(after_first.iter().any(|v| v == r#"{"base_url":"https://n.invalid"}"#));

    assert_eq!(db.llm_endpoint(e).unwrap().unwrap().api_key_ref.as_deref(), Some(LLM_KEY));
    assert_eq!(json_field(&db.embedding_model(m).unwrap().unwrap().api_config, "api_key"), EMB_KEY);
    assert_eq!(json_field(&db.reranker_model(r).unwrap().unwrap().api_config, "key_ref"), RR_KEY);
}

#[test]
fn missing_or_wrong_key_leaves_raw_enc_value() {
    let mut db = keyed_db();
    let (e, m, r) = fill(&db);
    for key in [None, Some(SecretKey::generate())] {
        db.set_secret_key(key);
        let ep = db.llm_endpoint(e).unwrap().unwrap();
        assert!(secrets::is_encrypted(ep.api_key_ref.as_deref().unwrap()));
        let emb = db.embedding_model(m).unwrap().unwrap();
        assert!(secrets::is_encrypted(&json_field(&emb.api_config, "api_key")));
        assert!(json_field(&emb.api_config, "base_url").starts_with("https://"));
        let rr = db.reranker_model(r).unwrap().unwrap();
        assert!(secrets::is_encrypted(&json_field(&rr.api_config, "key_ref")));
    }
}

#[test]
fn value_moved_to_another_field_does_not_decrypt() {
    let db = keyed_db();
    let (e, _, _) = fill(&db);
    let sealed: String = db
        .conn
        .query_row("SELECT api_key_ref FROM llm_endpoints WHERE id = ?1", [e], |r| r.get(0))
        .unwrap();
    // Copy the sealed LLM key into an embedding config's `api_key`.
    db.conn
        .execute(
            "INSERT INTO embedding_models (identifier, kind, api_config, native_dim, default_dim)
             VALUES ('moved', 'remote_api', ?1, 4, 4)",
            [format!(r#"{{"base_url":"https://m.invalid","api_key":"{sealed}"}}"#)],
        )
        .unwrap();
    let moved = db.list_embedding_models().unwrap().into_iter().find(|m| m.identifier == "moved").unwrap();
    assert_eq!(json_field(&moved.api_config, "api_key"), sealed);
}

#[test]
fn enable_secrets_creates_key_file_and_converts() {
    let dir = std::env::temp_dir().join(format!("matrix-secret-fields-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut db = Database::open(dir.join("matrix.db")).unwrap();
    fill(&db);
    assert_eq!(db.enable_secrets(&dir).unwrap(), 3);
    assert!(db.has_secret_key());
    assert!(secrets::key_path(&dir).exists());
    assert_no_plaintext(&db);
    drop(db);

    // Reopened with the same key file: plaintext again, nothing left to convert.
    let mut db = Database::open(dir.join("matrix.db")).unwrap();
    assert_eq!(db.enable_secrets(&dir).unwrap(), 0);
    assert_eq!(db.list_llm_endpoints().unwrap()[0].api_key_ref.as_deref(), Some(LLM_KEY));
    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}
