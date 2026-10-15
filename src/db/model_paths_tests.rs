//! The narrow path setters used when the app's data folder moved
//! (`set_embedding_model_file_paths`, `set_llm_endpoint_base_url`): they
//! rewrite only the path fields and leave every other column as it was.

use crate::db::models::*;
use crate::Database;

#[test]
fn embedding_model_file_paths_are_rewritten_alone() {
    let db = Database::open_in_memory().unwrap();
    let m = db
        .create_embedding_model(&NewEmbeddingModel {
            identifier: "embeddinggemma-2".into(),
            kind: ModelKind::LocalOnnx,
            model_path: Some("/old/models/e/model.onnx".into()),
            tokenizer_path: Some("/old/models/e/tokenizer.json".into()),
            api_config: None,
            execution_provider: Some(ExecutionProvider::Cpu),
            is_matryoshka: true,
            native_dim: 768,
            default_dim: 768,
            normalize: false,
            tpm_limit: None,
            rpm_limit: None,
            max_concurrency: 1,
            query_prefix: Some("q: ".into()),
            document_prefix: Some("d: ".into()),
        })
        .unwrap()
        .id;
    db.set_embedding_model_vision_path(m, Some("/old/models/e/v.onnx")).unwrap();

    db.set_embedding_model_file_paths(
        m,
        Some("/new/models/e/model.onnx"),
        Some("/new/models/e/tokenizer.json"),
        Some("/new/models/e/v.onnx"),
    )
    .unwrap();

    let got = db.embedding_model(m).unwrap().unwrap();
    assert_eq!(got.model_path.as_deref(), Some("/new/models/e/model.onnx"));
    assert_eq!(got.tokenizer_path.as_deref(), Some("/new/models/e/tokenizer.json"));
    assert_eq!(got.vision_model_path.as_deref(), Some("/new/models/e/v.onnx"));
    assert_eq!(got.identifier, "embeddinggemma-2");
    assert_eq!(got.query_prefix.as_deref(), Some("q: "));
    assert!(got.is_matryoshka);
}

#[test]
fn llm_endpoint_base_url_is_rewritten_alone() {
    let db = Database::open_in_memory().unwrap();
    let ep = db
        .create_llm_endpoint(&NewLlmEndpoint {
            name: "Qwen".into(),
            base_url: "/old/models/q.gguf".into(),
            model_id: "q.gguf".into(),
            api_key_ref: None,
            timeout_ms: 120_000,
            max_retries: 0,
            provider: "gguf".into(),
            window_tokens: 4096,
            context_window: 8192,
            output_reserve_tokens: 1024,
            tpm_limit: None,
            rpm_limit: None,
            max_concurrency: 1,
            is_reasoning: false,
            supports_structured_output: true,
            supports_tools: false,
            supports_vision: false,
            tools_advanced: false,
            stream_fallback: true,
            kv_quantization: None,
            cpu_threads: None,
            reasoning_list_id: None,
        })
        .unwrap();

    db.set_llm_endpoint_base_url(ep.id, "/new/models/q.gguf").unwrap();

    let got = db.list_llm_endpoints().unwrap().into_iter().find(|e| e.id == ep.id).unwrap();
    assert_eq!(got.base_url, "/new/models/q.gguf");
    assert_eq!(got.name, "Qwen");
    assert_eq!(got.context_window, 8192);
}
