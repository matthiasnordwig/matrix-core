//! Graphics unit against CPU for the local ONNX search model (`embedding/onnx_ep.rs`): passages per
//! second when ingesting, query latency, image embeddings per second, and how close the vectors of
//! each way are to the first one's (cosine per passage/query/image). Uses the app's own
//! `OrtEmbedder`/`OrtImageEmbedder`. Ways: `cpu`, `gpu`, and `gpu-fixed` (input padded to `FIXED`
//! tokens with a fixed shape, `OrtEmbedder::load_padded`; longer passages are skipped and counted).
//! Run (Windows: DirectML, Apple: CoreML):
//!   MODEL_DIR=<embeddinggemma-2 folder> TEXT_DIR=<folder of .md/.txt> [N=200] [CHARS=1200] [FIXED=512]
//!   [IMAGE_DIR=<folder of .png/.jpg> VISION=vision_encoder_q4.onnx [VISION_UNIT=cpu|gpu]] [UNITS=cpu,gpu,gpu-fixed]
//!   cargo run --release --example onnx_unit_bench --features onnx-download

#[cfg(feature = "onnx")]
fn main() {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Instant;

    use app::compute::Unit;
    use app::embedding::onnx::OrtEmbedder;
    use app::embedding::QueryEmbedder;
    use app::embedding::vision::OrtImageEmbedder;
    use app::models::{EmbeddingModel, ModelKind};

    let env = |k: &str| std::env::var(k).ok();
    let dir = PathBuf::from(env("MODEL_DIR").expect("set MODEL_DIR"));
    let n: usize = env("N").and_then(|v| v.parse().ok()).unwrap_or(200);
    let chars: usize = env("CHARS").and_then(|v| v.parse().ok()).unwrap_or(1200);
    let fixed: usize = env("FIXED").and_then(|v| v.parse().ok()).unwrap_or(512);
    // (unit, fixed input length)
    let ways: Vec<(Unit, Option<usize>)> = env("UNITS")
        .unwrap_or_else(|| "cpu,gpu".into())
        .split(',')
        .map(|u| match u.trim() {
            "gpu" => (Unit::Gpu, None),
            "gpu-fixed" => (Unit::Gpu, Some(fixed)),
            "cpu-fixed" => (Unit::Cpu, Some(fixed)),
            _ => (Unit::Cpu, None),
        })
        .collect();

    // Passages: paragraphs of the text files, joined up to `chars` (never cut inside a paragraph).
    let mut files: Vec<PathBuf> = std::fs::read_dir(env("TEXT_DIR").expect("set TEXT_DIR"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("md" | "txt")))
        .collect();
    files.sort();
    let mut passages = Vec::new();
    'files: for f in &files {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        let mut cur = String::new();
        for para in text.split('\n') {
            if !cur.is_empty() && cur.len() + para.len() > chars {
                passages.push(format!("title: none | text: {}", std::mem::take(&mut cur)));
                if passages.len() == n {
                    break 'files;
                }
            }
            cur.push_str(para);
            cur.push('\n');
        }
    }
    let avg = passages.iter().map(|p| p.chars().count()).sum::<usize>() / passages.len().max(1);
    println!("{} passages, avg {avg} chars", passages.len());
    let queries: Vec<String> = passages.iter().map(|p| format!("task: search result | query: {}", p.chars().skip(20).take(60).collect::<String>())).collect();

    let images: Vec<Vec<u8>> = env("IMAGE_DIR")
        .map(|d| {
            let mut v: Vec<PathBuf> = std::fs::read_dir(d).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("png" | "jpg" | "jpeg"))).collect();
            v.sort();
            v.into_iter().take(n).map(|p| std::fs::read(p).unwrap()).collect()
        })
        .unwrap_or_default();

    let path = |f: &str| dir.join(f).to_string_lossy().into_owned();
    let model = EmbeddingModel {
        id: 1,
        identifier: "bench".into(),
        kind: ModelKind::LocalOnnx,
        model_path: Some(path("model.onnx")),
        tokenizer_path: Some(path("tokenizer.json")),
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
    let vision = env("VISION").map(|v| path(&v));

    // Per way: passage vectors, query vectors, image vectors (`None`: longer than the fixed input).
    type Vecs = Vec<Option<Vec<f32>>>;
    type Run = (String, Vecs, Vecs, Vec<Vec<f32>>);
    let mut runs: Vec<Run> = Vec::new();
    for (unit, pad) in ways {
        let name = match pad { Some(l) => format!("{unit:?}-fixed{l}"), None => format!("{unit:?}") };
        let t = Instant::now();
        let e = Arc::new(match pad {
            Some(l) => OrtEmbedder::load_padded(&model, unit, l),
            None => OrtEmbedder::load_on(&model, unit),
        }.expect("load"));
        let load = t.elapsed().as_secs_f64();
        let fits = |t: &str| pad.is_none_or(|l| e.token_count(t).unwrap() <= l);
        for p in passages.iter().filter(|p| fits(p)).take(3) {
            e.embed_query(&model, p).unwrap(); // warm-up, discarded
        }
        let embed_all = |texts: &[String]| -> (Vecs, f64, usize) {
            let t = Instant::now();
            let v: Vecs = texts.iter().map(|p| fits(p).then(|| e.embed_query(&model, p).unwrap())).collect();
            let done = v.iter().filter(|x| x.is_some()).count();
            (v, t.elapsed().as_secs_f64() * 1000.0 / done.max(1) as f64, done)
        };
        let (vecs, p_ms, p_done) = embed_all(&passages);
        let tokens = passages.iter().filter(|p| fits(p)).map(|p| e.token_count(p).unwrap()).sum::<usize>() / p_done.max(1);
        let (qvecs, q_ms, _) = embed_all(&queries);
        println!(
            "{name}: load {load:.1} s, {p_done}/{} passages (avg {tokens} tokens) {p_ms:.0} ms each = {:.1}/s, query {q_ms:.0} ms",
            vecs.len(),
            1000.0 / p_ms
        );
        let mut img_vecs = Vec::new();
        // The vision encoder's own unit: `VISION_UNIT=cpu|gpu` (default: the text model's).
        let vision_unit = match env("VISION_UNIT").as_deref() {
            Some("gpu") => Unit::Gpu,
            Some("cpu") => Unit::Cpu,
            _ => unit,
        };
        if let (Some(v), false, None) = (&vision, images.is_empty(), pad) {
            let ie = OrtImageEmbedder::with_text_on(e.clone(), v, vision_unit).expect("vision").with_max_soft_tokens(app::embedding::vision_prep::APP_SOFT_TOKENS);
            ie.embed_image_bytes(&images[0]).unwrap();
            let t = Instant::now();
            img_vecs = images.iter().map(|b| ie.embed_image_bytes(b).unwrap()).collect();
            let s = t.elapsed().as_secs_f64();
            println!("{name} (vision {vision_unit:?}): {} images {s:.1} s = {:.2}/s", img_vecs.len(), img_vecs.len() as f64 / s);
        }
        runs.push((name, vecs, qvecs, img_vecs));
    }
    let cos = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let report = |what: &str, ways: &str, pairs: Vec<(&Vec<f32>, &Vec<f32>)>| {
        if pairs.is_empty() {
            return;
        }
        let c: Vec<f32> = pairs.iter().map(|(x, y)| cos(x, y)).collect();
        let min = c.iter().cloned().fold(f32::MAX, f32::min);
        let mean = c.iter().sum::<f32>() / c.len() as f32;
        let below = c.iter().filter(|&&v| v < 0.999).count();
        println!("cosine {what} {ways}: min {min:.6}, mean {mean:.6}, below 0.999: {below}/{}", c.len());
    };
    fn pairs<'a>(a: &'a [Option<Vec<f32>>], b: &'a [Option<Vec<f32>>]) -> Vec<(&'a Vec<f32>, &'a Vec<f32>)> {
        a.iter().zip(b).filter_map(|(x, y)| Some((x.as_ref()?, y.as_ref()?))).collect()
    }
    if let Some((first, rest)) = runs.split_first() {
        for r in rest {
            let ways = format!("{} vs {}", first.0, r.0);
            report("passages", &ways, pairs(&first.1, &r.1));
            report("queries", &ways, pairs(&first.2, &r.2));
            if first.3.len() == r.3.len() {
                report("images", &ways, first.3.iter().zip(&r.3).collect());
            }
        }
    }
}

#[cfg(not(feature = "onnx"))]
fn main() {
    eprintln!("rebuild with --features onnx-download");
}
