//! The best shots of a series (M7 owner request): bursts and look-alike
//! groups ranked by picture quality, measured on the 1024 px previews.
//!
//! - Sharpness: variance of the Laplacian (motion blur and missed focus
//!   lower it), compared within the series.
//! - Exposure: brightness near the middle and few clipped pixels.
//! - Faces, with the faces pack: how sharp the faces are (on their aligned
//!   crops), which counts as much as the whole picture's sharpness when
//!   two or more frames have faces.
//! - Closed eyes, with the eyes pack: a frame where someone's eyes are
//!   closed loses about as much as a clearly softer frame, more the more
//!   faces blink.
//! - Look, with the image model: how much more the picture resembles "a
//!   sharp, well exposed photo" than "a blurry, badly exposed photo", and
//!   (without the eyes pack) open rather than closed eyes. A rough judge,
//!   so it weighs least.
//!
//! Nothing is re-encoded; the ranking only suggests which to keep.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ShotScore {
    pub id: i32,
    /// 0–1 within the series.
    pub score: f32,
    /// Suggested to keep (the best one, and a second that is nearly as good).
    pub keep: bool,
    /// "Sharpest", "Best exposed", "Looks best", "Eyes closed", "Eyes may
    /// be closed".
    pub notes: Vec<String>,
}

/// What was measured for one picture.
#[derive(Debug, Clone, Default)]
pub struct Measured {
    pub id: i64,
    pub sharpness: f32,
    pub brightness: f32,
    /// Share of pixels at the black or white end.
    pub clipped: f32,
    /// Good minus bad description similarity, with the image model.
    pub look: Option<f32>,
    /// Open minus closed eyes similarity, with the image model.
    pub eyes: Option<f32>,
    /// Mean sharpness of the faces in it, with the faces pack.
    pub faces: Option<f32>,
    /// Faces with closed eyes and faces checked, with the eyes pack.
    pub closed_eyes: Option<(u32, u32)>,
}

pub const GOOD: &[&str] = &["a sharp, well exposed photo", "a beautiful photo", "a photo where everyone looks at the camera"];
pub const BAD: &[&str] = &["a blurry photo", "an out of focus photo", "a badly exposed, dark photo", "an overexposed photo"];
pub const EYES_OPEN: &str = "a photo of a person with open eyes";
pub const EYES_CLOSED: &str = "a photo of a person with closed eyes";

/// Share of clipped pixels and mean brightness of an image.
pub fn exposure_of(img: &image::RgbImage) -> (f32, f32) {
    let n = (img.width() * img.height()).max(1) as f32;
    let (mut sum, mut clipped) = (0f32, 0u32);
    for p in img.pixels() {
        let l = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
        sum += l;
        if !(3.0..=252.0).contains(&l) {
            clipped += 1;
        }
    }
    (sum / n / 255.0, clipped as f32 / n)
}

fn exposure(m: &Measured) -> f32 {
    (1.0 - (m.brightness - 0.5).abs() * 1.6 - m.clipped * 3.0).clamp(0.0, 1.0)
}

