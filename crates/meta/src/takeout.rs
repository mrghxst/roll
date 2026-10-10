//! Google Takeout JSON sidecars (`IMG_1234.jpg.json`,
//! `IMG_1234.jpg.supplemental-metadata.json` and truncated variants).

use chrono::DateTime;
use serde::Deserialize;

use crate::model::{DateSource, Gps, Taken};

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Raw {
    title: Option<String>,
    description: Option<String>,
    photo_taken_time: Option<Stamp>,
    geo_data: Option<Geo>,
    geo_data_exif: Option<Geo>,
    favorited: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct Stamp {
    timestamp: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Geo {
    latitude: f64,
    longitude: f64,
    altitude: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Takeout {
    pub title: Option<String>,
    pub description: Option<String>,
    pub taken: Option<Taken>,
    pub gps: Option<Gps>,
    pub favorite: bool,
}

pub fn parse(json: &str) -> Option<Takeout> {
    let raw: Raw = serde_json::from_str(json).ok()?;
    let taken = raw
        .photo_taken_time
        .and_then(|s| s.timestamp.parse::<i64>().ok())
        .filter(|t| *t > 0)
        .and_then(|t| DateTime::from_timestamp(t, 0))
        .map(|u| Taken::from_utc(u, None, DateSource::Takeout));
    let gps = [raw.geo_data, raw.geo_data_exif]
        .into_iter()
        .flatten()
        .map(|g| Gps { lat: g.latitude, lon: g.longitude, alt: g.altitude, accuracy_m: None })
        .find(Gps::is_valid);
    Some(Takeout {
        title: raw.title,
        description: raw.description.filter(|d| !d.trim().is_empty()),
        taken,
        gps,
        favorite: raw.favorited.unwrap_or(false),
    })
}

/// Whether a JSON file name belongs to a media file name. Takeout truncates
/// long names to 46 characters before `.json`, and adds `(1)` counters after
/// the extension (`IMG_1.jpg(1).json` belongs to `IMG_1(1).jpg`).
pub fn json_matches(json_name: &str, media_name: &str) -> bool {
    let Some(stem) = json_name.strip_suffix(".json") else { return false };
    let stem = stem.strip_suffix(".supplemental-metadata").or_else(|| stem.split_once(".suppl").map(|(a, _)| a)).unwrap_or(stem);
    if stem == media_name {
        return true;
    }
    // `X.ext(N)` belongs to `X(N).ext`.
    if let Some((base, counter)) = stem.strip_suffix(')').and_then(|s| s.rsplit_once('('))
        && let Some((b_stem, b_ext)) = base.rsplit_once('.')
        && media_name == format!("{b_stem}({counter}).{b_ext}")
    {
        return true;
    }
    stem.chars().count() >= 40 && media_name.starts_with(stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_takeout() {
        let j = r#"{"title":"IMG_1234.jpg","description":"","photoTakenTime":{"timestamp":"1565611402","formatted":"…"},
          "geoData":{"latitude":0.0,"longitude":0.0,"altitude":0.0},
          "geoDataExif":{"latitude":45.64,"longitude":10.68,"altitude":70.0},"favorited":true}"#;
        let t = parse(j).unwrap();
        assert_eq!(t.taken.unwrap().utc_ms(), 1_565_611_402_000);
        assert!((t.gps.unwrap().lat - 45.64).abs() < 1e-9);
        assert!(t.favorite);
        assert!(t.description.is_none());
    }

    #[test]
    fn matches_names() {
        assert!(json_matches("IMG_1234.jpg.json", "IMG_1234.jpg"));
        assert!(json_matches("IMG_1234.jpg.supplemental-metadata.json", "IMG_1234.jpg"));
        assert!(json_matches("IMG_1234.jpg.suppl.json", "IMG_1234.jpg"));
        assert!(json_matches("IMG_1.jpg(1).json", "IMG_1(1).jpg"));
        assert!(!json_matches("IMG_1.jpg(1).json", "IMG_1.jpg"));
        assert!(json_matches(
            "Screenshot_20190812-140322_Some_Very_Long_App.json",
            "Screenshot_20190812-140322_Some_Very_Long_App_Name.png"
        ));
        assert!(!json_matches("IMG_9.jpg.json", "IMG_1234.jpg"));
    }
}
