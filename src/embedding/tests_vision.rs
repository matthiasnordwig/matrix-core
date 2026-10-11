//! Tests for `vision_prep.rs` (Gemma 4 image preprocessing), values checked
//! against the upstream `transformers` formulas by hand.

use super::vision_prep::*;

#[test]
fn target_size_square_fits_budget() {
    let mp = max_patches(MAX_SOFT_TOKENS, POOL);
    assert_eq!(mp, 2520);
    let (h, w) = target_size(1000, 1000, PATCH, mp, POOL);
    assert_eq!((h, w), (768, 768));
    assert!((h / PATCH) * (w / PATCH) <= mp);
}

#[test]
fn target_size_keeps_aspect_and_multiple_of_48() {
    let mp = max_patches(MAX_SOFT_TOKENS, POOL);
    let (h, w) = target_size(3000, 1000, PATCH, mp, POOL);
    assert_eq!((h, w), (1344, 432));
    assert_eq!(h % 48, 0);
    assert_eq!(w % 48, 0);
    // soft tokens = patches / pool²
    assert_eq!((h / PATCH) * (w / PATCH) / (POOL * POOL), 252);
}

#[test]
fn target_size_upscales_small_images() {
    let mp = max_patches(MAX_SOFT_TOKENS, POOL);
    let (h, w) = target_size(100, 200, PATCH, mp, POOL);
    assert!(h > 100 && w > 200);
    assert!((h / PATCH) * (w / PATCH) <= mp);
}

#[test]
fn target_size_thin_image_gets_one_strip_capped() {
    let mp = max_patches(MAX_SOFT_TOKENS, POOL);
    let (h, w) = target_size(10, 5000, PATCH, mp, POOL);
    assert_eq!(h, 48);
    assert_eq!(w, 280 * 48);
    assert!((h / PATCH) * (w / PATCH) <= mp);
}

#[test]
fn patchify_orders_grid_then_row_col_channel() {
    // 2×2 patches of 1 px each → patch=1, image 2×2, pixel value encodes position.
    let rgb: Vec<u8> = vec![
        0, 1, 2, 10, 11, 12, // row 0: (0,0) (0,1)
        20, 21, 22, 30, 31, 32, // row 1: (1,0) (1,1)
    ];
    let p = patchify(&rgb, 2, 2, 1);
    let back: Vec<u8> = p.iter().map(|v| (v * 255.0).round() as u8).collect();
    assert_eq!(back, rgb);

    // patch=2 on a 2×4 image: two patches, first holds columns 0..2 of both rows.
    let mut img = vec![0u8; 2 * 4 * 3];
    for y in 0..2 {
        for x in 0..4 {
            img[(y * 4 + x) * 3] = (y * 10 + x) as u8;
        }
    }
    let p = patchify(&img, 2, 4, 2);
    assert_eq!(p.len(), 2 * 2 * 2 * 3);
    let red: Vec<u8> = p.chunks(3).map(|c| (c[0] * 255.0).round() as u8).collect();
    assert_eq!(red, vec![0, 1, 10, 11, 2, 3, 12, 13]);
}

#[test]
fn positions_are_x_then_y() {
    assert_eq!(positions(2, 3), vec![0, 0, 1, 0, 2, 0, 0, 1, 1, 1, 2, 1]);
}

#[test]
fn pad_fills_zero_patches_and_negative_positions() {
    let mut patches = vec![0.5f32; 2 * 4];
    let mut pos = vec![0i64, 0, 1, 0];
    pad_to(&mut patches, &mut pos, 4, 3);
    assert_eq!(patches.len(), 12);
    assert_eq!(&patches[8..], &[0.0; 4]);
    assert_eq!(pos, vec![0, 0, 1, 0, -1, -1]);
}

#[test]
fn image_prompt_wraps_soft_tokens() {
    assert_eq!(image_prompt(2), "<|image><|image|><|image|><image|>");
    assert_eq!(image_prompt(0), "<|image><image|>");
}
