//! Thumbnails (256 px on the short side, WebP), viewer previews (1024 px
//! on the long side), ThumbHash placeholders, dHash and statistics, all
//! from one decode.

use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer, images::Image};
use image::RgbImage;

use crate::stats::{self, ImageStats};

pub const THUMB_SHORT: u32 = 256;
pub const THUMB_LONG_MAX: u32 = 640;
pub const PREVIEW_LONG: u32 = 1024;

pub fn resize(img: &RgbImage, w: u32, h: u32) -> RgbImage {
    let src = Image::from_vec_u8(img.width(), img.height(), img.as_raw().clone(), PixelType::U8x3).expect("valid source");
    let mut dst = Image::new(w, h, PixelType::U8x3);
    let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Bilinear));
    Resizer::new().resize(&src, &mut dst, &opts).expect("same pixel types");
    RgbImage::from_raw(w, h, dst.into_vec()).expect("buffer size")
}

/// Size for the grid thumbnail: short side 256, long side at most 640.
pub fn thumb_size(w: u32, h: u32) -> (u32, u32) {
    let (short, long) = (w.min(h).max(1) as f64, w.max(h).max(1) as f64);
    let mut s = (THUMB_SHORT as f64 / short).min(1.0);
    if long * s > THUMB_LONG_MAX as f64 {
        s = THUMB_LONG_MAX as f64 / long;
    }
    (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1))
}

pub fn webp(img: &RgbImage, quality: f32) -> Vec<u8> {
    // libwebp sets up its encoder routines (function pointers chosen by
    // CPU) on first use, without a lock on Windows. Encodes starting on
    // several threads at once could run with pointers another thread is
    // rewriting, which crashed in VP8PutBit about once in 50 test runs.
    // One encode before any parallel ones finishes the setup.
    static READY: std::sync::Once = std::sync::Once::new();
    READY.call_once(|| {
        let warm = RgbImage::from_pixel(64, 48, image::Rgb([128, 96, 64]));
        let _ = webp::Encoder::from_rgb(warm.as_raw(), 64, 48).encode(quality);
    });
    webp::Encoder::from_rgb(img.as_raw(), img.width(), img.height()).encode(quality).to_vec()
}

#[derive(Debug, Clone)]
pub struct Thumbs {
    pub webp: Vec<u8>,
    pub thumb_w: u32,
    pub thumb_h: u32,
    /// Size of the decoded, upright image.
    pub source_w: u32,
    pub source_h: u32,
    pub thumbhash: Vec<u8>,
    /// Average color as 0xRRGGBB.
    pub avg_rgb: u32,
    pub stats: ImageStats,
    pub dhash: u64,
}

pub fn make(img: &RgbImage) -> Thumbs {
    let (tw, th) = thumb_size(img.width(), img.height());
    let small = if (tw, th) == img.dimensions() { img.clone() } else { resize(img, tw, th) };
    let s = 100.0 / tw.max(th) as f64;
    let (hw, hh) = (((tw as f64 * s).round() as u32).clamp(1, 100), ((th as f64 * s).round() as u32).clamp(1, 100));
    let tiny = resize(&small, hw, hh);
    let rgba: Vec<u8> = tiny.pixels().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    let thumbhash = thumbhash::rgba_to_thumb_hash(hw as usize, hh as usize, &rgba);
    let n = (hw * hh) as u64;
    let (r, g, b) = tiny.pixels().fold((0u64, 0u64, 0u64), |(r, g, b), p| (r + p[0] as u64, g + p[1] as u64, b + p[2] as u64));
    let avg_rgb = (((r / n) as u32) << 16) | (((g / n) as u32) << 8) | (b / n) as u32;
    Thumbs {
        webp: webp(&small, 76.0),
        thumb_w: tw,
        thumb_h: th,
        source_w: img.width(),
        source_h: img.height(),
        thumbhash,
        avg_rgb,
        stats: stats::compute(&small),
        dhash: stats::dhash(&small),
    }
}

/// The 1024 px viewer preview.
pub fn preview(img: &RgbImage) -> Vec<u8> {
    webp(&crate::decode::fit(img.clone(), PREVIEW_LONG), 80.0)
}

/// What a decision model sees: at most 512 px, JPEG quality 80. The image
/// crate's encoder writes no EXIF, so no metadata goes along.
pub fn ai_jpeg(img: &RgbImage) -> Vec<u8> {
    let small = crate::decode::fit(img.clone(), 512);
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80).encode_image(&small).expect("jpeg encode");
    out
}

/// Video keyframes side by side, scaled to a common height.
pub fn strip(frames: &[RgbImage]) -> Option<RgbImage> {
    let h = frames.iter().map(|f| f.height()).min()?;
    let scaled: Vec<RgbImage> = frames.iter().map(|f| resize(f, (f.width() * h / f.height().max(1)).max(1), h)).collect();
    let w = scaled.iter().map(|f| f.width()).sum();
    let mut out = RgbImage::new(w, h);
    let mut x = 0;
    for f in &scaled {
        image::imageops::replace(&mut out, f, x as i64, 0);
        x += f.width();
    }
    Some(out)
}

/// A full-size JPEG for formats the WebView cannot show (HEIC, RAW).
pub fn full_jpeg(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).encode_image(img).expect("jpeg encode");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(thumb_size(4032, 3024), (341, 256));
        assert_eq!(thumb_size(1170, 2532), (256, 554));
        assert_eq!(thumb_size(4000, 1000), (640, 160));
        assert_eq!(thumb_size(120, 90), (120, 90));
    }

    #[test]
    fn makes_small_webp() {
        let img = image::load_from_memory(&crate::testutil::jpeg_bytes(1600, 1200)).unwrap().to_rgb8();
        let t = make(&img);
        assert_eq!((t.thumb_w, t.thumb_h), (341, 256));
        assert!(t.webp.len() < 40_000 && t.webp.starts_with(b"RIFF"));
        assert!(!t.thumbhash.is_empty());
        assert!(!preview(&img).is_empty());
    }
}
