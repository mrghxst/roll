//! Open or closed eyes (best shots): on a face YuNet found, MediaPipe's
//! Face Mesh V2 places 478 landmarks, and MediaPipe's blendshape model
//! turns 146 of them into ARKit-style scores, of which `eyeBlinkLeft` and
//! `eyeBlinkRight` say how closed each eye is (0 open, 1 closed). Both
//! models are Google's (Apache-2.0), converted to ONNX with a parity check
//! against MediaPipe by the QtMeshEditor project. The crop follows
//! MediaPipe's face landmarker: a square 1.5 times the face, turned so the
//! eyes are level, at 256 × 256.

use std::path::Path;

use image::RgbImage;

use crate::faces::{Face, bilinear};
use crate::onnx::Model;

const CROP: u32 = 256;
/// MediaPipe's `face_blendshapes_graph.cc`: the landmarks the blendshape
/// model reads, in its order.
const SUBSET: [usize; 146] = [
    0, 1, 4, 5, 6, 7, 8, 10, 13, 14, 17, 21, 33, 37, 39, 40, 46, 52, 53, 54, 55, 58, 61, 63, 65, 66, 67, 70, 78, 80, 81, 82, 84, 87, 88,
    91, 93, 95, 103, 105, 107, 109, 127, 132, 133, 136, 144, 145, 146, 148, 149, 150, 152, 153, 154, 155, 157, 158, 159, 160, 161, 162,
    163, 168, 172, 173, 176, 178, 181, 185, 191, 195, 197, 234, 246, 249, 251, 263, 267, 269, 270, 276, 282, 283, 284, 285, 288, 291, 293,
    295, 296, 297, 300, 308, 310, 311, 312, 314, 317, 318, 321, 323, 324, 332, 334, 336, 338, 356, 361, 362, 365, 373, 374, 375, 377, 378,
    379, 380, 381, 382, 384, 385, 386, 387, 388, 389, 390, 397, 398, 400, 402, 405, 409, 415, 454, 466, 468, 469, 470, 471, 472, 473, 474,
    475, 476, 477,
];
/// `eyeBlinkLeft` and `eyeBlinkRight` among the 52 blendshapes.
const BLINK: [usize; 2] = [9, 10];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EyeState {
    /// How closed each eye is, 0 (open) to 1 (closed).
    pub blink: [f32; 2],
    /// How sure the mesh model is that it sees a face, 0 to 1.
    pub presence: f32,
}

impl EyeState {
    /// The eyes are closed: the two blink scores average [`CLOSED`] or
    /// more, on a face the mesh model is sure of.
    pub fn closed(&self) -> bool {
        self.presence >= 0.5 && (self.blink[0] + self.blink[1]) / 2.0 >= CLOSED
    }
}

/// On the CC0 eye set (`crates/core/tests/eyes_real.rs`, 99 faces labelled
/// by eye) this finds 41 of 49 closed and calls 1 of 50 open closed (a
/// broad smile behind glasses). 0.45 finds 43 but calls 2 open closed;
/// the smaller of either eye's score finds only 33.
pub const CLOSED: f32 = 0.5;

pub struct Eyes {
    mesh: Model,
    shapes: Model,
    mesh_in: String,
    shapes_in: String,
}

impl Eyes {
    /// `dir` is the pack folder (face_landmarks.onnx, face_blendshapes.onnx).
    pub fn load(dir: &Path, gpu: bool) -> anyhow::Result<Self> {
        let mesh = Model::load(&dir.join("face_landmarks.onnx"), gpu)?;
        // A small MLP; the CPU is quicker than a trip to the card.
        let shapes = Model::load(&dir.join("face_blendshapes.onnx"), false)?;
        let (mesh_in, shapes_in) = (mesh.input_name()?, shapes.input_name()?);
        Ok(Eyes { mesh, shapes, mesh_in, shapes_in })
    }

