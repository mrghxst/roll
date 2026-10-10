use chrono::{DateTime, Duration, FixedOffset, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    #[default]
    Photo,
    Video,
}

/// Where a date value came from. Ordered roughly by trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateSource {
    ExifOriginal,
    ExifCreate,
    QuickTime,
    Xmp,
    Sidecar,
    Takeout,
    Filename,
    Folder,
    FileTime,
}

impl DateSource {
    pub fn as_str(self) -> &'static str {
        match self {
            DateSource::ExifOriginal => "exif_original",
            DateSource::ExifCreate => "exif_create",
            DateSource::QuickTime => "quicktime",
            DateSource::Xmp => "xmp",
            DateSource::Sidecar => "sidecar",
            DateSource::Takeout => "takeout",
            DateSource::Filename => "filename",
            DateSource::Folder => "folder",
            DateSource::FileTime => "file_time",
        }
    }

    pub fn default_confidence(self) -> f32 {
        match self {
            DateSource::ExifOriginal | DateSource::Sidecar => 0.95,
            DateSource::ExifCreate | DateSource::QuickTime | DateSource::Xmp => 0.9,
            DateSource::Takeout => 0.9,
            DateSource::Filename => 0.7,
            DateSource::Folder => 0.4,
            DateSource::FileTime => 0.2,
        }
    }
}

/// A capture time as the file states it: local wall time plus an optional
/// UTC offset. Without an offset the UTC instant is unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Taken {
    pub local: NaiveDateTime,
    pub offset_min: Option<i32>,
    pub source: DateSource,
}

impl Taken {
    pub fn new(local: NaiveDateTime, offset_min: Option<i32>, source: DateSource) -> Self {
        Self { local, offset_min, source }
    }

    /// Build from a UTC instant and an optional offset to express it in.
    pub fn from_utc(utc: DateTime<Utc>, offset_min: Option<i32>, source: DateSource) -> Self {
        let local = match offset_min {
            Some(m) => utc.naive_utc() + Duration::minutes(m as i64),
            None => utc.naive_utc(),
        };
        Self { local, offset_min, source }
    }

    /// The UTC instant. Without an offset the local time is treated as UTC,
    /// which keeps ordering stable; callers check `offset_min` when it matters.
    pub fn utc_ms(&self) -> i64 {
        let off = self.offset_min.unwrap_or(0) as i64;
        (self.local - Duration::minutes(off)).and_utc().timestamp_millis()
    }

    pub fn fixed_offset(&self) -> Option<DateTime<FixedOffset>> {
        let off = FixedOffset::east_opt(self.offset_min? * 60)?;
        off.from_local_datetime(&self.local).single()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Gps {
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
    /// Horizontal accuracy in meters, when the file states it.
    pub accuracy_m: Option<f64>,
}

impl Gps {
    pub fn is_valid(&self) -> bool {
        self.lat.is_finite()
            && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lon)
            && !(self.lat == 0.0 && self.lon == 0.0)
    }
}

/// Everything Roll reads from one file's headers. All fields are optional:
/// broken or unusual files still produce a partial record.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FileMeta {
    pub kind: MediaKind,
    pub format: String,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub orientation: Option<u16>,
    pub duration_ms: Option<u64>,
    pub fps: Option<f32>,
    pub has_audio: Option<bool>,
    pub codec: Option<String>,
    /// Video rotation from the track matrix, in degrees clockwise.
    pub rotation: Option<i32>,
    pub taken: Option<Taken>,
    pub gps: Option<Gps>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub lens: Option<String>,
    pub software: Option<String>,
    pub screenshot_marker: bool,
    /// Apple ContentIdentifier / MediaGroupUUID, links Live Photo parts.
    pub content_id: Option<String>,
    pub burst_id: Option<String>,
    pub description: Option<String>,
    pub rating: Option<u8>,
    /// Hierarchical tags with `/` separators, as found in XMP.
    pub tags: Vec<String>,
    pub has_xmp: bool,
    pub has_maker_notes: bool,
    pub icc_profile: Option<String>,
    /// Short fingerprint of the JPEG quantization tables (encoder signature).
    pub qtable: Option<String>,
    /// Errors that did not stop reading.
    pub warnings: Vec<String>,
}

