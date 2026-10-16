//! Places and time zones, offline: time zone lookup, location history
//! importers, interpolation along tracks, a gazetteer, and parsing of
//! coordinates and map links.

pub mod coords;
pub mod gazetteer;
pub mod interp;
pub mod pmtiles;
pub mod tracks;
pub mod tz;

/// Mean Earth radius in metres.
pub const EARTH_M: f64 = 6_371_008.8;

/// Great-circle distance in metres.
pub fn distance_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = p2 - p1;
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_M * a.sqrt().asin()
}

/// A point with an accuracy radius in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub lat: f64,
    pub lon: f64,
    pub accuracy_m: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances() {
        // Zürich to Milan is about 218 km.
        let d = distance_m(47.3769, 8.5417, 45.4642, 9.19);
        assert!((d - 218_000.0).abs() < 3_000.0, "{d}");
        assert_eq!(distance_m(1.0, 2.0, 1.0, 2.0), 0.0);
    }
}
