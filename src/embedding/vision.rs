//! Image embedding with EmbeddingGemma 2: vision encoder + the text model's
//! `image_features` input, so images land in the SAME vector space as text
//! (a text question can be compared with an image vector directly).
//! Compiled only under the `onnx` feature.
//!
//! Pipeline (preprocessing pure in `vision_prep.rs`): decode → RGB → bicubic
//! resize to `target_size` → patches + positions, padded to the budget →
//! `vision_encoder.onnx` (`pixel_values [1, P, 768]`, `pixel_position_ids
//! [1, P, 2]` → `image_features [n, 512]`) → text model with the placeholder
//! prompt from `image_prompt(n)` and those features → `sentence_embedding`.
//! Images get no task prefix (model card: prefixes are text-only).
//!
//! The text model is the regular `OrtEmbedder` (same file as text embedding),
//! held here so an image and its text queries share one session. The vision
//! session runs on the CPU EP; CoreML for it is unmeasured.

use std::sync::Mutex;

use image::imageops::FilterType;
use image::DynamicImage;
use ndarray::{Array2, Array3};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

use super::onnx::OrtEmbedder;
use super::vision_prep::{image_prompt, max_patches, pad_to, patchify, positions, target_size, MAX_SOFT_TOKENS, PATCH, POOL};
use crate::db::models::EmbeddingModel;
use crate::{CoreError, Result};

pub struct OrtImageEmbedder {
    vision: Mutex<Session>,
    text: OrtEmbedder,
}

impl OrtImageEmbedder {
    /// `model` describes the text model (as for `OrtEmbedder::load`);
    /// `vision_model_path` is the matching `vision_encoder*.onnx`.
    pub fn load(model: &EmbeddingModel, vision_model_path: &str) -> Result<Self> {
        let text = OrtEmbedder::load(model)?;
        let vision = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .commit_from_file(vision_model_path)?;
        Ok(Self { vision: Mutex::new(vision), text })
    }

    /// The text side, for queries/documents in the same space.
    pub fn text(&self) -> &OrtEmbedder {
        &self.text
    }

    /// Embed one image (L2-normalised, model dimension).
    pub fn embed_image(&self, img: &DynamicImage) -> Result<Vec<f32>> {
        let rgb = img.to_rgb8();
        let (h, w) = (rgb.height() as usize, rgb.width() as usize);
        let budget = max_patches(MAX_SOFT_TOKENS, POOL);
        let (th, tw) = target_size(h, w, PATCH, budget, POOL);
        let rgb = if (th, tw) == (h, w) {
            rgb
        } else {
            image::imageops::resize(&rgb, tw as u32, th as u32, FilterType::CatmullRom)
        };
        let mut patches = patchify(rgb.as_raw(), th, tw, PATCH);
        let mut pos = positions(th / PATCH, tw / PATCH);
        let soft = (pos.len() / 2) / (POOL * POOL);
        let patch_len = PATCH * PATCH * 3;
        pad_to(&mut patches, &mut pos, patch_len, budget);
        let pixel_values = Array3::from_shape_vec((1, budget, patch_len), patches)
            .map_err(|e| CoreError::Embedding(e.to_string()))?;
        let position_ids =
            Array3::from_shape_vec((1, budget, 2), pos).map_err(|e| CoreError::Embedding(e.to_string()))?;

        let features = {
            let mut vision = self
                .vision
                .lock()
                .map_err(|_| CoreError::Embedding("vision session lock poisoned".into()))?;
            let outputs = vision.run(ort::inputs![
                "pixel_values" => Tensor::from_array(pixel_values)?,
                "pixel_position_ids" => Tensor::from_array(position_ids)?,
            ])?;
            let (shape, data) = outputs["image_features"].try_extract_tensor::<f32>()?;
            let (n, d) = (shape[0] as usize, shape[1] as usize);
            Array2::from_shape_vec((n, d), data.to_vec()).map_err(|e| CoreError::Embedding(e.to_string()))?
        };
        if features.nrows() != soft {
            return Err(CoreError::Embedding(format!(
                "vision encoder returned {} soft tokens, expected {soft}",
                features.nrows()
            )));
        }
        self.text.embed_with_features(&image_prompt(soft), Some(features))
    }
}