    /// The eyes of `face` in `img`.
    pub fn state(&self, img: &RgbImage, face: &Face) -> anyhow::Result<EyeState> {
        let (pts, presence) = self.landmarks(img, face)?;
        let sub: Vec<f32> = SUBSET.iter().flat_map(|&i| pts[i]).collect();
        let (_, shapes) = self.shapes.run_f32(&self.shapes_in, vec![1, 146, 2], sub)?;
        let blink = BLINK.map(|i| shapes.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0));
        Ok(EyeState { blink, presence })
    }

    /// The mesh's 478 points in the image's pixels, and its presence score.
    pub fn landmarks(&self, img: &RgbImage, face: &Face) -> anyhow::Result<(Vec<[f32; 2]>, f32)> {
        let rect = roi(face);
        let crop = crop(img, &rect);
        let plane = (CROP * CROP) as usize;
        let mut data = Vec::with_capacity(3 * plane);
        for p in crop.pixels() {
            data.extend(p.0.iter().map(|&c| c as f32 / 255.0));
        }
        let outs = self.mesh.run_all(&self.mesh_in, vec![1, CROP as i64, CROP as i64, 3], data)?;
        let marks =
            outs.iter().find(|(_, v)| v.len() == 478 * 3).map(|(_, v)| v.as_slice()).ok_or_else(|| anyhow::anyhow!("no landmarks"))?;
        let presence = outs.iter().find(|(_, v)| v.len() == 1).map_or(1.0, |(_, v)| sigmoid(v[0]));
        let pts = marks.as_chunks::<3>().0.iter().map(|p| project(&rect, p[0] / CROP as f32, p[1] / CROP as f32)).collect();
        Ok((pts, presence))
    }
}

/// A rotated square: centre, side and angle (radians, y down).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    cx: f32,
    cy: f32,
    side: f32,
    angle: f32,
}

/// MediaPipe's face rect: the face box made square at 1.5 times its long
/// side, turned so the line from the first eye to the second is level.
fn roi(face: &Face) -> Rect {
    let [x, y, w, h] = face.rect;
    let [a, b] = [face.landmarks[0], face.landmarks[1]];
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    Rect { cx: x + w / 2.0, cy: y + h / 2.0, side: w.max(h) * 1.5, angle: normalize(-(-dy).atan2(dx)) }
}

fn normalize(a: f32) -> f32 {
    use std::f32::consts::PI;
    a - 2.0 * PI * ((a + PI) / (2.0 * PI)).floor()
}

/// A point of the crop (0–1 on both axes) in the image.
fn project(r: &Rect, u: f32, v: f32) -> [f32; 2] {
    let (x, y) = ((u - 0.5) * r.side, (v - 0.5) * r.side);
    let (c, s) = (r.angle.cos(), r.angle.sin());
    [r.cx + x * c - y * s, r.cy + x * s + y * c]
}

/// The square the mesh model sees, for checking by eye.
pub fn face_crop(img: &RgbImage, face: &Face) -> RgbImage {
    crop(img, &roi(face))
}

fn crop(img: &RgbImage, r: &Rect) -> RgbImage {
    let (w, h) = img.dimensions();
    RgbImage::from_fn(CROP, CROP, |u, v| {
        let [x, y] = project(r, u as f32 / CROP as f32, v as f32 / CROP as f32);
        bilinear(img, x, y, w, h)
    })
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x.clamp(-50.0, 50.0)).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(eyes: [[f32; 2]; 2]) -> Face {
        Face {
            rect: [100.0, 100.0, 100.0, 120.0],
            score: 0.9,
            landmarks: [eyes[0], eyes[1], [150.0, 160.0], [130.0, 190.0], [170.0, 190.0]],
        }
    }

    #[test]
    fn level_eyes_give_an_upright_square() {
        let r = roi(&face([[130.0, 140.0], [170.0, 140.0]]));
        assert_eq!((r.cx, r.cy, r.side), (150.0, 160.0, 180.0));
        assert!(r.angle.abs() < 1e-6);
        // The crop's corners and middle land where they should.
        assert_eq!(project(&r, 0.5, 0.5), [150.0, 160.0]);
        assert_eq!(project(&r, 0.0, 0.0), [60.0, 70.0]);
    }

    #[test]
    fn a_tilted_face_is_turned_level() {
        // The second eye lower than the first: the head leans, and the crop
        // turns with it (positive angle, y down), so its x axis runs along
        // the eyes.
        let r = roi(&face([[130.0, 140.0], [170.0, 180.0]]));
        assert!((r.angle - std::f32::consts::FRAC_PI_4).abs() < 1e-5, "{}", r.angle);
        let [x0, y0] = project(&r, 0.4, 0.5);
        let [x1, y1] = project(&r, 0.6, 0.5);
        assert!(((y1 - y0) / (x1 - x0) - 1.0).abs() < 1e-4);
    }
}
