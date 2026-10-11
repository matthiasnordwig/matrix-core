//! Per-page measurements for graphics-page detection (which PDF pages are
//! worth embedding as an image). Headless, no DB, no model.
//!
//! For each page: render at 40 dpi (raw RGBA), mask the character boxes from
//! pdf_oxide `extract_chars`, then report
//!   ink        — share of non-white pixels (grey and black count too),
//!   ink_free   — the same outside the (slightly widened) character boxes,
//!   color      — share of saturated pixels,
//!   paths / curves / fills — vector drawing commands (all, Bézier segments,
//!                filled non-white paths) whose box lies outside the text,
//!   path_area  — share of the page covered by those paths' boxes,
//!   img_area   — share covered by raster images,
//!   chars      — number of characters.
//!   cargo run --release --example page_stats --features pdf-render -- \
//!       --pdf <file.pdf> [--pages 1-40] [--png <dir>]   (TSV on stdout)
//! `--png` also writes each 40-dpi render as `p<page>.png` (for labelling).

#[cfg(feature = "pdf-render")]
fn main() {
    use pdf_oxide::elements::PathOperation;
    use pdf_oxide::rendering::{render_page, RenderOptions};
    use pdf_oxide::PdfDocument;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let Some(pdf) = get("--pdf") else {
        eprintln!("usage: see header of examples/page_stats.rs");
        std::process::exit(2);
    };
    let doc = PdfDocument::open(&pdf).expect("open pdf");
    let pages = doc.page_count().expect("page count");
    let (from, to) = get("--pages")
        .and_then(|r| r.split_once('-').map(|(a, b)| (a.parse().unwrap_or(1), b.parse().unwrap_or(pages))))
        .unwrap_or((1, pages));
    let png_dir = get("--png");
    if let Some(d) = &png_dir {
        std::fs::create_dir_all(d).expect("png dir");
    }
    const DPI: u32 = 40;
    let scale = DPI as f32 / 72.0;
    println!("page\tink\tink_free\tcolor\tcolor_free\tsolid\tpaths\tcurves\tfills\tboxes\tdiag\tpath_area\timg_area\tchars\tms");
    for page in from..=to.min(pages) {
        let t0 = std::time::Instant::now();
        let idx = page - 1;
        let img = render_page(&doc, idx, &RenderOptions::with_dpi(DPI).as_raw()).expect("render");
        let (w, h) = (img.width as usize, img.height as usize);
        let (_, _, pw, ph) = doc.get_page_media_box(idx).unwrap_or((0.0, 0.0, w as f32 / scale, h as f32 / scale));
        let (pw, ph) = (pw.abs().max(1.0), ph.abs().max(1.0));
        // PDF user space (origin bottom-left) → pixel box, widened by `pad` pt.
        let to_px = |x: f32, y: f32, bw: f32, bh: f32, pad: f32| {
            let sx = w as f32 / pw;
            let sy = h as f32 / ph;
            let x0 = ((x - pad) * sx).floor().max(0.0) as usize;
            let x1 = (((x + bw + pad) * sx).ceil() as usize).min(w);
            let y0 = ((ph - (y + bh + pad)) * sy).floor().max(0.0) as usize;
            let y1 = (((ph - (y - pad)) * sy).ceil().max(0.0) as usize).min(h);
            (x0, x1, y0, y1)
        };
        let mut text = vec![false; w * h];
        let chars = doc.extract_chars(idx).unwrap_or_default();
        for c in &chars {
            let b = &c.bbox;
            let (x0, x1, y0, y1) = to_px(b.x, b.y, b.width, b.height, 1.5);
            for yy in y0..y1 {
                for xx in x0..x1 {
                    text[yy * w + xx] = true;
                }
            }
        }
        let (mut ink, mut ink_free, mut color, mut color_free) = (0usize, 0usize, 0usize, 0usize);
        for (i, px) in img.data.chunks_exact(4).enumerate() {
            let (r, g, b) = (px[0], px[1], px[2]);
            let (mn, mx) = (r.min(g).min(b), r.max(g).max(b));
            if mn < 230 {
                ink += 1;
                if !text[i] {
                    ink_free += 1;
                }
            }
            if mx - mn > 40 {
                color += 1;
                if !text[i] {
                    color_free += 1;
                }
            }
        }
        // Solid non-text areas: pixels whose whole 3×3 neighbourhood is
        // "strong" (dark or saturated) and outside text — thin rules and
        // light table shading erode away, bars/wedges/filled boxes stay.
        let strong: Vec<bool> = img
            .data
            .chunks_exact(4)
            .enumerate()
            .map(|(i, px)| {
                let (r, g, b) = (px[0] as u32, px[1] as u32, px[2] as u32);
                let lum = (r * 299 + g * 587 + b * 114) / 1000;
                let sat = r.max(g).max(b) - r.min(g).min(b);
                !text[i] && (lum < 200 || sat > 60)
            })
            .collect();
        let mut solid = 0usize;
        for y in 1..h.saturating_sub(1) {
            for x in 1..w.saturating_sub(1) {
                if (0..3).all(|dy| (0..3).all(|dx| strong[(y + dy - 1) * w + x + dx - 1])) {
                    solid += 1;
                }
            }
        }
        let mut covered = vec![false; w * h];
        let (mut paths, mut curves, mut fills, mut boxes, mut diag) = (0usize, 0usize, 0usize, 0usize, 0usize);
        for p in doc.extract_paths(idx).unwrap_or_default() {
            let b = &p.bbox;
            let (x0, x1, y0, y1) = to_px(b.x, b.y, b.width, b.height, 0.0);
            if x1 <= x0 && y1 <= y0 {
                continue;
            }
            // Inside text = its centre lies in a character box (underlines, bullets).
            let (cx, cy) = (((x0 + x1) / 2).min(w - 1), ((y0 + y1) / 2).min(h - 1));
            if text[cy * w + cx] {
                continue;
            }
            // Full-page background rectangles are not drawings.
            if (x1 - x0) * (y1 - y0) > w * h * 9 / 10 {
                continue;
            }
            paths += 1;
            curves += p.operations.iter().filter(|o| matches!(o, PathOperation::CurveTo(..))).count();
            // 2-D shapes (boxes, bars, wedges) — not rules/bands: both sides
            // ≥ 6 pt and narrower than a third of the page.
            if b.width >= 6.0 && b.height >= 6.0 && b.width < pw / 3.0 {
                boxes += 1;
            }
            let mut last = (0.0f32, 0.0f32);
            for o in &p.operations {
                match *o {
                    PathOperation::MoveTo(x, y) => last = (x, y),
                    PathOperation::LineTo(x, y) => {
                        let (dx, dy) = ((x - last.0).abs(), (y - last.1).abs());
                        if dx > 0.3 && dy > 0.3 {
                            diag += 1;
                        }
                        last = (x, y);
                    }
                    _ => {}
                }
            }
            if let Some(f) = &p.fill_color {
                if f.r.min(f.g).min(f.b) < 0.9 {
                    fills += 1;
                }
            }
            for yy in y0..y1.max(y0 + 1).min(h) {
                for xx in x0..x1.max(x0 + 1).min(w) {
                    covered[yy * w + xx] = true;
                }
            }
        }
        let mut img_cov = vec![false; w * h];
        for im in doc.extract_images(idx).unwrap_or_default() {
            if let Some(b) = im.bbox() {
                let (x0, x1, y0, y1) = to_px(b.x, b.y, b.width, b.height, 0.0);
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        img_cov[yy * w + xx] = true;
                    }
                }
            }
        }
        let n = (w * h) as f64;
        let frac = |v: usize| v as f64 / n;
        let path_area = frac(covered.iter().zip(&text).filter(|(c, t)| **c && !**t).count());
        let img_area = frac(img_cov.iter().filter(|c| **c).count());
        let ms = t0.elapsed().as_millis();
        println!(
            "{page}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{paths}\t{curves}\t{fills}\t{boxes}\t{diag}\t{path_area:.4}\t{img_area:.4}\t{}\t{ms}",
            frac(ink),
            frac(ink_free),
            frac(color),
            frac(color_free),
            frac(solid),
            chars.len()
        );
        if let Some(d) = &png_dir {
            let png = render_page(&doc, idx, &RenderOptions::with_dpi(DPI)).expect("render png");
            std::fs::write(format!("{d}/p{page}.png"), png.data).expect("write png");
        }
    }
}

#[cfg(not(feature = "pdf-render"))]
fn main() {
    eprintln!("build with --features pdf-render");
}
