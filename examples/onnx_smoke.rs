//! Host smoke test for the local ONNX embedder (CoreML EP → ANE on Apple
//! Silicon). Model-agnostic. Run with:
//!   MODEL_PATH=… TOKENIZER_PATH=… [QUERY=…] cargo run --example onnx_smoke --features onnx-download

#[cfg(feature = "onnx")]
fn main() {
    use app::embedding::onnx::OrtEmbedder;
    use app::embedding::QueryEmbedder;
    use app::models::{EmbeddingModel, ModelKind};

    let model_path = std::env::var("MODEL_PATH").expect("set MODEL_PATH");
    let tokenizer_path = std::env::var("TOKENIZER_PATH").expect("set TOKENIZER_PATH");
    let query = std::env::var("QUERY")
        .unwrap_or_else(|_| "Risiken aus Verbriefungstransaktionen sind zu berücksichtigen.".into());

    let model = EmbeddingModel {
        id: 1,
        identifier: "test".into(),
        kind: ModelKind::LocalOnnx,
        model_path: Some(model_path),
        tokenizer_path: Some(tokenizer_path),
        api_config: None,
        execution_provider: None,
        is_matryoshka: false, // no truncation → report the model's native dim
        native_dim: 0,
        default_dim: 0,
        normalize: true,
        created_at: 0,
        tpm_limit: None,
        rpm_limit: None,
        max_concurrency: 1,
    };

    let embedder = OrtEmbedder::load(&model).expect("load ONNX session");
    let v = embedder.embed_query(&model, &query).expect("embed");
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    println!("dim = {}", v.len());
    println!("first 5 = {:?}", &v[..5.min(v.len())]);
    println!("L2 norm = {norm:.4} (should be ~1.0)");
    assert!(!v.is_empty() && (norm - 1.0).abs() < 1e-3, "expected a normalized vector");
    println!("OK — local ONNX embedding works for this model.");
}

#[cfg(not(feature = "onnx"))]
fn main() {
    eprintln!("rebuild with --features onnx-download");
}
