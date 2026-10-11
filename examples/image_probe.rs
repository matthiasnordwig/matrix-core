//! Image-search probe for EmbeddingGemma 2 (does a TEXT question find the
//! right GRAPHIC?). Headless, no DB: images come straight from a PDF.
//!
//! Images are either the PDF's embedded raster images (key `p<page>_<i>`, ≥
//! `--min-side` px) or, with `--render [dpi]` (feature `pdf-render`), whole
//! pages rendered as images (key `p<page>`) — vector charts only exist that way.
//! Dump (look at what a PDF contains; writes `<key>.png`):
//!   cargo run --release --example image_probe --features onnx-download,pdf-render -- \
//!       --pdf <file.pdf> --dump <dir> [--pages 1-40] [--min-side 200] [--render 100]
//! Probe (embed every image + every page's text in the page range, rank per question):
//!   cargo run --release --example image_probe --features onnx-download -- \
//!       --pdf <file.pdf> --model-dir ~/matrix/models/embeddinggemma-2-onnx \
//!       [--text model_q4] [--vision vision_encoder_q4] [--soft 280] --queries q.json [--out r.json]
//! `q.json`: `{"queries": [{"q": "…", "image": "p12_0"}, …]}` — image keys as
//! written by `--dump` (1-based page[, index on the page]).
//! Per question it reports the rank of the expected image among all images
//! (image-only), among images + page texts (mixed), and — as the text-search
//! baseline — the rank of that page's TEXT among all page texts, and `fused`:
//! page-level rank fusion (RRF, k = 10 like the hybrid search) of the image
//! list and the text list. `--cache <file.json>` stores the image vectors
//! (keyed by model + key) so a re-run skips the slow image embedding.
//! `--chunk-chars N [--image-pages 9,13,…]`: additionally a chunk-level run
//! (how an image hit is mapped to the chunks of its page; see the code).
//! Image folder instead of a PDF (key = file stem, no page texts):
//!   ... -- --images <dir> --model-dir <dir> --queries q.json
//! Exit 0 = completed, 2 = usage, 3 = runtime error.