/// Rank a series, best first.
pub fn rank(series: &[Measured]) -> Vec<ShotScore> {
    if series.is_empty() {
        return Vec::new();
    }
    let max_sharp = series.iter().map(|m| m.sharpness).fold(f32::MIN, f32::max).max(1e-6);
    let looks: Vec<f32> = series.iter().filter_map(|m| m.look).collect();
    let (lo, hi) = (looks.iter().copied().fold(f32::MAX, f32::min), looks.iter().copied().fold(f32::MIN, f32::max));
    let spread = hi - lo;
    let mean_look = looks.iter().sum::<f32>() / looks.len().max(1) as f32;
    let with_faces = series.iter().filter(|m| m.faces.is_some()).count();
    let max_face = series.iter().filter_map(|m| m.faces).fold(f32::MIN, f32::max).max(1e-6);
    let mut out: Vec<(f32, &Measured)> = series
        .iter()
        .map(|m| {
            // Relative to the sharpest frame; the square root keeps a
            // somewhat softer frame from dropping to nothing.
            let mut sharp = (m.sharpness.max(0.0) / max_sharp).sqrt();
            // Faces in focus matter more than the background.
            if with_faces >= 2 {
                sharp = 0.5 * sharp + 0.5 * m.faces.map_or(0.0, |f| (f.max(0.0) / max_face).sqrt());
            }
            // On an absolute scale around the series' mean: a difference
            // of 0.01 in similarity moves it by 0.2.
            let look = m.look.map_or(0.5, |l| (0.5 + (l - mean_look) * 20.0).clamp(0.0, 1.0));
            // Only a clear "closed eyes" counts: without people the two
            // descriptions differ by noise.
            // The eyes pack, when it looked, replaces that guess: one
            // blink costs 0.15, everyone 0.3.
            let eyes = match m.closed_eyes {
                Some((0, _)) => 0.0,
                Some((closed, seen)) => -0.15 - 0.15 * closed as f32 / seen.max(1) as f32,
                None => m.eyes.filter(|e| *e < -0.01).map_or(0.0, |e| (e * 10.0).max(-0.15)),
            };
            ((0.5 * sharp + 0.2 * exposure(m) + 0.3 * look + eyes).clamp(0.0, 1.0), m)
        })
        .collect();
    out.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    let best = out[0].0;
    let sharpest = series.iter().map(|m| m.sharpness).fold(f32::MIN, f32::max);
    let exposed = series.iter().map(exposure).fold(f32::MIN, f32::max);
    out.iter()
        .enumerate()
        .map(|(i, (s, m))| {
            let mut notes = Vec::new();
            if m.sharpness >= sharpest && series.len() > 1 {
                notes.push("Sharpest".to_string());
            }
            if exposure(m) >= exposed && series.len() > 1 {
                notes.push("Best exposed".to_string());
            }
            if with_faces >= 2 && m.faces.is_some_and(|f| f >= max_face) {
                notes.push("Sharpest faces".to_string());
            }
            if spread > 0.004 && m.look == Some(hi) {
                notes.push("Looks best".to_string());
            }
            if m.closed_eyes.is_some_and(|c| c.0 > 0) {
                notes.push("Eyes closed".to_string());
            } else if m.closed_eyes.is_none() && m.eyes.is_some_and(|e| e < -0.01) {
                notes.push("Eyes may be closed".to_string());
            }
            ShotScore { id: m.id as i32, score: *s, keep: i == 0 || (i == 1 && *s >= best - 0.05 && series.len() > 3), notes }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: i64, sharpness: f32, brightness: f32, look: f32) -> Measured {
        Measured { id, sharpness, brightness, clipped: 0.0, look: Some(look), eyes: None, faces: None, closed_eyes: None }
    }

    #[test]
    fn a_blink_loses_to_a_softer_frame() {
        // Frame 1 is the sharpest, but one of two people blinked in it.
        let mut a = m(1, 400.0, 0.5, 0.02);
        let mut b = m(2, 300.0, 0.5, 0.02);
        let mut c = m(3, 200.0, 0.5, 0.02);
        a.closed_eyes = Some((1, 2));
        b.closed_eyes = Some((0, 2));
        c.closed_eyes = Some((0, 2));
        // A guess from the image model does not count when the eyes pack looked.
        b.eyes = Some(-0.05);
        let r = rank(&[a, b, c]);
        assert_eq!(r.iter().map(|s| s.id).collect::<Vec<_>>(), vec![2, 3, 1]);
        assert_eq!(r[2].notes, vec!["Sharpest", "Best exposed", "Eyes closed"]);
        assert!(!r[0].notes.iter().any(|n| n.starts_with("Eyes")));
    }

    #[test]
    fn sharp_faces_beat_a_sharp_background() {
        // Frame 1 is crisp overall but its face is blurred; frame 2's face is sharp.
        let mut a = m(1, 400.0, 0.5, 0.02);
        let mut b = m(2, 330.0, 0.5, 0.02);
        a.faces = Some(30.0);
        b.faces = Some(300.0);
        let r = rank(&[a, b]);
        assert_eq!(r[0].id, 2);
        assert!(r[0].notes.contains(&"Sharpest faces".to_string()));
    }

    #[test]
    fn sharp_well_exposed_frames_win() {
        let r = rank(&[m(1, 40.0, 0.5, 0.01), m(2, 400.0, 0.48, 0.03), m(3, 380.0, 0.9, 0.02), m(4, 90.0, 0.5, 0.0)]);
        assert_eq!(r[0].id, 2);
        assert!(r[0].keep && r[0].notes.contains(&"Sharpest".to_string()));
        assert_eq!(r.last().unwrap().id, 1, "the blurriest last");
        assert_eq!(r.iter().filter(|s| s.keep).count(), 1, "the second is not close enough");
        let one = rank(&[m(7, 10.0, 0.5, 0.0)]);
        assert!(one[0].keep && one[0].notes.is_empty());
    }

    #[test]
    fn closed_eyes_count_against() {
        let mut a = m(1, 300.0, 0.5, 0.02);
        let mut b = m(2, 300.0, 0.5, 0.02);
        a.eyes = Some(-0.03);
        b.eyes = Some(0.02);
        let r = rank(&[a, b]);
        assert_eq!(r[0].id, 2);
        assert!(r[1].notes.contains(&"Eyes may be closed".to_string()));
    }
}
