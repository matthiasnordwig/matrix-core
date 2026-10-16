//! PDF side of the image search (feature `pdf-render`, pdf_oxide + tiny-skia,
//! pure Rust): which pages of a PDF are graphics pages, and their JPEG renders.
//!
//! Per page: 40-dpi raw RGBA render + `extract_chars`/`extract_paths`/
//! `extract_images` → `page_select::page_stats` → the graphics-page rule; a
//! graphics page is then rendered again at [`IMAGE_DPI`] as JPEG
//! ([`JPEG_QUALITY`]). A page that fails (render error, even a panic inside
//! the PDF library) is skipped — one broken page never fails the whole PDF.
//! [`PageRenderer`] renders single pages on demand at any resolution (OCR of
//! scanned pages in the app's ingest).
//! `examples/page_stats.rs` prints the same measurements as a table.

use pdf_oxide::elements::PathOperation;
use pdf_oxide::rendering::{render_page, RenderOptions};
use pdf_oxide::PdfDocument;

use super::page_select::{page_stats, BoxPt, PageInput, PageStats, PathInfo, PathOp};
use crate::{CoreError, Result};

/// Resolution of the classification render.
pub const SELECT_DPI: u32 = 40;
/// Resolution of the stored page image (the vision encoder downsizes to its
/// token budget anyway; 100 dpi keeps org-chart labels legible).
pub const IMAGE_DPI: u32 = 100;
pub const JPEG_QUALITY: u8 = 80;

/// One graphics page as stored in `page_images`.
#[derive(Debug, Clone)]
pub struct RenderedPage {
    /// 1-based page number.
    pub page: usize,
    pub width: u32,
    pub height: u32,
    pub jpeg: Vec<u8>,
}

fn to_box(r: &pdf_oxide::geometry::Rect) -> BoxPt {
    BoxPt { x: r.x, y: r.y, width: r.width, height: r.height }
}

/// Run `f`, turning a panic inside pdf_oxide into `None`.
fn guarded<T>(f: impl FnOnce() -> Option<T>) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok().flatten()
}

/// Measurements of one page (0-based `idx`); `None` if it cannot be rendered.
pub fn measure_page(doc: &PdfDocument, idx: usize) -> Option<PageStats> {
    guarded(|| {
        let img = render_page(doc, idx, &RenderOptions::with_dpi(SELECT_DPI).as_raw()).ok()?;
        let (w, h) = (img.width as usize, img.height as usize);
        let scale = SELECT_DPI as f32 / 72.0;
        let (_, _, pw, ph) = doc.get_page_media_box(idx).unwrap_or((0.0, 0.0, w as f32 / scale, h as f32 / scale));
        let chars: Vec<BoxPt> = doc.extract_chars(idx).unwrap_or_default().iter().map(|c| to_box(&c.bbox)).collect();
        let paths: Vec<PathInfo> = doc
            .extract_paths(idx)
            .unwrap_or_default()
            .iter()
            .map(|p| PathInfo {
                bbox: to_box(&p.bbox),
                ops: p
                    .operations
                    .iter()
                    .map(|o| match *o {
                        PathOperation::MoveTo(x, y) => PathOp::MoveTo(x, y),
                        PathOperation::LineTo(x, y) => PathOp::LineTo(x, y),
                        PathOperation::CurveTo(..) => PathOp::CurveTo,
                        _ => PathOp::Other,
                    })
                    .collect(),
                dark_fill: p.fill_color.as_ref().is_some_and(|f| f.r.min(f.g).min(f.b) < 0.9),
            })
            .collect();
        let images: Vec<BoxPt> =
            doc.extract_images(idx).unwrap_or_default().iter().filter_map(|im| im.bbox().map(to_box)).collect();
        Some(page_stats(&PageInput {
            rgba: &img.data,
            width: w,
            height: h,
            page_w: pw,
            page_h: ph,
            chars: &chars,
            paths: &paths,
            images: &images,
        }))
    })
}

