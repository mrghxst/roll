//! Location history importers. Each format becomes a list of timed points,
//! sorted by time:
//!
//! | Format | Detected by | Points from |
//! |---|---|---|
//! | GPX | `<gpx>` root | `trkpt`, `rtept`, `wpt` with `<time>` |
//! | KML | `<kml>` root | `gx:Track` (`when` + `gx:coord`), timestamped `Point`s |
//! | GeoJSON | `"type": "FeatureCollection"`/`"Feature"` | `coordTimes`, 4th coordinate, `time` properties |
//! | Google Timeline (phone) | `semanticSegments` | `timelinePath`, `visit`, `activity`, `rawSignals.position` |
//! | Google Takeout Records | `locations` | `latitudeE7`, `longitudeE7`, `timestamp` or `timestampMs` |
//! | Semantic Location History | `timelineObjects` | `placeVisit`, `activitySegment` with waypoints |

use std::path::Path;

use chrono::DateTime;
use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::Value;

/// One timed position. A visit is a span at one place: its start and end
/// become two points, so interpolation inside the visit stays put.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPoint {
    pub utc_ms: i64,
    pub lat: f64,
    pub lon: f64,
    /// Accuracy radius in metres.
    pub acc_m: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Gpx,
    Kml,
    GeoJson,
    GoogleTimeline,
    GoogleRecords,
    GoogleSemantic,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Gpx => "GPX",
            Format::Kml => "KML",
            Format::GeoJson => "GeoJSON",
            Format::GoogleTimeline => "Google Maps Timeline",
            Format::GoogleRecords => "Google Takeout location history",
            Format::GoogleSemantic => "Google semantic location history",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TrackError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("This file is not a location history Roll can read (GPX, KML, GeoJSON or Google Timeline).")]
    Unknown,
    #[error("The file could not be read as {0}: {1}")]
    Parse(&'static str, String),
}

/// Typical accuracy when a format gives none.
const GPS_ACC: f32 = 10.0;
const VISIT_ACC: f32 = 50.0;
const PATH_ACC: f32 = 100.0;

pub fn read_file(path: &Path) -> Result<(Format, Vec<TrackPoint>), TrackError> {
    let bytes = std::fs::read(path)?;
    parse(&bytes)
}

/// Detect the format from the content and parse it.
pub fn parse(bytes: &[u8]) -> Result<(Format, Vec<TrackPoint>), TrackError> {
    let text = String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes));
    let head = text.trim_start();
    let (format, mut pts) = if head.starts_with('<') {
        let lower = head[..head.len().min(2000)].to_ascii_lowercase();
        if lower.contains("<gpx") {
            (Format::Gpx, xml_points(&text, Format::Gpx)?)
        } else if lower.contains("<kml") {
            (Format::Kml, xml_points(&text, Format::Kml)?)
        } else {
            return Err(TrackError::Unknown);
        }
    } else if head.starts_with('{') || head.starts_with('[') {
        let v: Value = serde_json::from_str(&text).map_err(|e| TrackError::Parse("JSON", e.to_string()))?;
        json_points(&v)?
    } else {
        return Err(TrackError::Unknown);
    };
    pts.retain(|p| (-90.0..=90.0).contains(&p.lat) && (-180.0..=180.0).contains(&p.lon) && !(p.lat == 0.0 && p.lon == 0.0));
    pts.sort_by_key(|p| p.utc_ms);
    pts.dedup_by(|a, b| a.utc_ms == b.utc_ms && a.lat == b.lat && a.lon == b.lon);
    Ok((format, pts))
}

fn time_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some(d.timestamp_millis());
    }
    // Some exporters leave out the zone; GPX and KML times are UTC.
    chrono::NaiveDateTime::parse_from_str(s.trim_end_matches('Z'), "%Y-%m-%dT%H:%M:%S%.f").ok().map(|d| d.and_utc().timestamp_millis())
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase()
}

