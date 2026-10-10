//! Decoding any supported file to RGB, upright. The image crate handles
//! JPEG, PNG, GIF, WebP, TIFF and BMP; FFmpeg handles HEIC, AVIF, JPEG XL
//! and video; RAW files use their embedded preview.

use std::path::Path;

use image::{imageops, ImageReader, RgbImage};

use crate::ffmpeg::{Ffmpeg, FfmpegError};
use crate::raw;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("{0}")]
    Image(#[from] image::ImageError),
    #[error(transparent)]
    Ffmpeg(#[from] FfmpegError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("This format needs FFmpeg, which was not found")]
    NeedsFfmpeg,
}

#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    pub path: &'a Path,
    pub format: &'a str,
    pub video: bool,
    pub orientation: Option<u16>,
    pub duration_ms: Option<u64>,
}

/// Apply an EXIF orientation (1–8).
pub fn orient(img: RgbImage, orientation: Option<u16>) -> RgbImage {
    match orientation.unwrap_or(1) {
        2 => imageops::flip_horizontal(&img),
        3 => imageops::rotate180(&img),
        4 => imageops::flip_vertical(&img),
        5 => imageops::flip_horizontal(&imageops::rotate90(&img)),
        6 => imageops::rotate90(&img),
        7 => imageops::flip_horizontal(&imageops::rotate270(&img)),
        8 => imageops::rotate270(&img),
        _ => img,
    }
}

fn decode_with_image(path: &Path) -> Result<RgbImage, DecodeError> {
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    reader.no_limits();
    Ok(reader.decode()?.to_rgb8())
}

fn decode_bytes(bytes: &[u8]) -> Result<RgbImage, DecodeError> {
    Ok(image::load_from_memory(bytes)?.to_rgb8())
}

/// Shrink so the long side is at most `max` (never enlarges).
pub fn fit(img: RgbImage, max: u32) -> RgbImage {
    let (w, h) = img.dimensions();
    if w.max(h) <= max {
        return img;
    }
    let s = max as f64 / w.max(h) as f64;
    crate::thumb::resize(&img, ((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1))
}

pub fn decode(src: Source, ffmpeg: Option<&Ffmpeg>, max_side: Option<u32>) -> Result<RgbImage, DecodeError> {
    let ff = || ffmpeg.ok_or(DecodeError::NeedsFfmpeg);
    let img = if src.video {
        ff()?.poster(src.path, src.duration_ms, max_side)?
    } else {
        match src.format {
            "jpeg" | "png" | "gif" | "webp" | "tiff" | "bmp" => orient(decode_with_image(src.path)?, src.orientation),
            // FFmpeg applies the HEIF rotation and mirror itself; EXIF
            // orientation inside HEIF is informative only.
            "heic" | "avif" | "jxl" => ff()?.frame(src.path, 0.0, max_side)?,
            "raw" => {
                let data = std::fs::read(src.path)?;
                match raw::largest_embedded_jpeg(&data).map(decode_bytes) {
                    Some(Ok(img)) => orient(img, src.orientation),
                    _ => match decode_with_image(src.path) {
                        Ok(img) => orient(img, src.orientation),
                        Err(_) => ff()?.frame(src.path, 0.0, max_side)?,
                    },
                }
            }
            _ => match decode_with_image(src.path) {
                Ok(img) => img,
                Err(_) => ff()?.frame(src.path, 0.0, max_side)?,
            },
        }
    };
    Ok(match max_side {
        Some(m) => fit(img, m),
        None => img,
    })
}

/// Whether the WebView can show the original file directly.
pub fn browser_native(format: &str) -> bool {
    matches!(format, "jpeg" | "png" | "gif" | "webp" | "bmp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_swaps_dimensions() {
        let img = RgbImage::new(40, 20);
        for o in 5..=8 {
            assert_eq!(orient(img.clone(), Some(o)).dimensions(), (20, 40));
        }
        for o in 1..=4 {
            assert_eq!(orient(img.clone(), Some(o)).dimensions(), (40, 20));
        }
    }

    #[test]
    fn orientation_6_rotates_clockwise() {
        let mut img = RgbImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        let r = orient(img, Some(6));
        // The left pixel ends up at the top.
        assert_eq!(r.get_pixel(0, 0)[0], 255);
    }

    #[test]
    fn decodes_jpeg_and_fits() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.jpg");
        std::fs::write(&p, crate::testutil::jpeg_bytes(800, 600)).unwrap();
        let src = Source { path: &p, format: "jpeg", video: false, orientation: Some(6), duration_ms: None };
        let img = decode(src, None, Some(400)).unwrap();
        assert_eq!(img.dimensions(), (300, 400));
    }
}
