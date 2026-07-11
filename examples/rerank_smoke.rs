//! Host smoke test for the local ONNX cross-encoder reranker (AP3). Proves the
//! jina-reranker-v2 ONNX model actually runs and produces one relevance logit
//! per (query, doc) pair. Run with:
//!   cargo run --example rerank_smoke --features onnx-download [-- <model_dir>]
//! Defaults `<model_dir>` to `~/matrix/models/jina-reranker-v2`.

#[cfg(feature = "onnx")]
fn main() {
    use app::embedding::rerank::{rank_merge, OrtReranker};
    use std::path::PathBuf;

    let dir = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| {
        let home = std::env::var_os("HOME").expect("HOME set");
        PathBuf::from(home).join("matrix").join("models").join("jina-reranker-v2")
    });
    eprintln!("loading reranker from {}", dir.display());

    let reranker = OrtReranker::load(&dir).expect("load reranker ONNX session");

    let query = "Welche Anforderungen gelten für das Auslagerungsmanagement?";
    let docs = [
        "AT 9 der MaRisk regelt die Anforderungen an Auslagerungen und das Auslagerungsmanagement der Institute.",
        "Das Rezept beschreibt, wie man einen Hefeteig für Pizza zubereitet.",
        "Bei wesentlichen Auslagerungen ist ein Auslagerungsvertrag mit klaren Regelungen erforderlich.",
    ];

    let scores = reranker.score_pairs(query, &docs).expect("score pairs");
    assert_eq!(scores.len(), docs.len(), "one logit per doc");
    println!("query: {query}");
    for (i, (d, s)) in docs.iter().zip(&scores).enumerate() {
        let snippet: String = d.chars().take(60).collect();
        println!("  logit[{i}] = {s:+.4}  {snippet}…");
    }
    let ranked = rank_merge(&scores, docs.len());
    println!("ranked order (best→worst): {ranked:?}");
    // Both norm-relevant docs (0, 2) should outrank the off-topic recipe (1).
    assert!(
        ranked.iter().position(|&i| i == 1) == Some(docs.len() - 1),
        "off-topic doc should rank last; got {ranked:?}"
    );
    println!("OK — local ONNX reranker runs and separates relevant from off-topic.");
}

#[cfg(not(feature = "onnx"))]
fn main() {
    eprintln!("rebuild with --features onnx-download");
}
