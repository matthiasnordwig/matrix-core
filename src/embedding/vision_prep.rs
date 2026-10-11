//! Image preprocessing for the EmbeddingGemma 2 vision encoder (Gemma 4
//! image processor), pure and model-free so it is tested without ONNX.
//!
//! Mirrors `transformers` `Gemma4ImageProcessor` + `EmbeddingGemma2Processor`:
//! 1. `target_size`: aspect-ratio-preserving resize so the image yields at most
//!    `max_soft_tokens * pool²` patches of `patch`×`patch` px, both sides a
//!    multiple of `pool * patch` (48 px for the shipped config);
//! 2. pixels rescaled to [0, 1] (no mean/std normalisation);
//! 3. `patchify`: patches in row-major grid order, each flattened as
//!    (row-in-patch, col-in-patch, channel) → `patch² * 3` floats (768);
//! 4. `positions`: per patch `(x = grid column, y = grid row)`;
//! 5. `pad_to`: zero patches / `-1` positions up to the fixed budget;
//! 6. `image_prompt`: the text side — `<|image>` + one `<|image|>` per soft
//!    token (`patches / pool²`) + `<image|>`; the tokenizer adds `<bos>`/`<eos>`.
//! The resize itself (bicubic) lives in `vision.rs` (needs the `image` crate).

/// Gemma 4 image processor defaults (`processor_config.json` of the export).
pub const PATCH: usize = 16;
pub const POOL: usize = 3;
pub const MAX_SOFT_TOKENS: usize = 280;

/// Patch budget for one image (`max_soft_tokens * pool²`).
pub fn max_patches(max_soft_tokens: usize, pool: usize) -> usize {
    max_soft_tokens * pool * pool
}

/// Largest (height, width) that keeps the aspect ratio, fits `max_patches`
/// patches and has both sides divisible by `pool * patch`. Degenerate very
/// thin images get one `pool * patch` strip on the short side, like upstream.
pub fn target_size(height: usize, width: usize, patch: usize, max_patches: usize, pool: usize) -> (usize, usize) {
    let (h, w) = (height.max(1) as f64, width.max(1) as f64);
    let target_px = (max_patches * patch * patch) as f64;
    let factor = (target_px / (h * w)).sqrt();
    let side = pool * patch;
    let mut th = ((factor * h) / side as f64).floor() as usize * side;
    let mut tw = ((factor * w) / side as f64).floor() as usize * side;
    let max_side = (max_patches / (pool * pool)) * side;
    if th == 0 && tw == 0 {
        th = side;
        tw = side;
    } else if th == 0 {
        th = side;
        tw = ((w / h).floor() as usize * side).min(max_side);
    } else if tw == 0 {
        tw = side;
        th = ((h / w).floor() as usize * side).min(max_side);
    }
    (th, tw)
}

/// RGB8 image (row-major, 3 bytes per pixel, `height`×`width`, both multiples
/// of `patch`) → patches in grid order, values rescaled to [0, 1].
pub fn patchify(rgb: &[u8], height: usize, width: usize, patch: usize) -> Vec<f32> {
    let (ph, pw) = (height / patch, width / patch);
    let mut out = Vec::with_capacity(ph * pw * patch * patch * 3);
    for gr in 0..ph {
        for gc in 0..pw {
            for y in 0..patch {
                let row = (gr * patch + y) * width;
                for x in 0..patch {
                    let px = (row + gc * patch + x) * 3;
                    for c in 0..3 {
                        out.push(rgb[px + c] as f32 / 255.0);
                    }
                }
            }
        }
    }
    out
}

/// Per-patch `(x, y)` grid positions in the same order as [`patchify`].
pub fn positions(patch_rows: usize, patch_cols: usize) -> Vec<i64> {
    let mut out = Vec::with_capacity(patch_rows * patch_cols * 2);
    for r in 0..patch_rows {
        for c in 0..patch_cols {
            out.push(c as i64);
            out.push(r as i64);
        }
    }
    out
}

/// Pad patches (zeros) and positions (`-1`) up to `target` patches.
pub fn pad_to(patches: &mut Vec<f32>, pos: &mut Vec<i64>, patch_len: usize, target: usize) {
    let n = pos.len() / 2;
    if n < target {
        patches.resize(target * patch_len, 0.0);
        pos.resize(target * 2, -1);
    }
}

/// Text-side placeholder sequence for one image with `soft_tokens` tokens.
pub fn image_prompt(soft_tokens: usize) -> String {
    let mut s = String::with_capacity(16 + soft_tokens * 9);
    s.push_str("<|image>");
    for _ in 0..soft_tokens {
        s.push_str("<|image|>");
    }
    s.push_str("<image|>");
    s
}
