//! Time zones: which zone a point is in (tzf-rs, offline), and the UTC
//! offset of a zone at an instant or for a local time (chrono-tz).

use std::sync::LazyLock;

use chrono::{DateTime, NaiveDateTime, Offset, TimeZone};
use chrono_tz::Tz;
use tzf_rs::EmbeddedFinder;

static FINDER: LazyLock<EmbeddedFinder> = LazyLock::new(EmbeddedFinder::new);

/// The IANA zone at a point, e.g. "Europe/Rome". Oceans map to "Etc/GMT±n".
pub fn zone_at(lat: f64, lon: f64) -> Option<String> {
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    let z = FINDER.get_tz_name(lon, lat);
    (!z.is_empty()).then(|| z.to_string())
}

/// Offset east of UTC in minutes for a zone at a UTC instant.
pub fn offset_at_utc(zone: &str, utc_ms: i64) -> Option<i32> {
    let tz: Tz = zone.parse().ok()?;
    let utc = DateTime::from_timestamp_millis(utc_ms)?.naive_utc();
    Some(tz.offset_from_utc_datetime(&utc).fix().local_minus_utc() / 60)
}

/// Offset in minutes for a wall-clock time in a zone. In the hour that
/// repeats when clocks go back, the earlier (summer) offset is used; in the
/// hour that is skipped, the offset after the change.
pub fn offset_for_local(zone: &str, local: NaiveDateTime) -> Option<i32> {
    let tz: Tz = zone.parse().ok()?;
    match tz.offset_from_local_datetime(&local) {
        chrono::LocalResult::Single(o) => Some(o.fix().local_minus_utc() / 60),
        chrono::LocalResult::Ambiguous(a, _) => Some(a.fix().local_minus_utc() / 60),
        chrono::LocalResult::None => {
            tz.offset_from_local_datetime(&(local + chrono::Duration::hours(1))).earliest().map(|o| o.fix().local_minus_utc() / 60)
        }
    }
}

/// The zone and offset at a point and instant.
pub fn at(lat: f64, lon: f64, utc_ms: i64) -> Option<(String, i32)> {
    let z = zone_at(lat, lon)?;
    let o = offset_at_utc(&z, utc_ms)?;
    Some((z, o))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn zones_and_offsets() {
        assert_eq!(zone_at(45.68, 10.65).as_deref(), Some("Europe/Rome"));
        assert_eq!(zone_at(40.7128, -74.006).as_deref(), Some("America/New_York"));
        assert_eq!(zone_at(38.7223, -9.1393).as_deref(), Some("Europe/Lisbon"));
        // 2019-08-03 08:00 UTC: Rome is on summer time.
        let summer = NaiveDate::from_ymd_opt(2019, 8, 3).unwrap().and_hms_opt(8, 0, 0).unwrap().and_utc().timestamp_millis();
        assert_eq!(offset_at_utc("Europe/Rome", summer), Some(120));
        assert_eq!(offset_at_utc("America/New_York", summer), Some(-240));
        let winter = NaiveDate::from_ymd_opt(2019, 1, 3).unwrap().and_hms_opt(8, 0, 0).unwrap().and_utc().timestamp_millis();
        assert_eq!(offset_at_utc("Europe/Rome", winter), Some(60));
    }

    #[test]
    fn dst_boundaries() {
        let d = |m, day, h, min| NaiveDate::from_ymd_opt(2021, m, day).unwrap().and_hms_opt(h, min, 0).unwrap();
        // Clocks go back at 03:00 on 31 Oct 2021: 02:30 happens twice.
        assert_eq!(offset_for_local("Europe/Zurich", d(10, 31, 2, 30)), Some(120));
        assert_eq!(offset_for_local("Europe/Zurich", d(10, 31, 3, 30)), Some(60));
        // Clocks go forward at 02:00 on 28 Mar 2021: 02:30 does not exist.
        assert_eq!(offset_for_local("Europe/Zurich", d(3, 28, 2, 30)), Some(120));
        assert_eq!(offset_for_local("Europe/Zurich", d(3, 28, 1, 30)), Some(60));
    }
}