impl FileMeta {
    pub fn has_camera(&self) -> bool {
        self.make.as_deref().is_some_and(|m| !m.trim().is_empty())
            || self.model.as_deref().is_some_and(|m| !m.trim().is_empty())
    }

    /// Display size after orientation (EXIF orientation or video rotation).
    pub fn display_size(&self) -> Option<(u32, u32)> {
        let (w, h) = (self.width?, self.height?);
        let swap = matches!(self.orientation, Some(5..=8)) || matches!(self.rotation, Some(90 | 270 | -90));
        Some(if swap { (h, w) } else { (w, h) })
    }
}

pub fn parse_offset(s: &str) -> Option<i32> {
    let s = s.trim();
    if s == "Z" {
        return Some(0);
    }
    let (sign, rest) = match s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => return None,
    };
    let mut parts = rest.split(':');
    let h: i32 = parts.next()?.parse().ok()?;
    let m: i32 = parts.next().unwrap_or("0").parse().ok()?;
    if h > 14 || m > 59 {
        return None;
    }
    Some(sign * (h * 60 + m))
}

pub fn format_offset(min: i32) -> String {
    let sign = if min < 0 { '-' } else { '+' };
    let a = min.abs();
    format!("{sign}{:02}:{:02}", a / 60, a % 60)
}

/// Parse EXIF-style (`2019:08:12 14:03:22`) and ISO-style dates, with an
/// optional trailing offset. Rejects zeroed and obviously invalid values.
pub fn parse_datetime(s: &str) -> Option<(NaiveDateTime, Option<i32>)> {
    let s = s.trim().trim_end_matches('\0');
    if s.len() < 19 || s.starts_with("0000") {
        return None;
    }
    let (head, tail) = s.split_at(19);
    let norm: String = head
        .char_indices()
        .map(|(i, c)| match (i, c) {
            (4 | 7, ':' | '-') => '-',
            (10, ' ' | 'T') => 'T',
            _ => c,
        })
        .collect();
    let local = NaiveDateTime::parse_from_str(&norm, "%Y-%m-%dT%H:%M:%S").ok()?;
    // Skip fractional seconds.
    let tail = tail.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset = if tail.is_empty() { None } else { parse_offset(tail) };
    Some((local, offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exif_and_iso_dates() {
        let (d, o) = parse_datetime("2019:08:12 14:03:22").unwrap();
        assert_eq!(d.to_string(), "2019-08-12 14:03:22");
        assert_eq!(o, None);
        let (_, o) = parse_datetime("2019-08-12T14:03:22.120+02:00").unwrap();
        assert_eq!(o, Some(120));
        let (_, o) = parse_datetime("2019:08:12 14:03:22Z").unwrap();
        assert_eq!(o, Some(0));
        assert!(parse_datetime("0000:00:00 00:00:00").is_none());
        assert!(parse_datetime("    :  :     :  :  ").is_none());
    }

    #[test]
    fn offsets_round_trip() {
        for m in [-570, -60, 0, 60, 345, 840] {
            assert_eq!(parse_offset(&format_offset(m)), Some(m));
        }
    }

    #[test]
    fn utc_uses_offset() {
        let (d, _) = parse_datetime("2019:08:12 14:00:00").unwrap();
        let t = Taken::new(d, Some(120), DateSource::ExifOriginal);
        let u = Taken::new(d, Some(0), DateSource::ExifOriginal);
        assert_eq!(u.utc_ms() - t.utc_ms(), 2 * 3600 * 1000);
        let back = Taken::from_utc(chrono::DateTime::from_timestamp_millis(t.utc_ms()).unwrap(), Some(120), DateSource::QuickTime);
        assert_eq!(back.local, d);
    }
}
