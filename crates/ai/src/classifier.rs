//! A linear classifier on image embeddings (logistic regression, L2), for
//! categories the owner teaches with examples. Small and quick: a few
//! hundred examples of 768 dimensions train in milliseconds.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Linear {
    pub w: Vec<f32>,
    pub b: f32,
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

impl Linear {
    /// Probability that `x` belongs.
    pub fn predict(&self, x: &[f32]) -> f32 {
        sigmoid(crate::vectors::dot(&self.w, x) + self.b)
    }
}

/// Trains on positives and negatives, each with a weight (unlabelled
/// library items count as weak negatives). Classes are balanced by weight.
pub fn train(pos: &[(&[f32], f32)], neg: &[(&[f32], f32)]) -> Option<Linear> {
    let dim = pos.first()?.0.len();
    if neg.is_empty() {
        return None;
    }
    let wp: f32 = pos.iter().map(|p| p.1).sum();
    let wn: f32 = neg.iter().map(|n| n.1).sum();
    // Each class carries half the total weight.
    let samples: Vec<(&[f32], f32, f32)> =
        pos.iter().map(|(x, w)| (*x, 1.0, w / wp * 0.5)).chain(neg.iter().map(|(x, w)| (*x, 0.0, w / wn * 0.5))).collect();
    let mut m = Linear { w: vec![0.0; dim], b: 0.0 };
    // Embeddings are unit vectors with small coordinates; a large step
    // and a little L2 converge in a few hundred full-batch steps.
    let (lr, l2) = (8.0f32, 1e-3f32);
    let mut grad = vec![0f32; dim];
    for _ in 0..400 {
        grad.iter_mut().for_each(|g| *g = 0.0);
        let mut gb = 0.0;
        for (x, y, w) in &samples {
            let e = (m.predict(x) - y) * w;
            for (g, xi) in grad.iter_mut().zip(x.iter()) {
                *g += e * xi;
            }
            gb += e;
        }
        for (wi, g) in m.w.iter_mut().zip(&grad) {
            *wi -= lr * (g + l2 * *wi);
        }
        m.b -= lr * gb;
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectors::normalize;

    fn v(seed: u32, center: &[f32]) -> Vec<f32> {
        let mut s = seed.wrapping_mul(2654435761);
        let mut out: Vec<f32> = center
            .iter()
            .map(|c| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                c + (s % 1000) as f32 / 1000.0 * 0.6 - 0.3
            })
            .collect();
        normalize(&mut out);
        out
    }

    #[test]
    fn separates_two_groups() {
        let a = [1.0, 0.2, 0.0, 0.1, 0.0, 0.3, 0.0, 0.1];
        let b = [0.0, 0.1, 1.0, 0.0, 0.2, 0.0, 0.4, 0.0];
        let pos: Vec<Vec<f32>> = (0..20).map(|i| v(i, &a)).collect();
        let neg: Vec<Vec<f32>> = (100..300).map(|i| v(i, &b)).collect();
        let m = train(
            &pos.iter().map(|x| (x.as_slice(), 1.0)).collect::<Vec<_>>(),
            &neg.iter().map(|x| (x.as_slice(), 0.3)).collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(m.predict(&v(999, &a)) > 0.8);
        assert!(m.predict(&v(998, &b)) < 0.2);
    }
}
