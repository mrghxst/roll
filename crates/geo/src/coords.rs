//! Coordinates pasted by the owner: decimal degrees, degrees-minutes-
//! seconds, `geo:` URIs and links from Google Maps, Apple Maps and
//! OpenStreetMap.

use std::sync::LazyLock;

use regex::Regex;

fn valid(lat: f64, lon: f64) -> Option<(f64, f64)> {
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) && !(lat == 0.0 && lon == 0.0)).then_some((lat, lon))
}

static LINK: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        // Google: the pin (!3d…!4d…) beats the viewport (@…).
        r"!3d(-?\d+(?:\.\d+)?)!4d(-?\d+(?:\.\d+)?)",
        r"[?&](?:q|query|ll|sll|daddr|destination)=(-?\d+(?:\.\d+)?)(?:,|%2C)\s*(-?\d+(?:\.\d+)?)",
        r"@(-?\d+(?:\.\d+)?),(-?\d+(?:\.\d+)?)",
        r"^geo:(-?\d+(?:\.\d+)?),(-?\d+(?:\.\d+)?)",
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});

static OSM_MAP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#map=\d+/(-?\d+(?:\.\d+)?)/(-?\d+(?:\.\d+)?)").unwrap());
static OSM_MARKER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"mlat=(-?\d+(?:\.\d+)?)&mlon=(-?\d+(?:\.\d+)?)").unwrap());

/// A degrees value with optional minutes, seconds and hemisphere letter.
static DMS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?ix)
        ([NSEW])?\s*
        (-?\d+(?:[.,]\d+)?)\s*(?:°|deg|d\b)?\s*
        (?:(\d+(?:[.,]\d+)?)\s*(?:'|′|’|m\b|min)\s*)?
        (?:(\d+(?:[.,]\d+)?)\s*(?:"|″|”|''|s\b|sec)\s*)?
        ([NSEW])?"#,
    )
    .unwrap()
});

fn num(s: &str) -> Option<f64> {
    s.replace(',', ".").parse().ok()
}

/// Parse "45.6789, 10.6543", "45°40'44"N 10°39'15"E", "N 45 40.734 E 10
/// 39.258", a `geo:` URI or a map link. Returns (lat, lon).
pub fn parse(input: &str) -> Option<(f64, f64)> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(c) = OSM_MARKER.captures(s).or_else(|| OSM_MAP.captures(s)) {
        return valid(num(&c[1])?, num(&c[2])?);
    }
    if s.contains("://") || s.starts_with("geo:") {
        let s = s.replace("%2C", ",").replace("%2c", ",");
        for re in LINK.iter() {
            if let Some(c) = re.captures(&s) {
                return valid(num(&c[1])?, num(&c[2])?);
            }
        }
        return None;
    }
    // Plain numbers: two decimals separated by a comma, semicolon or space.
    let plain: Vec<&str> = s.split(|c: char| c == ';' || c.is_whitespace() || c == ',').filter(|p| !p.is_empty()).collect();
    if plain.len() == 2
        && let (Some(a), Some(b)) = (plain[0].trim_end_matches('°').parse::<f64>().ok(), plain[1].trim_end_matches('°').parse::<f64>().ok())
    {
        return valid(a, b);
    }
    // Degrees, minutes, seconds with hemispheres.
    let mut parts = Vec::new();
    for c in DMS.captures_iter(s) {
        let Some(d) = c.get(2).and_then(|m| num(m.as_str())) else { continue };
        let m = c.get(3).and_then(|m| num(m.as_str())).unwrap_or(0.0);
        let sec = c.get(4).and_then(|m| num(m.as_str())).unwrap_or(0.0);
        let hemi = c.get(1).or(c.get(5)).map(|h| h.as_str().to_ascii_uppercase());
        let mut v = d.abs() + m / 60.0 + sec / 3600.0;
        if d < 0.0 || matches!(hemi.as_deref(), Some("S" | "W")) {
            v = -v;
        }
        parts.push((v, hemi));
        if parts.len() == 2 {
            break;
        }
    }
    if parts.len() != 2 {
        return None;
    }
    let (a, b) = (parts.remove(0), parts.remove(0));
    // "E 10 … N 45 …" lists longitude first.
    let swap = matches!(a.1.as_deref(), Some("E" | "W")) || matches!(b.1.as_deref(), Some("N" | "S"));
    if swap { valid(b.0, a.0) } else { valid(a.0, b.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(got: Option<(f64, f64)>, lat: f64, lon: f64) {
        let (a, b) = got.unwrap_or_else(|| panic!("no result for {lat}, {lon}"));
        assert!((a - lat).abs() < 1e-4 && (b - lon).abs() < 1e-4, "{a}, {b}");
    }

    #[test]
    fn decimal_and_dms() {
        close(parse("45.6789, 10.6543"), 45.6789, 10.6543);
        close(parse("45.6789 10.6543"), 45.6789, 10.6543);
        close(parse("-33.8688,151.2093"), -33.8688, 151.2093);
        close(parse("50.0506312°, 14.3439906°"), 50.0506312, 14.3439906);
        close(parse(r#"45°40'44.04"N 10°39'15.48"E"#), 45.6789, 10.6543);
        close(parse(r#"45° 40' 44.04" N, 10° 39' 15.48" E"#), 45.6789, 10.6543);
        close(parse("N 45 40.734' E 10 39.258'"), 45.6789, 10.6543);
        close(parse(r#"33°52'7.68"S 151°12'33.48"E"#), -33.8688, 151.2093);
        assert_eq!(parse("hello"), None);
        assert_eq!(parse("95, 10"), None);
    }

    #[test]
    fn links() {
        close(parse("https://www.google.com/maps/@45.6789,10.6543,15z"), 45.6789, 10.6543);
        close(
            parse("https://www.google.com/maps/place/Malcesine/@45.7,10.8,13z/data=!3m1!4b1!4m6!3m5!1s0x0:0x0!8m2!3d45.6789!4d10.6543"),
            45.6789,
            10.6543,
        );
        close(parse("https://maps.google.com/?q=45.6789,10.6543"), 45.6789, 10.6543);
        close(parse("https://maps.apple.com/?ll=45.6789,10.6543&q=Pin"), 45.6789, 10.6543);
        close(parse("https://www.openstreetmap.org/#map=15/45.6789/10.6543"), 45.6789, 10.6543);
        close(parse("https://www.openstreetmap.org/?mlat=45.6789&mlon=10.6543#map=15/45.5/10.5"), 45.6789, 10.6543);
        close(parse("geo:45.6789,10.6543?z=12"), 45.6789, 10.6543);
        assert_eq!(parse("https://example.com/no-coordinates"), None);
    }
}
