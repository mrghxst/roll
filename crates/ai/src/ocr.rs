//! The text reader: PP-OCRv6 small (PaddleOCR) detection and recognition,
//! ONNX exports from the oar-ocr project. Detection gives a probability map
//! of text; connected regions above the threshold become line boxes, each
//! box is read by the recognizer (CTC, greedy). Boxes are axis-aligned,
//! which suits screens, documents and memes; strongly rotated text in
//! photos is read worse.

use std::collections::VecDeque;
use std::path::Path;

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, RgbImage};

use crate::onnx::Model;

/// Longest side the detector sees.
const DET_MAX: u32 = 960;
const DET_THRESH: f32 = 0.3;
const BOX_THRESH: f32 = 0.6;
const UNCLIP: f32 = 1.5;
const REC_HEIGHT: u32 = 48;
const REC_MAX_WIDTH: u32 = 1600;
const MIN_CONF: f32 = 0.5;

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub conf: f32,
    /// x, y, width, height in the image's pixels.
    pub rect: [u32; 4],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reading {
    pub lines: Vec<Line>,
    /// Share of the image covered by text boxes, 0–1.
    pub area: f32,
}

impl Reading {
    pub fn text(&self) -> String {
        self.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
    }
}

pub struct Reader {
    det: Model,
    rec: Model,
    chars: Vec<String>,
}

impl Reader {
    /// `dir` is the pack folder (det.onnx, rec.onnx, dict.txt).
    pub fn load(dir: &Path, gpu: bool) -> anyhow::Result<Self> {
        let det = Model::load(&dir.join("det.onnx"), gpu)?;
        let rec = Model::load(&dir.join("rec.onnx"), gpu)?;
        let dict = std::fs::read_to_string(dir.join("dict.txt"))?;
        let mut chars: Vec<String> = dict.lines().map(|l| l.trim_end_matches('\r').to_string()).collect();
        chars.push(" ".into());
        Ok(Reader { det, rec, chars })
    }

    pub fn read(&self, img: &DynamicImage) -> anyhow::Result<Reading> {
        let rgb = img.to_rgb8();
        let (w, h) = rgb.dimensions();
        if w < 8 || h < 8 {
            return Ok(Reading::default());
        }
        let boxes = self.detect(&rgb)?;
        let mut lines = Vec::new();
        let mut covered = 0u64;
        for b in boxes {
            covered += b[2] as u64 * b[3] as u64;
            let crop = image::imageops::crop_imm(&rgb, b[0], b[1], b[2], b[3]).to_image();
            if let Some((text, conf)) = self.recognize(&crop)?
                && conf >= MIN_CONF
                && !text.trim().is_empty()
            {
                lines.push(Line { text: text.trim().to_string(), conf, rect: b });
            }
        }
        Ok(Reading { lines, area: (covered as f32 / (w as f32 * h as f32)).min(1.0) })
    }

    /// Line boxes in reading order (top to bottom, then left to right).
    fn detect(&self, rgb: &RgbImage) -> anyhow::Result<Vec<[u32; 4]>> {
        let (w, h) = rgb.dimensions();
        let scale = (DET_MAX as f32 / w.max(h) as f32).min(1.0);
        let round32 = |v: f32| ((v / 32.0).round() as u32).max(1) * 32;
        let (dw, dh) = (round32(w as f32 * scale), round32(h as f32 * scale));
        let small = image::imageops::resize(rgb, dw, dh, FilterType::Triangle);
        // PaddleOCR trains on BGR images normalized with ImageNet's mean and
        // standard deviation, in that channel order.
        let (mean, std) = ([0.485f32, 0.456, 0.406], [0.229f32, 0.224, 0.225]);
        let n = (dw * dh) as usize;
        let mut input = vec![0f32; 3 * n];
        for (i, p) in small.pixels().enumerate() {
            for c in 0..3 {
                let v = p[2 - c] as f32 / 255.0;
                input[c * n + i] = (v - mean[c]) / std[c];
            }
        }
        let (shape, prob) = self.det.run_f32("x", vec![1, 3, dh as i64, dw as i64], input)?;
        let (ph, pw) = (shape[2] as usize, shape[3] as usize);
        let (sx, sy) = (w as f32 / pw as f32, h as f32 / ph as f32);
        let mut out = Vec::new();
        for (x0, y0, x1, y1, score) in regions(&prob, pw, ph, DET_THRESH) {
            if score < BOX_THRESH {
                continue;
            }
            let (bw, bh) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
            if bw.min(bh) < 3.0 {
                continue;
            }
            // Detection shrinks text regions; grow them back (DB's unclip).
            let d = bw * bh * UNCLIP / (2.0 * (bw + bh));
            let fx0 = ((x0 as f32 - d) * sx).max(0.0);
            let fy0 = ((y0 as f32 - d) * sy).max(0.0);
            let fx1 = (((x1 + 1) as f32 + d) * sx).min(w as f32);
            let fy1 = (((y1 + 1) as f32 + d) * sy).min(h as f32);
            let r = [fx0 as u32, fy0 as u32, (fx1 - fx0) as u32, (fy1 - fy0) as u32];
            if r[2] >= 4 && r[3] >= 4 {
                out.push(r);
            }
        }
        // Lines whose middles are within half a line height read as one row.
        out.sort_by_key(|r| (r[1] + r[3] / 2, r[0]));
        let mut rows: Vec<Vec<[u32; 4]>> = Vec::new();
        for r in out {
            let mid = r[1] + r[3] / 2;
            match rows.last_mut() {
                Some(row) if row.iter().any(|q| mid.abs_diff(q[1] + q[3] / 2) < q[3].min(r[3]) / 2) => row.push(r),
                _ => rows.push(vec![r]),
            }
        }
        Ok(rows
            .into_iter()
            .flat_map(|mut row| {
                row.sort_by_key(|r| r[0]);
                row
            })
            .collect())
    }

