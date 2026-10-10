//! Deciding an asset's date from what the file says, its name, its folder
//! and, as a last resort, the file time. Each candidate carries a source
//! and a confidence.

use chrono::{DateTime, Datelike, Local, NaiveDateTime, Offset, TimeZone, Timelike, Utc};
use roll_meta::filename::{from_file_name, from_folder_name, Precision};
use roll_meta::{DateSource, FileMeta};

use crate::model::problem;

/// Guesses the UTC offset where the file states none. The default uses the
/// computer's time zone; tests use a fixed one.
pub trait TzGuess: Sync {
    fn offset_for_local(&self, local: NaiveDateTime) -> i32;
    fn offset_for_utc(&self, utc_ms: i64) -> i32;
}

pub struct SystemTz;

impl TzGuess for SystemTz {
    fn offset_for_local(&self, local: NaiveDateTime) -> i32 {
        Local.offset_from_local_datetime(&local).earliest().map(|o| o.fix().local_minus_utc() / 60).unwrap_or(0)
    }
    fn offset_for_utc(&self, utc_ms: i64) -> i32 {
        DateTime::<Utc>::from_timestamp_millis(utc_ms)
            .map(|u| Local.offset_from_utc_datetime(&u.naive_utc()).fix().local_minus_utc() / 60)
            .unwrap_or(0)
    }
}

pub struct FixedTz(pub i32);