fn xml_points(text: &str, format: Format) -> Result<Vec<TrackPoint>, TrackError> {
    let label = if format == Format::Gpx { "GPX" } else { "KML" };
    let mut r = Reader::from_str(text);
    r.config_mut().trim_text(true);
    let mut out = Vec::new();
    // GPX state: the open point and its time.
    let mut gpx_pt: Option<(f64, f64)> = None;
    let mut gpx_time: Option<i64> = None;
    // KML state: gx:Track whens and coords, or a Placemark's time and point.
    let mut whens: Vec<i64> = Vec::new();
    let mut coords: Vec<(f64, f64)> = Vec::new();
    let mut mark_time: Option<i64> = None;
    let mut mark_point: Option<(f64, f64)> = None;
    let mut path: Vec<String> = Vec::new();
    loop {
        match r.read_event() {
            Err(e) => return Err(TrackError::Parse(label, e.to_string())),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = local(e.name().as_ref());
                if format == Format::Gpx && matches!(name.as_str(), "trkpt" | "rtept" | "wpt") {
                    let mut lat = None;
                    let mut lon = None;
                    for a in e.attributes().flatten() {
                        let v = a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().and_then(|v| v.trim().parse::<f64>().ok());
                        match local(a.key.as_ref()).as_str() {
                            "lat" => lat = v,
                            "lon" => lon = v,
                            _ => {}
                        }
                    }
                    gpx_pt = lat.zip(lon);
                    gpx_time = None;
                }
                if format == Format::Kml && name == "placemark" {
                    mark_time = None;
                    mark_point = None;
                }
                path.push(name);
            }
            Ok(Event::Empty(_)) => {}
            Ok(Event::Text(t)) => {
                let s = t.xml10_content().into_owned();
                let Some(cur) = path.last() else { continue };
                match (format, cur.as_str()) {
                    (Format::Gpx, "time") if gpx_pt.is_some() => gpx_time = time_ms(&s),
                    (Format::Kml, "when") => {
                        let in_track = path.iter().any(|p| p == "track");
                        if let Some(t) = time_ms(&s) {
                            if in_track {
                                whens.push(t);
                            } else {
                                mark_time = Some(t);
                            }
                        }
                    }
                    (Format::Kml, "coord") => {
                        let v: Vec<f64> = s.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                        if v.len() >= 2 {
                            coords.push((v[1], v[0]));
                        }
                    }
                    (Format::Kml, "coordinates") if path.iter().any(|p| p == "point") => {
                        let v: Vec<f64> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                        if v.len() >= 2 {
                            mark_point = Some((v[1], v[0]));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = local(e.name().as_ref());
                match (format, name.as_str()) {
                    (Format::Gpx, "trkpt" | "rtept" | "wpt") => {
                        if let (Some((lat, lon)), Some(t)) = (gpx_pt.take(), gpx_time.take()) {
                            out.push(TrackPoint { utc_ms: t, lat, lon, acc_m: GPS_ACC });
                        }
                    }
                    (Format::Kml, "track") => {
                        for (t, (lat, lon)) in whens.drain(..).zip(coords.drain(..)) {
                            out.push(TrackPoint { utc_ms: t, lat, lon, acc_m: GPS_ACC });
                        }
                    }
                    (Format::Kml, "placemark") => {
                        if let (Some(t), Some((lat, lon))) = (mark_time.take(), mark_point.take()) {
                            out.push(TrackPoint { utc_ms: t, lat, lon, acc_m: GPS_ACC });
                        }
                    }
                    _ => {}
                }
                path.pop();
            }
            Ok(_) => {}
        }
    }
    Ok(out)
}

/// "50.0506312°, 14.3439906°" or "geo:50.05,14.34".
fn latlng_str(v: &Value) -> Option<(f64, f64)> {
    let s = v.as_str()?.trim_start_matches("geo:");
    let mut it = s.split(',').map(|p| p.trim().trim_end_matches('°').trim().parse::<f64>());
    Some((it.next()?.ok()?, it.next()?.ok()?))
}

fn e7(v: &Value, lat: &str, lon: &str) -> Option<(f64, f64)> {
    Some((v.get(lat)?.as_f64()? / 1e7, v.get(lon)?.as_f64()? / 1e7))
}

fn ts(v: &Value) -> Option<i64> {
    match v {
        Value::String(s) => time_ms(s).or_else(|| s.parse::<i64>().ok()),
        Value::Number(n) => n.as_i64(),
        _ => None,
    }
}

fn acc(v: &Value, keys: &[&str], default: f32) -> f32 {
    keys.iter().find_map(|k| v.get(*k).and_then(Value::as_f64)).map(|a| a as f32).filter(|a| *a > 0.0).unwrap_or(default)
}

/// Points spread evenly in time between `start` and `end`.
fn spread(points: &[(f64, f64)], start: i64, end: i64, acc_m: f32, out: &mut Vec<TrackPoint>) {
    let n = points.len();
    for (i, (lat, lon)) in points.iter().enumerate() {
        let t = if n > 1 { start + (end - start) * i as i64 / (n as i64 - 1) } else { start };
        out.push(TrackPoint { utc_ms: t, lat: *lat, lon: *lon, acc_m });
    }
}

fn json_points(v: &Value) -> Result<(Format, Vec<TrackPoint>), TrackError> {
    let mut out = Vec::new();
    if v.get("semanticSegments").is_some() || v.get("rawSignals").is_some() {
        for seg in v.get("semanticSegments").and_then(Value::as_array).into_iter().flatten() {
            let start = seg.get("startTime").and_then(ts);
            let end = seg.get("endTime").and_then(ts);
            for p in seg.get("timelinePath").and_then(Value::as_array).into_iter().flatten() {
                if let (Some((lat, lon)), Some(t)) = (p.get("point").and_then(latlng_str), p.get("time").and_then(ts)) {
                    out.push(TrackPoint { utc_ms: t, lat, lon, acc_m: PATH_ACC });
                }
            }
            if let (Some(visit), Some(s), Some(e)) = (seg.get("visit"), start, end)
                && let Some((lat, lon)) = visit.pointer("/topCandidate/placeLocation/latLng").and_then(latlng_str)
            {
                spread(&[(lat, lon), (lat, lon)], s, e, VISIT_ACC, &mut out);
            }
            if let (Some(act), Some(s), Some(e)) = (seg.get("activity"), start, end) {
                let a = act.pointer("/start/latLng").and_then(latlng_str);
                let b = act.pointer("/end/latLng").and_then(latlng_str);
                if let (Some(a), Some(b)) = (a, b) {
                    spread(&[a, b], s, e, PATH_ACC, &mut out);
                }
            }
        }
        for sig in v.get("rawSignals").and_then(Value::as_array).into_iter().flatten() {
            if let Some(p) = sig.get("position")
                && let (Some((lat, lon)), Some(t)) =
                    (p.get("LatLng").or(p.get("latLng")).and_then(latlng_str), p.get("timestamp").and_then(ts))
            {
                out.push(TrackPoint { utc_ms: t, lat, lon, acc_m: acc(p, &["accuracyMeters"], GPS_ACC) });
            }
        }
        return Ok((Format::GoogleTimeline, out));
    }
    if let Some(locs) = v.get("locations").and_then(Value::as_array) {
        for l in locs {
            let t = l.get("timestamp").and_then(ts).or_else(|| l.get("timestampMs").and_then(ts));
            if let (Some((lat, lon)), Some(t)) = (e7(l, "latitudeE7", "longitudeE7"), t) {
                out.push(TrackPoint { utc_ms: t, lat, lon, acc_m: acc(l, &["accuracy"], GPS_ACC) });
            }
        }
        return Ok((Format::GoogleRecords, out));
    }
    if let Some(objs) = v.get("timelineObjects").and_then(Value::as_array) {
        for o in objs {
            let span = |x: &Value| -> Option<(i64, i64)> {
                let d = x.get("duration")?;
                Some((
                    d.get("startTimestamp").or(d.get("startTimestampMs")).and_then(ts)?,
                    d.get("endTimestamp").or(d.get("endTimestampMs")).and_then(ts)?,
                ))
            };
            if let Some(pv) = o.get("placeVisit")
                && let (Some((s, e)), Some(p)) = (span(pv), pv.get("location").and_then(|l| e7(l, "latitudeE7", "longitudeE7")))
            {
                spread(&[p, p], s, e, VISIT_ACC, &mut out);
            }
            if let Some(a) = o.get("activitySegment")
                && let Some((s, e)) = span(a)
            {
                let mut pts: Vec<(f64, f64)> = Vec::new();
                pts.extend(a.get("startLocation").and_then(|l| e7(l, "latitudeE7", "longitudeE7")));
                for w in a.pointer("/waypointPath/waypoints").and_then(Value::as_array).into_iter().flatten() {
                    pts.extend(e7(w, "latE7", "lngE7"));
                }
                pts.extend(a.get("endLocation").and_then(|l| e7(l, "latitudeE7", "longitudeE7")));
                pts.dedup();
                spread(&pts, s, e, PATH_ACC, &mut out);
            }
        }
        return Ok((Format::GoogleSemantic, out));
    }
    let features: Vec<&Value> = match v.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => v.get("features").and_then(Value::as_array).map(|f| f.iter().collect()).unwrap_or_default(),
        Some("Feature") => vec![v],
        _ => return Err(TrackError::Unknown),
    };
    for f in features {
        let props = f.get("properties").cloned().unwrap_or(Value::Null);
        let Some(g) = f.get("geometry") else { continue };
        let coords = g.get("coordinates");
        match g.get("type").and_then(Value::as_str) {
            Some("Point") => {
                let c = coords.and_then(Value::as_array);
                let t = ["time", "timestamp", "datetime", "date"].iter().find_map(|k| props.get(*k).and_then(ts));
                if let (Some(c), Some(t)) = (c, t)
                    && let (Some(lon), Some(lat)) = (c.first().and_then(Value::as_f64), c.get(1).and_then(Value::as_f64))
                {
                    out.push(TrackPoint { utc_ms: t, lat, lon, acc_m: acc(&props, &["accuracy", "acc"], GPS_ACC) });
                }
            }
            Some("LineString") | Some("MultiLineString") => {
                let lines: Vec<&Value> = if g["type"] == "LineString" {
                    coords.into_iter().collect()
                } else {
                    coords.and_then(Value::as_array).map(|l| l.iter().collect()).unwrap_or_default()
                };
                let times = props.get("coordTimes").or(props.get("times")).and_then(Value::as_array);
                let mut k = 0;
                for line in lines {
                    for c in line.as_array().into_iter().flatten() {
                        let c = c.as_array().cloned().unwrap_or_default();
                        let t = times
                            .and_then(|ts_| ts_.get(k))
                            .and_then(|t| match t {
                                Value::Array(inner) => inner.first().and_then(ts),
                                t => ts(t),
                            })
                            // A 4th coordinate is a Unix time in seconds.
                            .or_else(|| c.get(3).and_then(Value::as_f64).map(|s| (s * 1000.0) as i64));
                        if let (Some(lon), Some(lat), Some(t)) = (c.first().and_then(Value::as_f64), c.get(1).and_then(Value::as_f64), t) {
                            out.push(TrackPoint { utc_ms: t, lat, lon, acc_m: GPS_ACC });
                        }
                        k += 1;
                    }
                }
            }
            _ => {}
        }
    }
    Ok((Format::GeoJson, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpx() {
        let x = r#"<?xml version="1.0"?><gpx version="1.1" xmlns="http://www.topografix.com/GPX/1/1"><trk><trkseg>
            <trkpt lat="45.1" lon="10.1"><ele>9</ele><time>2019-08-03T06:02:00Z</time></trkpt>
            <trkpt lat="45.0" lon="10.0"><time>2019-08-03T06:00:00Z</time></trkpt>
            <trkpt lat="1" lon="1"></trkpt></trkseg></trk></gpx>"#;
        let (f, p) = parse(x.as_bytes()).unwrap();
        assert_eq!(f, Format::Gpx);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].lat, 45.0);
        assert_eq!(p[1].utc_ms - p[0].utc_ms, 120_000);
    }

    #[test]
    fn kml_track_and_placemark() {
        let x = r#"<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:gx="http://www.google.com/kml/ext/2.2"><Document>
            <Placemark><gx:Track><when>2020-01-01T10:00:00Z</when><when>2020-01-01T10:01:00Z</when>
            <gx:coord>8.5 47.3 400</gx:coord><gx:coord>8.6 47.4 410</gx:coord></gx:Track></Placemark>
            <Placemark><TimeStamp><when>2020-01-01T12:00:00Z</when></TimeStamp><Point><coordinates>9.0,46.0,0</coordinates></Point></Placemark>
            </Document></kml>"#;
        let (f, p) = parse(x.as_bytes()).unwrap();
        assert_eq!(f, Format::Kml);
        assert_eq!(p.len(), 3);
        assert_eq!((p[0].lat, p[0].lon), (47.3, 8.5));
        assert_eq!((p[2].lat, p[2].lon), (46.0, 9.0));
    }

    #[test]
    fn geojson_line_with_times() {
        let j = r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"coordTimes":["2020-01-01T10:00:00Z","2020-01-01T10:05:00Z"]},
            "geometry":{"type":"LineString","coordinates":[[8.5,47.3],[8.6,47.4]]}},
            {"type":"Feature","properties":{"time":"2020-01-01T12:00:00+01:00"},"geometry":{"type":"Point","coordinates":[9.0,46.0]}}]}"#;
        let (f, p) = parse(j.as_bytes()).unwrap();
        assert_eq!(f, Format::GeoJson);
        assert_eq!(p.len(), 3);
        assert_eq!(p[1].utc_ms - p[0].utc_ms, 5 * 60_000);
        assert_eq!(p[2].utc_ms - p[0].utc_ms, 60 * 60_000);
    }

    #[test]
    fn google_formats() {
        let records = r#"{"locations":[{"latitudeE7":458856412,"longitudeE7":108409739,"accuracy":15,"timestamp":"2019-08-03T06:00:00.000Z"},
            {"latitudeE7":458856000,"longitudeE7":108409000,"timestampMs":"1564812300000"}]}"#;
        let (f, p) = parse(records.as_bytes()).unwrap();
        assert_eq!(f, Format::GoogleRecords);
        assert_eq!(p.len(), 2);
        assert!((p[0].lat - 45.8856412).abs() < 1e-9);
        assert_eq!(p[0].acc_m, 15.0);

        let timeline = r#"{"semanticSegments":[{"startTime":"2019-08-03T08:00:00.000+02:00","endTime":"2019-08-03T14:30:00.000+02:00",
            "visit":{"topCandidate":{"placeLocation":{"latLng":"45.8858000°, 10.8410000°"}}}},
            {"startTime":"2019-08-03T14:30:00.000+02:00","endTime":"2019-08-03T15:20:00.000+02:00",
            "timelinePath":[{"point":"45.8858°, 10.841°","time":"2019-08-03T14:30:00.000+02:00"}]}],
            "rawSignals":[{"position":{"LatLng":"45.88°, 10.84°","accuracyMeters":12,"timestamp":"2019-08-03T08:00:00.000+02:00"}}]}"#;
        let (f, p) = parse(timeline.as_bytes()).unwrap();
        assert_eq!(f, Format::GoogleTimeline);
        // The path's first point repeats the visit's end and is merged.
        assert_eq!(p.len(), 3);
        assert_eq!(p[0].utc_ms, chrono::DateTime::parse_from_rfc3339("2019-08-03T06:00:00Z").unwrap().timestamp_millis());

        let semantic = r#"{"timelineObjects":[{"placeVisit":{"duration":{"startTimestamp":"2019-08-03T06:00:00.000Z","endTimestamp":"2019-08-03T12:30:00.000Z"},
            "location":{"latitudeE7":458858000,"longitudeE7":108410000}}},
            {"activitySegment":{"duration":{"startTimestamp":"2019-08-03T12:30:00.000Z","endTimestamp":"2019-08-03T13:20:00.000Z"},
            "startLocation":{"latitudeE7":458858000,"longitudeE7":108410000},"endLocation":{"latitudeE7":459187000,"longitudeE7":108869000},
            "waypointPath":{"waypoints":[{"latE7":458923800,"lngE7":108501800}]}}}]}"#;
        let (f, p) = parse(semantic.as_bytes()).unwrap();
        assert_eq!(f, Format::GoogleSemantic);
        // Visit start and end, then the activity; its start repeats the visit's end.
        assert_eq!(p.len(), 4);
    }

    #[test]
    fn unknown() {
        assert!(matches!(parse(b"hello"), Err(TrackError::Unknown)));
        assert!(matches!(parse(br#"{"a":1}"#), Err(TrackError::Unknown)));
    }

    /// The generated fixtures, when present.
    #[test]
    fn fixture_files() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out/location-history");
        if !dir.exists() {
            return;
        }
        for (rel, want) in [
            ("lake-garda-2019.gpx", Format::Gpx),
            ("dolomites-hike-2022-07-16.gpx", Format::Gpx),
            ("Timeline.json", Format::GoogleTimeline),
            ("Takeout/Location History/Records.json", Format::GoogleRecords),
            ("Takeout/Location History/Semantic Location History/2019/2019_AUGUST.json", Format::GoogleSemantic),
        ] {
            let (f, p) = read_file(&dir.join(rel)).unwrap();
            assert_eq!(f, want, "{rel}");
            assert!(p.len() > 20, "{rel}: {} points", p.len());
            assert!(p.windows(2).all(|w| w[0].utc_ms <= w[1].utc_ms));
        }
    }
}
