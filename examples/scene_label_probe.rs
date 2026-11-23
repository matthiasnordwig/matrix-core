//! Measures the scene labels (`media/scene_labels.rs`) on hand-labelled pictures: embeds each picture
//! like the app (local ONNX model + vision encoder, `APP_SOFT_TOKENS`), embeds the label texts as search
//! questions, applies the rule and reports precision and recall per label. Also sweeps the margin of the
//! rule (standard deviations above the mean place similarity) to show where the rule sits.
//!   cargo run --release --example scene_label_probe --features onnx-download -- \
//!       --images <dir> --truth truth.json --model-dir <embeddinggemma-2 dir> [--vision vision_encoder_q4] [--cache c.json]
//! `truth.json`: `{"<file stem>": ["outdoors", "water", …], …}`; `-io` in a list = setting not judged.
//! Pictures not in the truth file are skipped. Exit 0 = completed, 2 = usage, 3 = runtime error.

#[cfg(feature = "onnx")]
fn main() {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    use app::embedding::onnx::OrtEmbedder;
    use app::embedding::vision::OrtImageEmbedder;
    use app::embedding::QueryEmbedder;
    use app::media::scene_labels::{self as sl, LabelVectors, Scores};
    use app::models::{EmbeddingModel, ModelKind};

    const QUERY_PREFIX: &str = "task: search result | query: ";
    let fail = |e: String| -> ! {
        eprintln!("scene_label_probe: {e}");
        std::process::exit(3)
    };
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let (Some(dir), Some(truth), Some(model_dir)) = (get("--images"), get("--truth"), get("--model-dir")) else {
        eprintln!("usage: --images <dir> --truth <json> --model-dir <dir> [--vision name] [--cache file]");
        std::process::exit(2)
    };
    let model_dir = PathBuf::from(model_dir);
    let vision = get("--vision").unwrap_or_else(|| "vision_encoder_q4".into());
    let truth: BTreeMap<String, Vec<String>> =
        serde_json::from_str(&std::fs::read_to_string(&truth).unwrap_or_else(|e| fail(e.to_string()))).unwrap_or_else(|e| fail(e.to_string()));
    let text_path = ["onnx/model.onnx", "model.onnx"].iter().map(|p| model_dir.join(p)).find(|p| p.exists()).unwrap_or_else(|| fail("model.onnx missing".into()));
    let vision_path = ["onnx", "."].iter().map(|d| model_dir.join(d).join(format!("{vision}.onnx"))).find(|p| p.exists()).unwrap_or_else(|| fail("vision encoder missing".into()));
    let model = EmbeddingModel {
        id: 1,
        identifier: "probe".into(),
        kind: ModelKind::LocalOnnx,
        model_path: Some(text_path.to_string_lossy().into_owned()),
        tokenizer_path: Some(model_dir.join("tokenizer.json").to_string_lossy().into_owned()),
        api_config: None,
        execution_provider: None,
        is_matryoshka: true,
        native_dim: 768,
        default_dim: 768,
        normalize: true,
        created_at: 0,
        tpm_limit: None,
        rpm_limit: None,
        max_concurrency: 1,
        query_prefix: None,
        document_prefix: None,
        vision_model_path: None,
    };
    let emb = OrtImageEmbedder::load(&model, &vision_path.to_string_lossy())
        .unwrap_or_else(|e| fail(e.to_string()))
        .with_max_soft_tokens(app::embedding::vision_prep::APP_SOFT_TOKENS);
    let text: &OrtEmbedder = emb.text();
    // `--texts t.json` (`{"<key>": "<text>"}`): try other texts for some labels.
    let over: BTreeMap<String, String> = get("--texts").map(|p| serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()).unwrap_or_default();
    let keyed: Vec<&str> = sl::keys().collect();
    let texts: Vec<String> = sl::prompts().iter().enumerate().map(|(i, p)| keyed.get(i).and_then(|k| over.get(*k)).cloned().unwrap_or_else(|| p.to_string())).collect();
    let vecs: Vec<Vec<f32>> = texts.iter().map(|p| text.embed_query(&model, &format!("{QUERY_PREFIX}{p}")).unwrap_or_else(|e| fail(e.to_string()))).collect();
    let lv = LabelVectors::new(vecs).unwrap_or_else(|| fail("prompt count".into()));

    let cache_path = get("--cache");
    let mut cache: BTreeMap<String, Vec<f32>> =
        cache_path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut rows: Vec<(String, Scores)> = Vec::new();
    for key in truth.keys() {
        let v = match cache.get(key) {
            Some(v) => v.clone(),
            None => {
                let f = PathBuf::from(&dir).join(format!("{key}.jpg"));
                let img = image::open(&f).unwrap_or_else(|e| fail(format!("{key}: {e}")));
                let v = emb.embed_image(&img).unwrap_or_else(|e| fail(format!("{key}: {e}")));
                cache.insert(key.clone(), v.clone());
                v
            }
        };
        rows.push((key.clone(), sl::scores(&v, &lv)));
    }
    if let Some(p) = &cache_path {
        std::fs::write(p, serde_json::to_string(&cache).unwrap()).unwrap_or_else(|e| fail(e.to_string()));
    }

    // Precision/recall of a labelling function over all judged (picture, label) pairs.
    let evaluate = |name: &str, f: &dyn Fn(&Scores) -> Vec<&'static str>, verbose: bool| {
        let mut per: BTreeMap<&str, (u32, u32, u32)> = BTreeMap::new(); // tp, fp, fn
        let mut wrong = Vec::new();
        for (key, s) in &rows {
            let t = &truth[key];
            let skip_io = t.iter().any(|x| x == "-io");
            let want: BTreeSet<&str> = t.iter().map(String::as_str).filter(|x| *x != "-io").collect();
            let got: BTreeSet<&str> = f(s).into_iter().collect();
            for k in sl::keys() {
                if skip_io && (k == "indoors" || k == "outdoors") {
                    continue;
                }
                let e = per.entry(k).or_default();
                match (want.contains(k), got.contains(k)) {
                    (true, true) => e.0 += 1,
                    (false, true) => {
                        e.1 += 1;
                        wrong.push(format!("{key}+{k}"))
                    }
                    (true, false) => {
                        e.2 += 1;
                        wrong.push(format!("{key}-{k}"))
                    }
                    _ => {}
                }
            }
        }
        let (tp, fp, fnn) = per.values().fold((0, 0, 0), |a, v| (a.0 + v.0, a.1 + v.1, a.2 + v.2));
        let p = tp as f64 / (tp + fp).max(1) as f64;
        let r = tp as f64 / (tp + fnn).max(1) as f64;
        println!("{name}: precision {p:.3} recall {r:.3} F1 {:.3}  (tp {tp}, fp {fp}, fn {fnn})", 2.0 * p * r / (p + r).max(1e-9));
        if verbose {
            for (k, (tp, fp, fnn)) in &per {
                println!("  {k:15} P {:.2} R {:.2}  (tp {tp}, fp {fp}, fn {fnn})", *tp as f64 / (tp + fp).max(1) as f64, *tp as f64 / (tp + fnn).max(1) as f64);
            }
            println!("  errors: {}", wrong.join(" "));
        }
    };
    println!("{} pictures, {} label texts", rows.len(), sl::prompts().len());
    evaluate("rule (scene_labels::labels_from, SPREADS)", &sl::labels_from, true);
    // Sweep of the rule's factor (standard deviations above the mean of the place similarities).
    for k in [1.0f32, 1.25, 1.5, 1.75, 2.0] {
        let f = move |s: &Scores| {
            let mut out = sl::labels_from(s);
            out.truncate(1);
            out.extend(sl::places_above(&s.places, k));
            out
        };
        evaluate(&format!("{k:.2} standard deviations"), &f, false);
    }
}

#[cfg(not(feature = "onnx"))]
fn main() {
    eprintln!("build with --features onnx-download");
}
