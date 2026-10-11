//! Per-page measurements for graphics-page detection (which PDF pages are
//! worth embedding as an image). Headless, no DB, no model. The measuring and
//! the rule are production code (`embedding::page_render::measure_page`,
//! `embedding::page_select`); this prints them as a table.
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
//!   chars      — number of characters,
//!   graphics   — 1 if the page is a graphics page (`PageStats::is_graphics_page`).
//!   cargo run --release --example page_stats --features pdf-render -- \
//!       --pdf <file.pdf> [--pages 1-40] [--png <dir>]   (TSV on stdout)
//! `--png` also writes each 40-dpi render as `p<page>.png` (for labelling).

#[cfg(feature = "pdf-render")]
fn main() {
    use app::embedding::page_render::{measure_page, SELECT_DPI};
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
    println!("page\tink\tink_free\tcolor\tcolor_free\tsolid\tpaths\tcurves\tfills\tboxes\tdiag\tpath_area\timg_area\tchars\tms\tgraphics");
    for page in from..=to.min(pages) {
        let t0 = std::time::Instant::now();
        let idx = page - 1;
        let Some(s) = measure_page(&doc, idx) else {
            eprintln!("page {page}: render failed, skipped");
            continue;
        };
        let ms = t0.elapsed().as_millis();
        println!(
            "{page}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{}\t{ms}\t{}",
            s.ink,
            s.ink_free,
            s.color,
            s.color_free,
            s.solid,
            s.paths,
            s.curves,
            s.fills,
            s.boxes,
            s.diag,
            s.path_area,
            s.img_area,
            s.chars,
            u8::from(s.is_graphics_page())
        );
        if let Some(d) = &png_dir {
            let png = render_page(&doc, idx, &RenderOptions::with_dpi(SELECT_DPI)).expect("render png");
            std::fs::write(format!("{d}/p{page}.png"), png.data).expect("write png");
        }
    }
}

#[cfg(not(feature = "pdf-render"))]
fn main() {
    eprintln!("build with --features pdf-render");
}