impl TzGuess for FixedTz {
    fn offset_for_local(&self, _: NaiveDateTime) -> i32 {
        self.0
    }
    fn offset_for_utc(&self, _: i64) -> i32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DateDecision {
    pub utc_ms: i64,
    pub local: NaiveDateTime,
    /// The stated offset; `None` when it was guessed.
    pub offset: Option<i32>,
    pub source: &'static str,
    pub conf: f32,
    pub problems: i64,
}

impl DateDecision {
    pub fn local_string(&self) -> String {
        self.local.format("%Y-%m-%dT%H:%M:%S").to_string()
    }
}

pub fn is_suspicious(local: NaiveDateTime, now_ms: i64) -> bool {
    let d = local.date();
    let y = d.year();
    // Cameras reset to these after losing power.
    let reset_default = d.month() == 1 && d.day() == 1 && local.time().hour() < 1 && matches!(y, 1970 | 1980 | 2000 | 2001 | 2010);
    y < 1990 || reset_default || local.and_utc().timestamp_millis() > now_ms + 2 * 86_400_000
}

fn from_local(local: NaiveDateTime, offset: Option<i32>, tz: &dyn TzGuess) -> i64 {
    let off = offset.unwrap_or_else(|| tz.offset_for_local(local));
    local.and_utc().timestamp_millis() - off as i64 * 60_000
}

/// Decide the date of one file.
pub fn decide(meta: &FileMeta, name: &str, folders: &[&str], mtime_ms: i64, now_ms: i64, tz: &dyn TzGuess) -> DateDecision {
    if let Some(t) = &meta.taken {
        let mut d = if t.source == DateSource::QuickTime && t.offset_min.is_none() {
            // QuickTime CreateDate is UTC; express it in the local zone.
            let utc_ms = t.local.and_utc().timestamp_millis();
            let off = tz.offset_for_utc(utc_ms);
            DateDecision {
                utc_ms,
                local: t.local + chrono::Duration::minutes(off as i64),
                offset: None,
                source: t.source.as_str(),
                conf: 0.8,
                problems: problem::NO_OFFSET,
            }
        } else {
            DateDecision {
                utc_ms: from_local(t.local, t.offset_min, tz),
                local: t.local,
                offset: t.offset_min,
                source: t.source.as_str(),
                conf: t.source.default_confidence(),
                problems: if t.offset_min.is_none() { problem::NO_OFFSET } else { 0 },
            }
        };
        if is_suspicious(d.local, now_ms) {
            d.problems |= problem::SUSPICIOUS_DATE;
            d.conf = 0.2;
        }
        return d;
    }
    if let Some(n) = from_file_name(name) {
        let (local, offset, utc_ms) = if n.is_utc {
            let utc_ms = n.local.and_utc().timestamp_millis();
            let off = tz.offset_for_utc(utc_ms);
            (n.local + chrono::Duration::minutes(off as i64), None, utc_ms)
        } else {
            (n.local, None, from_local(n.local, None, tz))
        };
        let conf = if n.precision == Precision::Second { 0.7 } else { 0.6 };
        return DateDecision { utc_ms, local, offset, source: DateSource::Filename.as_str(), conf, problems: problem::NO_OFFSET };
    }
    for f in folders.iter().rev() {
        if let Some(n) = from_folder_name(f) {
            let conf = match n.precision {
                Precision::Second | Precision::Day => 0.5,
                Precision::Month => 0.4,
                Precision::Year => 0.3,
            };
            return DateDecision {
                utc_ms: from_local(n.local, None, tz),
                local: n.local,
                offset: None,
                source: DateSource::Folder.as_str(),
                conf,
                problems: problem::NO_DATE,
            };
        }
    }
    let off = tz.offset_for_utc(mtime_ms);
    let local = DateTime::<Utc>::from_timestamp_millis(mtime_ms).unwrap_or_default().naive_utc() + chrono::Duration::minutes(off as i64);
    DateDecision { utc_ms: mtime_ms, local, offset: None, source: DateSource::FileTime.as_str(), conf: 0.2, problems: problem::NO_DATE }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roll_meta::model::parse_datetime;
    use roll_meta::Taken;

    fn now() -> i64 {
        1_790_000_000_000
    }

    fn meta(s: &str, off: Option<i32>, src: DateSource) -> FileMeta {
        let (l, _) = parse_datetime(s).unwrap();
        FileMeta { taken: Some(Taken::new(l, off, src)), ..Default::default() }
    }

    #[test]
    fn exif_with_offset() {
        let d = decide(&meta("2019:08:12 14:00:00", Some(120), DateSource::ExifOriginal), "IMG_1.JPG", &[], 0, now(), &FixedTz(60));
        assert_eq!(d.utc_ms, 1_565_611_200_000);
        assert_eq!(d.offset, Some(120));
        assert_eq!(d.problems, 0);
    }

    #[test]
    fn exif_without_offset_uses_guess() {
        let d = decide(&meta("2019:08:12 14:00:00", None, DateSource::ExifOriginal), "DSC_1.JPG", &[], 0, now(), &FixedTz(60));
        assert_eq!(d.utc_ms, 1_565_611_200_000 + 3_600_000);
        assert_eq!(d.problems, problem::NO_OFFSET);
    }

    #[test]
    fn quicktime_utc_becomes_local() {
        let d = decide(&meta("2019:08:12 12:00:00", None, DateSource::QuickTime), "clip.mp4", &[], 0, now(), &FixedTz(120));
        assert_eq!(d.local.to_string(), "2019-08-12 14:00:00");
        assert_eq!(d.utc_ms, 1_565_611_200_000);
    }

    #[test]
    fn default_dates_are_suspicious() {
        let d = decide(&meta("2000:01:01 00:07:00", None, DateSource::ExifOriginal), "P1.JPG", &[], 0, now(), &FixedTz(60));
        assert!(d.problems & problem::SUSPICIOUS_DATE != 0);
        let d = decide(&meta("2099:01:01 10:00:00", None, DateSource::ExifOriginal), "P1.JPG", &[], 0, now(), &FixedTz(60));
        assert!(d.problems & problem::SUSPICIOUS_DATE != 0);
    }

    #[test]
    fn fallbacks() {
        let m = FileMeta::default();
        let d = decide(&m, "IMG-20190812-WA0001.jpg", &[], 0, now(), &FixedTz(0));
        assert_eq!(d.source, "filename");
        let d = decide(&m, "IMG_1.jpg", &["Old events", "2019-08 Lake Garda"], 0, now(), &FixedTz(0));
        assert_eq!(d.source, "folder");
        assert!(d.problems & problem::NO_DATE != 0);
        let d = decide(&m, "IMG_1.jpg", &["Misc"], 1_565_611_200_000, now(), &FixedTz(0));
        assert_eq!(d.source, "file_time");
        assert_eq!(d.utc_ms, 1_565_611_200_000);
    }
}
