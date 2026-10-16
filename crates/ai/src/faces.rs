//! Faces (PROMPT M7, People): YuNet (OpenCV Zoo, MIT) finds faces and five
//! landmarks; each face is aligned to 112 px from its landmarks and SFace
//! (OpenCV Zoo, Apache-2.0) turns it into 128 numbers. Two faces of the
//! same person are usually 0.363 or more alike (OpenCV's threshold).

use std::path::Path;

use image::imageops::FilterType;
use image::{GrayImage, RgbImage};

use crate::onnx::Model;

/// The detector's fixed input size.
const DET: u32 = 640;
const SCORE: f32 = 0.8;
const NMS_IOU: f32 = 0.3;
/// Faces narrower than this (in the image's pixels) are kept but not
/// recognized: too few pixels to tell people apart.
pub const MIN_RECOGNIZE: f32 = 36.0;
/// Where SFace expects the eyes, nose and mouth corners in 112 × 112.
const TEMPLATE: [[f32; 2]; 5] = [[38.2946, 51.6963], [73.5318, 51.5014], [56.0252, 71.7366], [41.5493, 92.3655], [70.7299, 92.2041]];

#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// x, y, width, height in the image's pixels.
    pub rect: [f32; 4],
    pub score: f32,
    /// Eyes (left, right as seen), nose, mouth corners (left, right).
    pub landmarks: [[f32; 2]; 5],
}

pub struct Faces {
    det: Model,
    rec: Model,
}

impl Faces {
    /// `dir` is the pack folder (yunet.onnx, sface.onnx).
    pub fn load(dir: &Path, gpu: bool) -> anyhow::Result<Self> {
        // The detector is tiny; the CPU avoids a round trip to the card.
        Ok(Faces { det: Model::load(&dir.join("yunet.onnx"), false)?, rec: Model::load(&dir.join("sface.onnx"), gpu)? })
    }

    /// Faces in `img`, best first.
    pub fn detect(&self, img: &RgbImage) -> anyhow::Result<Vec<Face>> {
        let (w, h) = img.dimensions();
        if w < 16 || h < 16 {
            return Ok(Vec::new());
        }
        // Letterbox: scale the long side to 640, pad right and bottom.
        let scale = DET as f32 / w.max(h) as f32;
        let (sw, sh) = (((w as f32 * scale).round() as u32).clamp(1, DET), ((h as f32 * scale).round() as u32).clamp(1, DET));
        let small = image::imageops::resize(img, sw, sh, FilterType::Triangle);
        let plane = (DET * DET) as usize;
        let mut data = vec![0f32; 3 * plane];
        for (x, y, p) in small.enumerate_pixels() {
            let i = (y * DET + x) as usize;
            // BGR, 0–255, as OpenCV feeds it.
            data[i] = p[2] as f32;
            data[plane + i] = p[1] as f32;
            data[2 * plane + i] = p[0] as f32;
        }
        let outs = self.det.run_all("input", vec![1, 3, DET as i64, DET as i64], data)?;
        let get = |name: &str| outs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_slice()).unwrap_or(&[]);
        let mut found = Vec::new();
        for stride in [8u32, 16, 32] {
            let (cls, obj, bbox, kps) = (
                get(&format!("cls_{stride}")),
                get(&format!("obj_{stride}")),
                get(&format!("bbox_{stride}")),
                get(&format!("kps_{stride}")),
            );
            let cols = DET / stride;
            for (i, (c, o)) in cls.iter().zip(obj).enumerate() {
                let score = (c.clamp(0.0, 1.0) * o.clamp(0.0, 1.0)).sqrt();
                if score < SCORE || bbox.len() < (i + 1) * 4 || kps.len() < (i + 1) * 10 {
                    continue;
                }
                let (col, row) = ((i as u32 % cols) as f32, (i as u32 / cols) as f32);
                let s = stride as f32;
                let b = &bbox[i * 4..i * 4 + 4];
                let (cx, cy) = ((col + b[0]) * s, (row + b[1]) * s);
                let (bw, bh) = (b[2].exp() * s, b[3].exp() * s);
                let k = &kps[i * 10..i * 10 + 10];
                let mut landmarks = [[0f32; 2]; 5];
                for (n, l) in landmarks.iter_mut().enumerate() {
                    *l = [(k[2 * n] + col) * s / scale, (k[2 * n + 1] + row) * s / scale];
                }
                let x0 = ((cx - bw / 2.0) / scale).max(0.0);
                let y0 = ((cy - bh / 2.0) / scale).max(0.0);
                let x1 = ((cx + bw / 2.0) / scale).min(w as f32);
                let y1 = ((cy + bh / 2.0) / scale).min(h as f32);
                if x1 - x0 < 4.0 || y1 - y0 < 4.0 {
                    continue;
                }
                found.push(Face { rect: [x0, y0, x1 - x0, y1 - y0], score, landmarks });
            }
        }
        Ok(nms(found))
    }

    /// The face's 128 numbers, unit length.
    pub fn embed(&self, img: &RgbImage, face: &Face) -> anyhow::Result<Vec<f32>> {
        let aligned = align(img, &face.landmarks);
        let plane = 112 * 112;
        let mut data = vec![0f32; 3 * plane];
        for (x, y, p) in aligned.enumerate_pixels() {
            let i = (y * 112 + x) as usize;
            // RGB, 0–255 (OpenCV swaps its BGR for this model).
            data[i] = p[0] as f32;
            data[plane + i] = p[1] as f32;
            data[2 * plane + i] = p[2] as f32;
        }
        let (_, v) = self.rec.run_f32("data", vec![1, 3, 112, 112], data)?;
        Ok(unit(v))
    }
}