    fn recognize(&self, crop: &RgbImage) -> anyhow::Result<Option<(String, f32)>> {
        let (w, h) = crop.dimensions();
        if w == 0 || h == 0 {
            return Ok(None);
        }
        let rw = ((REC_HEIGHT as f32 * w as f32 / h as f32).ceil() as u32).clamp(8, REC_MAX_WIDTH);
        let img = image::imageops::resize(crop, rw, REC_HEIGHT, FilterType::Triangle);
        let n = (rw * REC_HEIGHT) as usize;
        let mut input = vec![0f32; 3 * n];
        for (i, p) in img.pixels().enumerate() {
            for c in 0..3 {
                input[c * n + i] = p[2 - c] as f32 / 127.5 - 1.0;
            }
        }
        let (shape, probs) = self.rec.run_f32("x", vec![1, 3, REC_HEIGHT as i64, rw as i64], input)?;
        let (t, k) = (shape[1] as usize, shape[2] as usize);
        Ok(decode(&probs, t, k, &self.chars))
    }
}

/// Greedy CTC: the best class per step, repeats merged, blanks (0) dropped.
pub fn decode(probs: &[f32], steps: usize, classes: usize, chars: &[String]) -> Option<(String, f32)> {
    let mut text = String::new();
    let mut confs = Vec::new();
    let mut last = 0usize;
    for s in 0..steps {
        let row = &probs[s * classes..(s + 1) * classes];
        let (best, p) = row.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(i, p)| (i, *p))?;
        if best != 0
            && best != last
            && let Some(ch) = chars.get(best - 1)
        {
            text.push_str(ch);
            confs.push(p);
        }
        last = best;
    }
    (!confs.is_empty()).then(|| (text, confs.iter().sum::<f32>() / confs.len() as f32))
}

/// Connected regions (8-neighbourhood) of `prob > thresh`: bounding box and
/// the mean probability inside the region.
fn regions(prob: &[f32], w: usize, h: usize, thresh: f32) -> Vec<(usize, usize, usize, usize, f32)> {
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    let mut queue = VecDeque::new();
    for start in 0..w * h {
        if seen[start] || prob[start] <= thresh {
            continue;
        }
        seen[start] = true;
        queue.push_back(start);
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        let (mut sum, mut n) = (0f32, 0usize);
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            sum += prob[i];
            n += 1;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if !seen[j] && prob[j] > thresh {
                        seen[j] = true;
                        queue.push_back(j);
                    }
                }
            }
        }
        out.push((x0, y0, x1, y1, sum / n as f32));
    }
    out
}

/// Whether `img` is worth reading at all: very small images are not.
pub fn readable(img: &DynamicImage) -> bool {
    let (w, h) = img.dimensions();
    w.min(h) >= 32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctc_merges_repeats_and_drops_blanks() {
        let chars: Vec<String> = ["a", "b", " "].iter().map(|s| s.to_string()).collect();
        // Steps: a a blank a b space b → "aab b"
        let seq = [1usize, 1, 0, 1, 2, 3, 2];
        let mut probs = vec![0.0f32; seq.len() * 4];
        for (s, c) in seq.iter().enumerate() {
            probs[s * 4 + c] = 0.9;
        }
        let (text, conf) = decode(&probs, seq.len(), 4, &chars).unwrap();
        assert_eq!(text, "aab b");
        assert!((conf - 0.9).abs() < 1e-6);
    }

    #[test]
    fn regions_find_separate_blobs() {
        let (w, h) = (10, 4);
        let mut p = vec![0.0f32; w * h];
        for x in 1..4 {
            p[w + x] = 0.9;
        }
        for x in 6..9 {
            p[2 * w + x] = 0.8;
        }
        let r = regions(&p, w, h, 0.3);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].0, r[0].1, r[0].2, r[0].3), (1, 1, 3, 1));
    }
}
