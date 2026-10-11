//! Image embeddings: stored as f16 blobs in SQLite, searched in memory by
//! brute force over normalized f32 rows (cosine similarity is a dot
//! product). Fine up to about 500k items; HNSW only if that's measured to
//! be needed. Result lists from different searches are fused with
//! Reciprocal Rank Fusion.

use std::collections::{HashMap, HashSet};

use half::f16;
use rayon::prelude::*;

/// Little-endian f16, as stored in the `embeddings` table.
pub fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| f16::from_f32(*x).to_le_bytes()).collect()
}

pub fn from_blob(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<2>().0.iter().map(|c| f16::from_le_bytes(*c).to_f32()).collect()
}

pub fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    // Eight lanes so the compiler vectorizes without fast-math.
    let mut acc = [0f32; 8];
    let ((ca, ra), (cb, rb)) = (a.as_chunks::<8>(), b.as_chunks::<8>());
    let tail: f32 = ra.iter().zip(rb).map(|(x, y)| x * y).sum();
    for (x, y) in ca.iter().zip(cb) {
        for i in 0..8 {
            acc[i] += x[i] * y[i];
        }
    }
    acc.iter().sum::<f32>() + tail
}

/// Rows of one model's embeddings, normalized.
#[derive(Debug, Default, Clone)]
pub struct Index {
    dim: usize,
    ids: Vec<i64>,
    rows: Vec<f32>,
    pos: HashMap<i64, usize>,
}

impl Index {
    pub fn new(dim: usize) -> Self {
        Self { dim, ..Default::default() }
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn contains(&self, id: i64) -> bool {
        self.pos.contains_key(&id)
    }

    /// Add or replace the vector of `id`. Vectors of another size are ignored.
    pub fn insert(&mut self, id: i64, v: &[f32]) -> bool {
        if v.len() != self.dim {
            return false;
        }
        let mut v = v.to_vec();
        normalize(&mut v);
        match self.pos.get(&id) {
            Some(&p) => self.rows[p * self.dim..(p + 1) * self.dim].copy_from_slice(&v),
            None => {
                self.pos.insert(id, self.ids.len());
                self.ids.push(id);
                self.rows.extend_from_slice(&v);
            }
        }
        true
    }

    pub fn remove(&mut self, id: i64) {
        let Some(p) = self.pos.remove(&id) else { return };
        let last = self.ids.len() - 1;
        if p != last {
            let moved = self.ids[last];
            self.ids.swap(p, last);
            let (head, tail) = self.rows.split_at_mut(last * self.dim);
            head[p * self.dim..(p + 1) * self.dim].copy_from_slice(&tail[..self.dim]);
            self.pos.insert(moved, p);
        }
        self.ids.pop();
        self.rows.truncate(last * self.dim);
    }

    pub fn get(&self, id: i64) -> Option<&[f32]> {
        self.pos.get(&id).map(|&p| &self.rows[p * self.dim..(p + 1) * self.dim])
    }

    /// The `k` most similar rows, best first, among `only` when given.
    pub fn search(&self, query: &[f32], k: usize, only: Option<&HashSet<i64>>) -> Vec<(i64, f32)> {
        if query.len() != self.dim || self.ids.is_empty() || k == 0 {
            return Vec::new();
        }
        let mut q = query.to_vec();
        normalize(&mut q);
        let mut scored: Vec<(i64, f32)> = self
            .rows
            .par_chunks(self.dim)
            .zip(self.ids.par_iter())
            .filter(|(_, id)| only.is_none_or(|s| s.contains(id)))
            .map(|(row, id)| (*id, dot(row, &q)))
            .collect();
        let k = k.min(scored.len());
        if k < scored.len() {
            scored.select_nth_unstable_by(k, |a, b| b.1.total_cmp(&a.1));
            scored.truncate(k);
        }
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        scored
    }
}

/// Reciprocal Rank Fusion: each list adds `1 / (k + rank)` per id (rank
/// from 1). `k = 60` is the usual constant. Best first.
pub fn rrf(lists: &[&[i64]], k: f32) -> Vec<(i64, f32)> {
    let mut score: HashMap<i64, f32> = HashMap::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            *score.entry(*id).or_default() += 1.0 / (k + rank as f32 + 1.0);
        }
    }
    let mut out: Vec<(i64, f32)> = score.into_iter().collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rnd(seed: u64, dim: usize) -> Vec<f32> {
        let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (0..dim)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 33) as f32 / (1u64 << 31) as f32) - 0.5
            })
            .collect()
    }

    #[test]
    fn blobs_round_trip_in_half_precision() {
        let v = vec![0.0, 1.0, -0.5, 0.123_456, 65504.0];
        let back = from_blob(&to_blob(&v));
        assert_eq!(to_blob(&v).len(), 10);
        for (a, b) in v.iter().zip(&back) {
            assert!((a - b).abs() <= a.abs() * 1e-3 + 1e-4, "{a} {b}");
        }
    }

    #[test]
    fn finds_the_nearest_and_filters() {
        let mut ix = Index::new(64);
        for id in 0..500 {
            ix.insert(id, &rnd(id as u64, 64));
        }
        assert!(!ix.insert(999, &[1.0, 2.0]), "wrong size is refused");
        let mut q = rnd(42, 64);
        q.iter_mut().for_each(|x| *x *= 3.0); // scale doesn't matter
        let top = ix.search(&q, 5, None);
        assert_eq!(top[0].0, 42);
        assert!((top[0].1 - 1.0).abs() < 1e-5);
        assert!(top.windows(2).all(|w| w[0].1 >= w[1].1));
        let only: HashSet<i64> = [7, 8, 9].into();
        let some = ix.search(&q, 10, Some(&only));
        assert_eq!(some.len(), 3);
        assert!(some.iter().all(|(id, _)| only.contains(id)));
    }

    #[test]
    fn replace_and_remove_keep_rows_aligned() {
        let mut ix = Index::new(16);
        for id in 0..10 {
            ix.insert(id, &rnd(id as u64, 16));
        }
        ix.remove(3);
        ix.remove(9); // the last row
        ix.remove(100); // unknown
        assert_eq!(ix.len(), 8);
        assert!(!ix.contains(3));
        for id in [0, 1, 2, 4, 5, 6, 7, 8] {
            assert_eq!(ix.search(&rnd(id as u64, 16), 1, None)[0].0, id);
        }
        ix.insert(5, &rnd(77, 16));
        assert_eq!(ix.len(), 8);
        assert_eq!(ix.search(&rnd(77, 16), 1, None)[0].0, 5);
        assert!(ix.get(5).is_some());
    }

    #[test]
    fn rrf_rewards_agreement() {
        let text = [1, 2, 3, 4];
        let image = [3, 5, 1, 6];
        let fused = rrf(&[&text, &image], 60.0);
        let order: Vec<i64> = fused.iter().map(|(id, _)| *id).collect();
        // 1 and 3 appear in both lists; 1 ranks higher overall.
        assert_eq!(&order[..2], &[1, 3]);
        assert_eq!(order.len(), 6);
        assert!(rrf(&[], 60.0).is_empty());
    }

    #[test]
    #[ignore = "timing; run with --ignored --release"]
    fn search_100k_by_768() {
        let mut ix = Index::new(768);
        for id in 0..100_000 {
            ix.insert(id, &rnd(id as u64, 768));
        }
        let q = rnd(5, 768);
        let t = std::time::Instant::now();
        let top = ix.search(&q, 200, None);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!("100k × 768 search: {ms:.1} ms");
        assert_eq!(top[0].0, 5);
    }
}
