//! Cheap image statistics on the 256 px thumbnail: brightness, contrast,
//! blur (variance of the Laplacian), colorfulness and a text-like edge
//! ratio. They feed junk detection and the sorting features.

use image::RgbImage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ImageStats {
    /// Mean luma, 0..1.
    pub brightness: f32,
    /// Standard deviation of luma, 0..1.
    pub contrast: f32,
    /// Variance of the Laplacian on luma (0..255 scale). Low means blurry.
    pub sharpness: f32,
    /// Hasler and Süsstrunk colorfulness, roughly 0..150.
    pub colorfulness: f32,
    /// Share of pixels on strong, short horizontal edges (text, UI).
    pub text_ratio: f32,
    /// Share of pixels equal to the most common color (flat UI, documents).
    pub flat_ratio: f32,
}

fn luma(p: &image::Rgb<u8>) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

pub fn mean_luma(img: &RgbImage) -> f32 {
    let n = (img.width() * img.height()).max(1) as f32;
    img.pixels().map(luma).sum::<f32>() / n / 255.0
}

pub fn compute(img: &RgbImage) -> ImageStats {
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w < 3 || h < 3 {
        return ImageStats::default();
    }
    let l: Vec<f32> = img.pixels().map(luma).collect();
    let n = l.len() as f32;
    let mean = l.iter().sum::<f32>() / n;
    let var = l.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n;

    let mut lap = Vec::with_capacity((w - 2) * (h - 2));
    let mut text_edges = 0usize;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let c = l[y * w + x];
            let v = l[(y - 1) * w + x] + l[(y + 1) * w + x] + l[y * w + x - 1] + l[y * w + x + 1] - 4.0 * c;
            lap.push(v);
            // A sharp vertical step (left/right contrast) with flat rows above
            // and below is typical of glyph strokes.
            let dx = (l[y * w + x + 1] - l[y * w + x - 1]).abs();
            let dy = (l[(y + 1) * w + x] - l[(y - 1) * w + x]).abs();
            if dx > 80.0 && dy < 40.0 {
                text_edges += 1;
            }
        }
    }
    let lm = lap.iter().sum::<f32>() / lap.len() as f32;
    let sharpness = lap.iter().map(|v| (v - lm).powi(2)).sum::<f32>() / lap.len() as f32;

    let (mut rg_s, mut rg_ss, mut yb_s, mut yb_ss) = (0f32, 0f32, 0f32, 0f32);
    let mut hist = std::collections::HashMap::<u32, usize>::new();
    for p in img.pixels() {
        let (r, g, b) = (p[0] as f32, p[1] as f32, p[2] as f32);
        let rg = r - g;
        let yb = 0.5 * (r + g) - b;
        rg_s += rg;
        rg_ss += rg * rg;
        yb_s += yb;
        yb_ss += yb * yb;
        let key = ((p[0] as u32 >> 3) << 10) | ((p[1] as u32 >> 3) << 5) | (p[2] as u32 >> 3);
        *hist.entry(key).or_default() += 1;
    }
    let (rg_m, yb_m) = (rg_s / n, yb_s / n);
    let rg_sd = (rg_ss / n - rg_m * rg_m).max(0.0).sqrt();
    let yb_sd = (yb_ss / n - yb_m * yb_m).max(0.0).sqrt();
    let colorfulness = (rg_sd.powi(2) + yb_sd.powi(2)).sqrt() + 0.3 * (rg_m.powi(2) + yb_m.powi(2)).sqrt();
    let flat = hist.values().copied().max().unwrap_or(0) as f32 / n;

    ImageStats {
        brightness: mean / 255.0,
        contrast: var.sqrt() / 255.0,
        sharpness,
        colorfulness,
        text_ratio: text_edges as f32 / lap.len() as f32,
        flat_ratio: flat,
    }
}

/// Why an image looks like junk, if it does.
pub fn junk_reason(s: &ImageStats, w: u32, h: u32) -> Option<&'static str> {
    if w.max(h) < 200 {
        return Some("smaller than 200 px");
    }
    if s.brightness < 0.06 && s.contrast < 0.05 {
        return Some("almost black");
    }
    if s.brightness > 0.96 && s.contrast < 0.04 {
        return Some("almost white");
    }
    if s.brightness < 0.15 && s.sharpness < 15.0 {
        return Some("dark and blurred, like a pocket shot");
    }
    if s.sharpness < 6.0 && s.text_ratio < 0.002 {
        return Some("very blurry");
    }
    None
}

/// 64-bit difference hash on a 9×8 grayscale reduction.
pub fn dhash(img: &RgbImage) -> u64 {
    let small = image::imageops::resize(img, 9, 8, image::imageops::FilterType::Triangle);
    let mut bits = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            let a = luma(small.get_pixel(x, y));
            let b = luma(small.get_pixel(x + 1, y));
            bits = (bits << 1) | u64::from(a > b);
        }
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn junk_and_sharpness() {
        let black = RgbImage::from_pixel(256, 256, Rgb([3, 3, 3]));
        assert_eq!(junk_reason(&compute(&black), 256, 256), Some("almost black"));
        let mut checker = RgbImage::new(256, 256);
        for (x, y, p) in checker.enumerate_pixels_mut() {
            *p = if (x / 4 + y / 4) % 2 == 0 { Rgb([255, 255, 255]) } else { Rgb([0, 0, 0]) };
        }
        let s = compute(&checker);
        assert!(s.sharpness > 1000.0);
        assert!(junk_reason(&s, 256, 256).is_none());
        let blurred = image::imageops::blur(&checker, 12.0);
        assert!(compute(&blurred).sharpness < s.sharpness / 10.0);
        assert_eq!(junk_reason(&compute(&checker), 120, 90), Some("smaller than 200 px"));
    }

    #[test]
    fn dhash_is_stable_under_resize() {
        let mut img = RgbImage::new(200, 150);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgb([(x % 256) as u8, (y * 2 % 256) as u8, 90]);
        }
        let small = image::imageops::resize(&img, 100, 75, image::imageops::FilterType::Triangle);
        assert!((dhash(&img) ^ dhash(&small)).count_ones() <= 4);
    }
}
