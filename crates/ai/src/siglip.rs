//! SigLIP 2 ViT-B/16 (multilingual), the ONNX export from the Immich
//! project: an image and a text tower that map into the same 768-dimension
//! space, so a photo and a description of it lie close together. Embeddings
//! are L2-normalized; cosine similarity is a dot product.

use std::path::Path;

use image::DynamicImage;
use image::imageops::FilterType;

use crate::onnx::Model;
use crate::vectors;

/// Stored with every embedding, so a later model never mixes with this one.
pub const MODEL_ID: &str = "siglip2-b16";
pub const DIM: usize = 768;
const SIZE: u32 = 224;
const CONTEXT: usize = 64;

pub struct ImageEncoder {
    model: Model,
}

impl ImageEncoder {
    /// `dir` is the pack folder (with `visual/model.onnx`).
    pub fn load(dir: &Path, gpu: bool) -> anyhow::Result<Self> {
        Ok(ImageEncoder { model: Model::load(&dir.join("visual").join("model.onnx"), gpu)? })
    }

    pub fn on_gpu(&self) -> bool {
        self.model.gpu
    }

    pub fn embed(&self, img: &DynamicImage) -> anyhow::Result<Vec<f32>> {
        let (_, mut v) = self.model.run_f32("image", vec![1, 3, SIZE as i64, SIZE as i64], pixels(img))?;
        anyhow::ensure!(v.len() == DIM, "unexpected embedding size {}", v.len());
        vectors::normalize(&mut v);
        Ok(v)
    }
}

/// Squashed to 224×224 (bicubic), RGB planes scaled to −1…1, as the
/// export's `preprocess_cfg.json` says.
pub fn pixels(img: &DynamicImage) -> Vec<f32> {
    let rgb = img.resize_exact(SIZE, SIZE, FilterType::CatmullRom).to_rgb8();
    let n = (SIZE * SIZE) as usize;
    let mut out = vec![0f32; 3 * n];
    for (i, p) in rgb.pixels().enumerate() {
        for c in 0..3 {
            out[c * n + i] = p[c] as f32 / 127.5 - 1.0;
        }
    }
    out
}

pub struct TextEncoder {
    model: Model,
    tokenizer: tokenizers::Tokenizer,
    pad: u32,
}

impl TextEncoder {
    /// `dir` is the pack folder (with `textual/model.onnx` and
    /// `textual/tokenizer.json`). The text tower is large and only needed
    /// for searching and for sorting prompts, so it runs on the CPU.
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let model = Model::load(&dir.join("textual").join("model.onnx"), false)?;
        let tokenizer = tokenizers::Tokenizer::from_file(dir.join("textual").join("tokenizer.json")).map_err(|e| anyhow::anyhow!("{e}"))?;
        let pad = tokenizer.token_to_id("<pad>").unwrap_or(0);
        Ok(TextEncoder { model, tokenizer, pad })
    }

    pub fn tokens(&self, text: &str) -> anyhow::Result<Vec<i32>> {
        let enc = self.tokenizer.encode(canonical(text), true).map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut ids: Vec<i32> = enc.get_ids().iter().map(|i| *i as i32).collect();
        ids.truncate(CONTEXT);
        ids.resize(CONTEXT, self.pad as i32);
        Ok(ids)
    }

    pub fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let (_, mut v) = self.model.run_i32("text", vec![1, CONTEXT as i64], self.tokens(text)?)?;
        anyhow::ensure!(v.len() == DIM, "unexpected embedding size {}", v.len());
        vectors::normalize(&mut v);
        Ok(v)
    }
}

/// open_clip's "canonicalize" cleaning: whitespace collapsed, punctuation
/// removed, lower case.
pub fn canonical(text: &str) -> String {
    let t: String = text.chars().filter(|c| !c.is_ascii_punctuation()).collect::<String>().to_lowercase();
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleaning_and_pixels() {
        assert_eq!(canonical("  A  Photo, of my DOG!  "), "a photo of my dog");
        let img = DynamicImage::new_rgb8(10, 20);
        let p = pixels(&img);
        assert_eq!(p.len(), 3 * 224 * 224);
        assert!(p.iter().all(|x| (*x + 1.0).abs() < 1e-6), "black is -1");
    }
}
