//! A position at a moment, from timed points before and after it: the
//! owner's own geotagged photos, or an imported location history.

use crate::tracks::TrackPoint;
use crate::{Place, distance_m};

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Neighbors must be at most this far away in time, on both sides.
    pub window_ms: i64,
    /// One-sided matches (the moment is past the end of a track) are
    /// allowed this close in time.
    pub one_side_ms: i64,
    /// Neighbors further apart than this are not interpolated between
    /// (a flight, a train ride).
    pub max_span_m: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self { window_ms: 30 * 60_000, one_side_ms: 5 * 60_000, max_span_m: 30_000.0 }
    }
}

/// Assumed movement while nobody recorded a position: about walking pace.
const DRIFT_M_PER_MIN: f64 = 2.0;
const ONE_SIDE_M_PER_MIN: f64 = 80.0;

/// The interpolated place, with an accuracy radius that grows with the time
/// gap and the distance between the neighbors.
pub fn at(points: &[TrackPoint], utc_ms: i64, o: &Options) -> Option<Place> {
    if points.is_empty() {
        return None;
    }
    let i = points.partition_point(|p| p.utc_ms <= utc_ms);
    let before = i.checked_sub(1).map(|k| &points[k]).filter(|p| utc_ms - p.utc_ms <= o.window_ms);
    let after = points.get(i).filter(|p| p.utc_ms - utc_ms <= o.window_ms);
    match (before, after) {
        (Some(b), _) if b.utc_ms == utc_ms => Some(Place { lat: b.lat, lon: b.lon, accuracy_m: b.acc_m as f64 }),
        (Some(b), Some(a)) => {
            let d = distance_m(b.lat, b.lon, a.lat, a.lon);
            if d > o.max_span_m {
                return None;
            }
            let span = (a.utc_ms - b.utc_ms).max(1) as f64;
            let f = (utc_ms - b.utc_ms) as f64 / span;
            let gap_min = ((utc_ms - b.utc_ms).min(a.utc_ms - utc_ms)) as f64 / 60_000.0;
            let acc = (b.acc_m.max(a.acc_m) as f64) + d / 2.0 * 4.0 * f * (1.0 - f) + DRIFT_M_PER_MIN * gap_min;
            Some(Place { lat: b.lat + (a.lat - b.lat) * f, lon: lerp_lon(b.lon, a.lon, f), accuracy_m: acc })
        }
        (Some(p), None) | (None, Some(p)) => {
            let gap = (utc_ms - p.utc_ms).abs();
            (gap <= o.one_side_ms).then(|| Place {
                lat: p.lat,
                lon: p.lon,
                accuracy_m: p.acc_m as f64 + ONE_SIDE_M_PER_MIN * gap as f64 / 60_000.0,
            })
        }
        (None, None) => None,
    }
}

/// Longitude interpolation across the antimeridian.
fn lerp_lon(a: f64, b: f64, f: f64) -> f64 {
    let mut d = b - a;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    let v = a + d * f;
    if v > 180.0 {
        v - 360.0
    } else if v < -180.0 {
        v + 360.0
    } else {
        v
    }
}

/// Confidence from an accuracy radius: 1.0 for GPS-like, falling off for
/// wider circles.
pub fn confidence(accuracy_m: f64) -> f32 {
    (1.0 / (1.0 + (accuracy_m / 500.0).powf(1.5))) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(min: i64, lat: f64, lon: f64) -> TrackPoint {
        TrackPoint { utc_ms: min * 60_000, lat, lon, acc_m: 10.0 }
    }

    #[test]
    fn interpolates_between_neighbors() {
        let pts = [p(0, 45.0, 10.0), p(10, 45.01, 10.0)];
        let r = at(&pts, 5 * 60_000, &Options::default()).unwrap();
        assert!((r.lat - 45.005).abs() < 1e-9);
        // About 1.1 km apart: half of it at the midpoint, plus drift.
        assert!(r.accuracy_m > 500.0 && r.accuracy_m < 600.0, "{}", r.accuracy_m);
        let r = at(&pts, 0, &Options::default()).unwrap();
        assert_eq!(r.accuracy_m, 10.0);
    }

    #[test]
    fn respects_windows_and_spans() {
        let o = Options::default();
        let pts = [p(0, 45.0, 10.0), p(100, 45.0, 10.0)];
        assert!(at(&pts, 50 * 60_000, &o).is_none(), "gap too large");
        assert!(at(&pts, 3 * 60_000, &o).is_some(), "close after the first point");
        let flight = [p(0, 47.0, 8.0), p(20, 41.0, 12.0)];
        assert!(at(&flight, 10 * 60_000, &o).is_none());
        let wrap = [p(0, 0.0, 179.9), p(10, 0.0, -179.9)];
        let r = at(&wrap, 5 * 60_000, &o).unwrap();
        assert!(r.lon.abs() > 179.9, "{}", r.lon);
    }

    #[test]
    fn confidence_falls() {
        assert!(confidence(10.0) > 0.99);
        assert!(confidence(500.0) < 0.6);
        assert!(confidence(5000.0) < 0.05);
    }
}