/// Keeps the best of overlapping boxes.
fn nms(mut faces: Vec<Face>) -> Vec<Face> {
    faces.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Face> = Vec::new();
    for f in faces {
        if kept.iter().all(|k| iou(&k.rect, &f.rect) < NMS_IOU) {
            kept.push(f);
        }
    }
    kept
}

pub fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = (a[0] + a[2]).min(b[0] + b[2]);
    let y1 = (a[1] + a[3]).min(b[1] + b[3]);
    let inter = (x1 - x0).max(0.0) * (y1 - y0).max(0.0);
    let union = a[2] * a[3] + b[2] * b[3] - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

fn unit(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

/// The similarity transform (scale, rotation, shift) that best maps
/// `from` onto `to`, as [a, b, tx, ty]: x' = a·x − b·y + tx, y' = b·x + a·y + ty.
pub fn similarity(from: &[[f32; 2]; 5], to: &[[f32; 2]; 5]) -> [f32; 4] {
    let mean = |p: &[[f32; 2]; 5]| {
        let (sx, sy) = p.iter().fold((0.0, 0.0), |(x, y), q| (x + q[0], y + q[1]));
        [sx / 5.0, sy / 5.0]
    };
    let (mf, mt) = (mean(from), mean(to));
    let (mut num_a, mut num_b, mut den) = (0f32, 0f32, 0f32);
    for (f, t) in from.iter().zip(to) {
        let (x, y) = (f[0] - mf[0], f[1] - mf[1]);
        let (u, v) = (t[0] - mt[0], t[1] - mt[1]);
        num_a += x * u + y * v;
        num_b += x * v - y * u;
        den += x * x + y * y;
    }
    if den <= f32::EPSILON {
        return [1.0, 0.0, mt[0] - mf[0], mt[1] - mf[1]];
    }
    let (a, b) = (num_a / den, num_b / den);
    [a, b, mt[0] - (a * mf[0] - b * mf[1]), mt[1] - (b * mf[0] + a * mf[1])]
}

/// The face cut out and turned upright at 112 × 112, as SFace expects it.
pub fn align(img: &RgbImage, landmarks: &[[f32; 2]; 5]) -> RgbImage {
    let [a, b, tx, ty] = similarity(landmarks, &TEMPLATE);
    // Inverse: from the 112 px face back into the image.
    let d = a * a + b * b;
    let (ia, ib) = (a / d, b / d);
    let (w, h) = img.dimensions();
    RgbImage::from_fn(112, 112, |u, v| {
        let (u, v) = (u as f32 - tx, v as f32 - ty);
        let x = ia * u + ib * v;
        let y = -ib * u + ia * v;
        bilinear(img, x, y, w, h)
    })
}

pub(crate) fn bilinear(img: &RgbImage, x: f32, y: f32, w: u32, h: u32) -> image::Rgb<u8> {
    if x < 0.0 || y < 0.0 || x > (w - 1) as f32 || y > (h - 1) as f32 {
        return image::Rgb([0, 0, 0]);
    }
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let p = |xx, yy| img.get_pixel(xx, yy).0;
    let (p00, p10, p01, p11) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
    let mut out = [0u8; 3];
    for c in 0..3 {
        let top = p00[c] as f32 * (1.0 - fx) + p10[c] as f32 * fx;
        let bottom = p01[c] as f32 * (1.0 - fx) + p11[c] as f32 * fx;
        out[c] = (top * (1.0 - fy) + bottom * fy).round() as u8;
    }
    image::Rgb(out)
}

/// How sharp a face is: the variance of the Laplacian on its aligned
/// grey crop, so faces of any size compare.
pub fn sharpness(img: &RgbImage, face: &Face) -> f32 {
    let g: GrayImage = image::DynamicImage::ImageRgb8(align(img, &face.landmarks)).to_luma8();
    let (w, h) = g.dimensions();
    let mut vals = Vec::with_capacity((w * h) as usize);
    // The middle of the face; the corners are often background.
    for y in 20..h - 20 {
        for x in 20..w - 20 {
            let c = g.get_pixel(x, y)[0] as f32;
            let l = g.get_pixel(x - 1, y)[0] as f32
                + g.get_pixel(x + 1, y)[0] as f32
                + g.get_pixel(x, y - 1)[0] as f32
                + g.get_pixel(x, y + 1)[0] as f32
                - 4.0 * c;
            vals.push(l);
        }
    }
    let n = vals.len().max(1) as f32;
    let mean = vals.iter().sum::<f32>() / n;
    vals.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarity_recovers_a_known_transform() {
        // Scale 2, a quarter turn, shifted.
        let from = TEMPLATE;
        let to = from.map(|[x, y]| [-2.0 * y + 10.0, 2.0 * x + 5.0]);
        let [a, b, tx, ty] = similarity(&from, &to);
        assert!((a - 0.0).abs() < 1e-3 && (b - 2.0).abs() < 1e-3, "{a} {b}");
        assert!((tx - 10.0).abs() < 1e-2 && (ty - 5.0).abs() < 1e-2, "{tx} {ty}");
    }

    #[test]
    fn align_puts_the_landmarks_on_the_template() {
        // A face twice the template's size at (100, 50), drawn as dots.
        let lm = TEMPLATE.map(|[x, y]| [x * 2.0 + 100.0, y * 2.0 + 50.0]);
        let mut img = RgbImage::from_pixel(400, 400, image::Rgb([0, 0, 0]));
        for [x, y] in lm {
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    img.put_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32, image::Rgb([255, 255, 255]));
                }
            }
        }
        let a = align(&img, &lm);
        for [x, y] in TEMPLATE {
            assert!(a.get_pixel(x.round() as u32, y.round() as u32)[0] > 200, "landmark at {x},{y}");
        }
    }

    #[test]
    fn nms_keeps_the_best_of_overlaps() {
        let f = |x: f32, s: f32| Face { rect: [x, 0.0, 10.0, 10.0], score: s, landmarks: [[0.0; 2]; 5] };
        let kept = nms(vec![f(0.0, 0.85), f(1.0, 0.95), f(50.0, 0.9)]);
        assert_eq!(kept.iter().map(|k| k.score).collect::<Vec<_>>(), vec![0.95, 0.9]);
    }
}
