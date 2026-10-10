//! Dates hidden in file and folder names: `IMG_20190812_140322`,
//! `IMG-20190812-WA0001`, `Screenshot 2019-08-12 at 14.03.22`,
//! `FB_IMG_1565611402000`, `2019-08 Lake Garda`.

use std::sync::LazyLock;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime};
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precision {
    Year,
    Month,
    Day,
    Second,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameDate {
    pub local: NaiveDateTime,
    pub precision: Precision,
    /// Unix timestamps in names are UTC instants, not wall time.
    pub is_utc: bool,
}

static FULL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)(?:^|[^0-9])
        (?P<y>(?:19|20)\d{2})[-_.]?(?P<mo>0[1-9]|1[0-2])[-_.]?(?P<d>0[1-9]|[12]\d|3[01])
        (?: (?:[\sT_-]|\sat\s)? (?P<h>[01]?\d|2[0-3]) [-_.:]? (?P<mi>[0-5]\d) [-_.:]? (?P<s>[0-5]\d) (?:\d{3})?
            (?:\s?(?P<ampm>[AaPp][Mm]))? )?
        (?:[^0-9]|$)",
    )
    .unwrap()
});
static UNIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[^0-9])(1[0-9]{9})(\d{3})?(?:[^0-9]|$)").unwrap());
static YEAR_MONTH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[^0-9])((?:19|20)\d{2})[-_. ](0[1-9]|1[0-2])(?:[^0-9]|$)").unwrap());
static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[^0-9])((?:19|20)\d{2})(?:[^0-9]|$)").unwrap());

fn plausible_year(y: i32) -> bool {
    (1990..=2100).contains(&y)
}

/// A date from a file name (stem without extension works best).
pub fn from_file_name(name: &str) -> Option<NameDate> {
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    if let Some(c) = FULL.captures(stem) {
        let y: i32 = c["y"].parse().ok()?;
        let date = NaiveDate::from_ymd_opt(y, c["mo"].parse().ok()?, c["d"].parse().ok()?)?;
        if !plausible_year(y) {
            return None;
        }
        if let (Some(h), Some(mi), Some(s)) = (c.name("h"), c.name("mi"), c.name("s")) {
            let mut h: u32 = h.as_str().parse().ok()?;
            match c.name("ampm").map(|m| m.as_str().to_ascii_lowercase()) {
                Some(p) if p == "pm" && h < 12 => h += 12,
                Some(p) if p == "am" && h == 12 => h = 0,
                _ => {}
            }
            let t = NaiveTime::from_hms_opt(h, mi.as_str().parse().ok()?, s.as_str().parse().ok()?)?;
            return Some(NameDate { local: date.and_time(t), precision: Precision::Second, is_utc: false });
        }
        return Some(NameDate { local: date.and_hms_opt(12, 0, 0)?, precision: Precision::Day, is_utc: false });
    }
    let c = UNIX.captures(stem)?;
    let secs: i64 = c[1].parse().ok()?;
    let dt = DateTime::from_timestamp(secs, 0)?;
    let y = dt.naive_utc().date().format("%Y").to_string().parse::<i32>().ok()?;
    (2005..=2040).contains(&y).then(|| NameDate { local: dt.naive_utc(), precision: Precision::Second, is_utc: true })
}

/// A coarse date from a folder name: `2019-08 Lake Garda` or `2019`.
pub fn from_folder_name(name: &str) -> Option<NameDate> {
    if let Some(d) = from_file_name(name).filter(|d| !d.is_utc) {
        return Some(d);
    }
    if let Some(c) = YEAR_MONTH.captures(name) {
        let y: i32 = c[1].parse().ok()?;
        let date = NaiveDate::from_ymd_opt(y, c[2].parse().ok()?, 15)?;
        return plausible_year(y).then(|| NameDate { local: date.and_hms_opt(12, 0, 0).unwrap(), precision: Precision::Month, is_utc: false });
    }
    let c = YEAR.captures(name)?;
    let y: i32 = c[1].parse().ok()?;
    let date = NaiveDate::from_ymd_opt(y, 7, 1)?;
    plausible_year(y).then(|| NameDate { local: date.and_hms_opt(12, 0, 0).unwrap(), precision: Precision::Year, is_utc: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: &str) -> String {
        let d = from_file_name(n).unwrap();
        format!("{} {:?}", d.local, d.precision)
    }

    #[test]
    fn file_names() {
        assert_eq!(s("IMG_20190812_140322.jpg"), "2019-08-12 14:03:22 Second");
        assert_eq!(s("PXL_20190812_140322123.jpg"), "2019-08-12 14:03:22 Second");
        assert_eq!(s("IMG-20190812-WA0001.jpg"), "2019-08-12 12:00:00 Day");
        assert_eq!(s("Screenshot_20190812-140322.png"), "2019-08-12 14:03:22 Second");
        assert_eq!(s("Screenshot 2019-08-12 at 14.03.22.png"), "2019-08-12 14:03:22 Second");
        assert_eq!(s("Screen Shot 2019-08-12 at 2.03.22 PM.png"), "2019-08-12 14:03:22 Second");
        assert_eq!(s("2019-08-12 14.03.22.jpg"), "2019-08-12 14:03:22 Second");
        let fb = from_file_name("FB_IMG_1565611402000.jpg").unwrap();
        assert!(fb.is_utc);
        assert_eq!(fb.local.to_string(), "2019-08-12 12:03:22");
        assert!(from_file_name("IMG_1234.HEIC").is_none());
        assert!(from_file_name("IMG_device_2019 - 12.jpeg").is_none());
    }

    #[test]
    fn folder_names() {
        let d = from_folder_name("2019-08 Lake Garda").unwrap();
        assert_eq!(d.precision, Precision::Month);
        assert_eq!(from_folder_name("Italy 2019").unwrap().precision, Precision::Year);
        assert!(from_folder_name("Holidays").is_none());
    }
}