/// JPEG render of one page (0-based `idx`) at [`IMAGE_DPI`].
pub fn render_page_jpeg(doc: &PdfDocument, idx: usize) -> Option<RenderedPage> {
    guarded(|| {
        let img = render_page(doc, idx, &RenderOptions::with_dpi(IMAGE_DPI).as_jpeg(JPEG_QUALITY)).ok()?;
        Some(RenderedPage { page: idx + 1, width: img.width, height: img.height, jpeg: img.data })
    })
}

/// 1-based numbers of the graphics pages of an opened PDF.
pub fn graphics_page_numbers(doc: &PdfDocument) -> Vec<usize> {
    let pages = doc.page_count().unwrap_or(0);
    (0..pages).filter(|&i| measure_page(doc, i).is_some_and(|s| s.is_graphics_page())).map(|i| i + 1).collect()
}

/// PDF bytes → JPEG renders of its graphics pages, in page order. Fails only
/// when the PDF cannot be opened at all.
pub fn graphics_pages(pdf: &[u8]) -> Result<Vec<RenderedPage>> {
    Ok(graphics_pages_counted(pdf)?.1)
}

/// [`graphics_pages`] plus the page count pdf_oxide sees — the ingest only
/// trusts page offsets from `pdf_extract` when both libraries agree on it.
pub fn graphics_pages_counted(pdf: &[u8]) -> Result<(usize, Vec<RenderedPage>)> {
    let r = PageRenderer::open(pdf)?;
    let pages = graphics_page_numbers(&r.doc).into_iter().filter_map(|p| render_page_jpeg(&r.doc, p - 1)).collect();
    Ok((r.page_count(), pages))
}

/// An opened PDF whose single pages are rendered on demand (OCR of scanned
/// pages, a page stored outside the graphics rule). Not `Send`-bound by
/// contract: keep it on the thread that opened it.
pub struct PageRenderer {
    doc: PdfDocument,
}

impl PageRenderer {
    /// Open `pdf`; fails when it cannot be parsed (a panic inside the PDF library included).
    pub fn open(pdf: &[u8]) -> Result<Self> {
        let doc = guarded(|| Some(PdfDocument::from_bytes(pdf.to_vec())))
            .ok_or_else(|| CoreError::Embedding("PDF parser panicked".into()))?
            .map_err(|e| CoreError::Embedding(format!("open PDF: {e}")))?;
        Ok(Self { doc })
    }

    /// The page count pdf_oxide sees (0 when it cannot tell).
    pub fn page_count(&self) -> usize {
        guarded(|| self.doc.page_count().ok()).unwrap_or(0)
    }

    /// JPEG of the 1-based `page` at `dpi`; `None` when that page cannot be rendered.
    pub fn render_jpeg(&self, page: usize, dpi: u32, quality: u8) -> Option<RenderedPage> {
        let idx = page.checked_sub(1)?;
        guarded(|| {
            let img = render_page(&self.doc, idx, &RenderOptions::with_dpi(dpi).as_jpeg(quality)).ok()?;
            Some(RenderedPage { page, width: img.width, height: img.height, jpeg: img.data })
        })
    }
}

#[cfg(test)]
mod tests {
    /// The calibrated rule on the KfW report excerpt (`eval_runs/retrieval/images/ap0/README.md`).
    /// Needs the sample PDF next to the repos: `cargo test --features
    /// pdf-render --lib page_render -- --ignored`.
    #[test]
    #[ignore]
    fn kfw_excerpt_selects_the_calibrated_pages() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../Beipspieldokumente/KfW-Finanzbericht-2025-Auszug.pdf");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("skipped: {path} not found");
            return;
        };
        let expected = vec![1, 2, 3, 4, 6, 9, 13, 15, 17, 19, 21, 22, 23, 25, 27, 32, 39, 40, 41];
        let doc = pdf_oxide::PdfDocument::from_bytes(bytes.clone()).unwrap();
        assert_eq!(super::graphics_page_numbers(&doc), expected);
        let pages = super::graphics_pages(&bytes).unwrap();
        assert_eq!(pages.iter().map(|p| p.page).collect::<Vec<_>>(), expected);
        assert!(pages.iter().all(|p| p.jpeg.starts_with(&[0xFF, 0xD8]) && p.width > 500 && p.height > 500));
    }
}