#[cfg(feature = "onnx")]
fn main() {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;
    use std::time::Instant;

    use app::embedding::onnx::OrtEmbedder;
    use app::embedding::vision::OrtImageEmbedder;
    use app::models::{EmbeddingModel, ModelKind};
    use pdf_oxide::PdfDocument;
    use serde_json::{json, Value};

    const QUERY_PREFIX: &str = "task: search result | query: ";
    const DOC_PREFIX: &str = "title: none | text: ";

    fn fail(msg: impl std::fmt::Display) -> ! {
        eprintln!("error: {msg}");
        std::process::exit(3);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let (pdf, img_dir) = (get("--pdf"), get("--images"));
    if pdf.is_none() && img_dir.is_none() {
        eprintln!("usage: see header of examples/image_probe.rs");
        std::process::exit(2);
    }
    let doc = pdf.as_ref().map(|p| PdfDocument::open(p).unwrap_or_else(|e| fail(format!("open {p}: {e}"))));
    let pages = doc.as_ref().map_or(0, |d| d.page_count().unwrap_or_else(|e| fail(e)));
    let (from, to) = get("--pages")
        .and_then(|r| r.split_once('-').map(|(a, b)| (a.parse().unwrap_or(1), b.parse().unwrap_or(pages))))
        .unwrap_or((1, pages));
    let min_side: u32 = get("--min-side").and_then(|v| v.parse().ok()).unwrap_or(200);

    let render_dpi: Option<u32> = args
        .iter()
        .position(|a| a == "--render")
        .map(|i| args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(100));
    // Images per page (1-based page): rendered page, or raster images with
    // their index among the images on that page.
    let images_of = |page: usize| -> Vec<(String, image::DynamicImage)> {
        let Some(doc) = doc.as_ref() else { return Vec::new() };
        if let Some(dpi) = render_dpi {
            return render_page(doc, page - 1, dpi).map(|img| vec![(format!("p{page}"), img)]).unwrap_or_default();
        }
        doc.extract_images(page - 1)
            .unwrap_or_default()
            .iter()
            .enumerate()
            .filter_map(|(i, im)| {
                let dynimg = im.to_dynamic_image().ok()?;
                (dynimg.width() >= min_side && dynimg.height() >= min_side).then(|| (format!("p{page}_{i}"), dynimg))
            })
            .collect()
    };

    if let Some(dir) = get("--dump") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| fail(e));
        let mut n = 0;
        for page in from..=to.min(pages) {
            for (key, img) in images_of(page) {
                img.save(dir.join(format!("{key}.png"))).unwrap_or_else(|e| fail(e));
                println!("{key} {}x{}", img.width(), img.height());
                n += 1;
            }
        }
        eprintln!("{n} images ≥ {min_side}px from pages {from}-{to} of {pages}");
        return;
    }

    let model_dir = PathBuf::from(get("--model-dir").unwrap_or_else(|| fail("--model-dir missing")));
    let text_name = get("--text").unwrap_or_else(|| "model_q4".into());
    let vision_name = get("--vision").unwrap_or_else(|| "vision_encoder_q4".into());
    let qfile = get("--queries").unwrap_or_else(|| fail("--queries missing"));
    let spec: Value = serde_json::from_str(&std::fs::read_to_string(&qfile).unwrap_or_else(|e| fail(e)))
        .unwrap_or_else(|e| fail(e));
    let wanted: BTreeSet<String> = spec["queries"].as_array().into_iter().flatten().filter_map(|q| q["image"].as_str().map(String::from)).collect();

    let model = EmbeddingModel {
        id: 1,
        identifier: "probe".into(),
        kind: ModelKind::LocalOnnx,
        model_path: Some(model_dir.join("onnx").join(format!("{text_name}.onnx")).to_string_lossy().into_owned()),
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
    };
    let vision_path = model_dir.join("onnx").join(format!("{vision_name}.onnx"));
    let t0 = Instant::now();
    let soft: usize = get("--soft").and_then(|v| v.parse().ok()).unwrap_or(app::embedding::vision_prep::MAX_SOFT_TOKENS);
    let emb = OrtImageEmbedder::load(&model, &vision_path.to_string_lossy())
        .unwrap_or_else(|e| fail(e))
        .with_max_soft_tokens(soft);
    eprintln!("loaded {text_name} + {vision_name} in {:.1}s", t0.elapsed().as_secs_f64());
    let text: &OrtEmbedder = emb.text();
    use app::embedding::QueryEmbedder;

    // Every image in the page range + one text vector per page.
    let mut items: Vec<(String, bool, Vec<f32>)> = Vec::new(); // (key, is_image, vec)
    let cache_path = get("--cache");
    let cache_ns = format!("{text_name}+{vision_name}@{soft}");
    let mut cache: BTreeMap<String, Vec<f32>> = cache_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let t_img = Instant::now();
    for page in from..=to.min(pages) {
        for (key, img) in images_of(page) {
            let ck = format!("{cache_ns}|{key}");
            let v = match cache.get(&ck) {
                Some(v) => v.clone(),
                None => {
                    let v = emb.embed_image(&img).unwrap_or_else(|e| fail(format!("{key}: {e}")));
                    cache.insert(ck, v.clone());
                    v
                }
            };
            items.push((key, true, v));
        }
        if page % 25 == 0 {
            eprintln!("  images: page {page}/{to}");
        }
    }
    if let Some(dir) = &img_dir {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir).unwrap_or_else(|e| fail(e)).flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files {
            let Ok(img) = image::open(&f) else { continue };
            let key = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let v = emb.embed_image(&img).unwrap_or_else(|e| fail(format!("{key}: {e}")));
            items.push((key, true, v));
        }
    }
    if let Some(p) = &cache_path {
        std::fs::write(p, serde_json::to_string(&cache).unwrap()).unwrap_or_else(|e| fail(e));
    }
    let n_img = items.len();
    let img_secs = t_img.elapsed().as_secs_f64();
    let t_txt = Instant::now();
    for page in from..=to.min(pages) {
        let Some(doc) = doc.as_ref() else { break };
        let t = doc.extract_text(page - 1).unwrap_or_default();
        let t: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.len() < 40 {
            continue;
        }
        let v = text.embed_query(&model, &format!("{DOC_PREFIX}{t}")).unwrap_or_else(|e| fail(e));
        items.push((format!("text p{page}"), false, v));
    }
    eprintln!(
        "embedded {n_img} images ({:.2}s/image) + {} page texts ({:.2}s/page)",
        img_secs / n_img.max(1) as f64,
        items.len() - n_img,
        t_txt.elapsed().as_secs_f64() / (items.len() - n_img).max(1) as f64
    );
    let missing: Vec<&String> = wanted.iter().filter(|k| !items.iter().any(|(ik, _, _)| ik == *k)).collect();
    if !missing.is_empty() {
        fail(format!("images not found (check --min-side): {missing:?}"));
    }

    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let mut rows = Vec::new();
    let (mut rr_img, mut rr_mix, mut rr_txt, mut rr_fused, mut n) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0usize);
    // Page of an item key: "p33", "p33_0" and "text p33" all → "p33".
    let page_of = |k: &str| k.trim_start_matches("text ").split('_').next().unwrap_or(k).to_string();
    println!("img#   mix#  text#  fused  best      question");
    for q in spec["queries"].as_array().into_iter().flatten() {
        let (question, target) = (q["q"].as_str().unwrap_or(""), q["image"].as_str().unwrap_or(""));
        let qv = text.embed_query(&model, &format!("{QUERY_PREFIX}{question}")).unwrap_or_else(|e| fail(e));
        let mut scored: Vec<(&str, bool, f32)> = items.iter().map(|(k, im, v)| (k.as_str(), *im, dot(&qv, v))).collect();
        scored.sort_by(|a, b| b.2.total_cmp(&a.2));
        let mix_rank = scored.iter().position(|s| s.0 == target).map(|p| p + 1);
        let img_rank = scored.iter().filter(|s| s.1).position(|s| s.0 == target).map(|p| p + 1);
        let page_text = format!("text {}", target.split('_').next().unwrap_or(target));
        let txt_rank = scored.iter().filter(|s| !s.1).position(|s| s.0 == page_text).map(|p| p + 1);
        rr_img += img_rank.map_or(0.0, |r| 1.0 / r as f64);
        rr_mix += mix_rank.map_or(0.0, |r| 1.0 / r as f64);
        rr_txt += txt_rank.map_or(0.0, |r| 1.0 / r as f64);
        // RRF over pages: best rank of the page in the image list + in the text list.
        let mut fused: BTreeMap<String, f64> = BTreeMap::new();
        for want_img in [true, false] {
            let mut seen = BTreeSet::new();
            for (r, it) in scored.iter().filter(|s| s.1 == want_img).enumerate() {
                let pg = page_of(it.0);
                if seen.insert(pg.clone()) {
                    *fused.entry(pg).or_default() += 1.0 / (10.0 + (r + 1) as f64);
                }
            }
        }
        let mut fused: Vec<(String, f64)> = fused.into_iter().collect();
        fused.sort_by(|a, b| b.1.total_cmp(&a.1));
        let fused_rank = fused.iter().position(|(pg, _)| *pg == page_of(target)).map(|p| p + 1);
        rr_fused += fused_rank.map_or(0.0, |r| 1.0 / r as f64);
        n += 1;
        let show = |r: Option<usize>| r.map_or("-".to_string(), |r| r.to_string());
        println!(
            "{:<6} {:<5} {:<6} {:<6} {:<9} {question}",
            show(img_rank),
            show(mix_rank),
            show(txt_rank),
            show(fused_rank),
            scored[0].0
        );
        let top: BTreeMap<usize, Value> = scored.iter().take(5).enumerate().map(|(i, s)| (i + 1, json!({"key": s.0, "score": s.2}))).collect();
        rows.push(json!({"q": question, "image": target, "image_rank": img_rank, "mixed_rank": mix_rank, "text_rank": txt_rank, "fused_rank": fused_rank, "top5": top}));
    }
    let summary = json!({
        "pdf": pdf, "text_model": text_name, "vision_model": vision_name, "images": n_img,
        "page_texts": items.len() - n_img, "mrr_image_only": rr_img / n.max(1) as f64,
        "mrr_mixed": rr_mix / n.max(1) as f64, "mrr_text_only": rr_txt / n.max(1) as f64, "mrr_fused": rr_fused / n.max(1) as f64, "seconds_per_image": img_secs / n_img.max(1) as f64, "queries": rows,
    });
    let nf = n.max(1) as f64;
    println!(
        "MRR image-only {:.3} · mixed {:.3} · text-only baseline {:.3} · fused (RRF) {:.3} · {n} questions, {n_img} images",
        rr_img / nf,
        rr_mix / nf,
        rr_txt / nf,
        rr_fused / nf
    );
    if let Some(out) = get("--out") {
        std::fs::write(&out, serde_json::to_string_pretty(&summary).unwrap()).unwrap_or_else(|e| fail(e));
    }

    // Chunk level (`--chunk-chars N`): page texts cut into ~N-char chunks; the
    // image list is fused into the chunk vector list after mapping each image
    // hit to chunks of its page — (a) only the page's most query-similar
    // chunk, (b) every chunk of the page with weight 1/n, (c) every chunk with
    // full weight. `--image-pages 9,13,…` keeps only those pages' images (the
    // graphics-page selection). MRR = rank of the first chunk of the target page.
    let Some(cc) = get("--chunk-chars").and_then(|v| v.parse::<usize>().ok()) else { return };
    let Some(doc) = doc.as_ref() else { return };
    let img_pages: Option<BTreeSet<String>> =
        get("--image-pages").map(|s| s.split(',').map(|p| format!("p{}", p.trim())).collect());
    let mut chunks: Vec<(String, Vec<f32>)> = Vec::new(); // (page key, vec)
    for page in from..=to.min(pages) {
        let t = doc.extract_text(page - 1).unwrap_or_default();
        let mut cur = String::new();
        let mut flush = |cur: &mut String| {
            if cur.len() >= 40 {
                let v = text.embed_query(&model, &format!("{DOC_PREFIX}{cur}")).unwrap_or_else(|e| fail(e));
                chunks.push((format!("p{page}"), v));
            }
            cur.clear();
        };
        for w in t.split_whitespace() {
            if cur.len() + w.len() + 1 > cc {
                flush(&mut cur);
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(w);
        }
        flush(&mut cur);
    }
    let images: Vec<&(String, bool, Vec<f32>)> = items
        .iter()
        .filter(|(k, im, _)| *im && img_pages.as_ref().is_none_or(|s| s.contains(k)))
        .collect();
    eprintln!("chunk level: {} chunks of ≤{cc} chars, {} page images", chunks.len(), images.len());
    let k = 10.0f64;
    let mut rr = [0.0f64; 4]; // text-only, (a), (b), (c)
    let mut nq = 0usize;
    for q in spec["queries"].as_array().into_iter().flatten() {
        let (question, target) = (q["q"].as_str().unwrap_or(""), q["image"].as_str().unwrap_or(""));
        let target = page_of(target);
        let qv = text.embed_query(&model, &format!("{QUERY_PREFIX}{question}")).unwrap_or_else(|e| fail(e));
        let cs: Vec<f32> = chunks.iter().map(|(_, v)| dot(&qv, v)).collect();
        let mut vec_order: Vec<usize> = (0..chunks.len()).collect();
        vec_order.sort_by(|&a, &b| cs[b].total_cmp(&cs[a]));
        let mut img_order: Vec<(&str, f32)> = images.iter().map(|(k, _, v)| (k.as_str(), dot(&qv, v))).collect();
        img_order.sort_by(|a, b| b.1.total_cmp(&a.1));
        for variant in 0..4 {
            let mut score = vec![0.0f64; chunks.len()];
            for (r, &ci) in vec_order.iter().enumerate() {
                score[ci] += 1.0 / (k + (r + 1) as f64);
            }
            if variant > 0 {
                for (r, (pg, _)) in img_order.iter().enumerate() {
                    let on_page: Vec<usize> = (0..chunks.len()).filter(|&i| chunks[i].0 == *pg).collect();
                    let contrib = 1.0 / (k + (r + 1) as f64);
                    match variant {
                        1 => {
                            if let Some(&best) = on_page.iter().max_by(|&&a, &&b| cs[a].total_cmp(&cs[b])) {
                                score[best] += contrib;
                            }
                        }
                        2 => on_page.iter().for_each(|&i| score[i] += contrib / on_page.len() as f64),
                        _ => on_page.iter().for_each(|&i| score[i] += contrib),
                    }
                }
            }
            let mut order: Vec<usize> = (0..chunks.len()).collect();
            order.sort_by(|&a, &b| score[b].total_cmp(&score[a]));
            if let Some(p) = order.iter().position(|&i| chunks[i].0 == target) {
                rr[variant] += 1.0 / (p + 1) as f64;
            }
        }
        nq += 1;
    }
    let nf = nq.max(1) as f64;
    println!(
        "chunk-level MRR: text-only {:.3} · (a) best chunk {:.3} · (b) all chunks 1/n {:.3} · (c) all chunks full {:.3}",
        rr[0] / nf,
        rr[1] / nf,
        rr[2] / nf,
        rr[3] / nf
    );
}

#[cfg(all(feature = "onnx", feature = "pdf-render"))]
fn render_page(doc: &pdf_oxide::PdfDocument, page_index: usize, dpi: u32) -> Option<image::DynamicImage> {
    let opts = pdf_oxide::rendering::RenderOptions::with_dpi(dpi);
    let png = pdf_oxide::rendering::render_page(doc, page_index, &opts).ok()?;
    image::load_from_memory(&png.data).ok()
}

#[cfg(all(feature = "onnx", not(feature = "pdf-render")))]
fn render_page(_: &pdf_oxide::PdfDocument, _: usize, _: u32) -> Option<image::DynamicImage> {
    eprintln!("--render needs --features pdf-render");
    std::process::exit(2);
}

#[cfg(not(feature = "onnx"))]
fn main() {
    eprintln!("build with --features onnx-download");
}
