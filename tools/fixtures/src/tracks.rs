//! A simulated movement track and its export in the four location history
//! formats Roll imports.

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, TimeZone, Utc};
use serde_json::{Value, json};

use crate::pixels::Rng;

#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub t: DateTime<Utc>,
    pub lat: f64,
    pub lon: f64,
    pub ele: f64,
}

#[derive(Debug, Clone)]
pub struct Stop {
    pub lat: f64,
    pub lon: f64,
    pub name: &'static str,
}

/// One point per minute. Between `day_start` and `day_end` local time the
/// person visits the day's stops (with a small random walk) and travels
/// between them; at night they stay at the last stop.
#[derive(Debug, Clone)]
pub struct Track {
    pub points: Vec<Point>,
    pub offset: FixedOffset,
    pub stops: Vec<(DateTime<Utc>, DateTime<Utc>, usize)>,
    pub stop_list: Vec<Stop>,
}

impl Track {
    pub fn simulate(days: &[(NaiveDate, Vec<Stop>)], offset_min: i32, seed: u64) -> Track {
        let offset = FixedOffset::east_opt(offset_min * 60).unwrap();
        let mut rng = Rng::new(seed);
        let mut points = Vec::new();
        let mut visits = Vec::new();
        let mut stop_list: Vec<Stop> = Vec::new();
        for (date, stops) in days {
            let start = offset.from_local_datetime(&date.and_hms_opt(8, 0, 0).unwrap()).unwrap().with_timezone(&Utc);
            let per = Duration::minutes(13 * 60 / stops.len() as i64);
            for (i, stop) in stops.iter().enumerate() {
                let idx = stop_list.len();
                stop_list.push(stop.clone());
                let seg_start = start + per * i as i32;
                let travel = if i == 0 { Duration::zero() } else { Duration::minutes(50) };
                let prev = if i == 0 { None } else { Some(&stops[i - 1]) };
                let mut t = seg_start;
                let (mut lat, mut lon) = (stop.lat, stop.lon);
                while t < seg_start + per {
                    let into = t - seg_start;
                    if let (Some(p), true) = (prev, into < travel) {
                        let f = into.num_seconds() as f64 / travel.num_seconds() as f64;
                        lat = p.lat + (stop.lat - p.lat) * f;
                        lon = p.lon + (stop.lon - p.lon) * f;
                    } else {
                        lat += (rng.f() - 0.5) * 0.0004;
                        lon += (rng.f() - 0.5) * 0.0004;
                        lat += (stop.lat - lat) * 0.05;
                        lon += (stop.lon - lon) * 0.05;
                    }
                    points.push(Point { t, lat, lon, ele: 80.0 + rng.f() * 40.0 });
                    t += Duration::minutes(1);
                }
                visits.push((seg_start + travel, seg_start + per, idx));
            }
        }
        Track { points, offset, stops: visits, stop_list }
    }

    /// A track from explicit points (a hike, sampled every `step` seconds).
    pub fn path(start: DateTime<Utc>, offset_min: i32, waypoints: &[(f64, f64, f64)], minutes: i64, step_secs: i64) -> Track {
        let offset = FixedOffset::east_opt(offset_min * 60).unwrap();
        let n = (minutes * 60 / step_secs) as usize;
        let segs = waypoints.len() - 1;
        let points = (0..=n)
            .map(|i| {
                let f = i as f64 / n as f64 * segs as f64;
                let k = (f.floor() as usize).min(segs - 1);
                let u = f - k as f64;
                let (a, b) = (waypoints[k], waypoints[k + 1]);
                Point {
                    t: start + Duration::seconds(i as i64 * step_secs),
                    lat: a.0 + (b.0 - a.0) * u,
                    lon: a.1 + (b.1 - a.1) * u,
                    ele: a.2 + (b.2 - a.2) * u,
                }
            })
            .collect();
        Track { points, offset, stops: Vec::new(), stop_list: Vec::new() }
    }

    pub fn at(&self, t: DateTime<Utc>) -> Point {
        let i = self.points.partition_point(|p| p.t <= t);
        if i == 0 {
            return self.points[0];
        }
        if i >= self.points.len() {
            return *self.points.last().unwrap();
        }
        let (a, b) = (self.points[i - 1], self.points[i]);
        let f = (t - a.t).num_milliseconds() as f64 / (b.t - a.t).num_milliseconds().max(1) as f64;
        Point { t, lat: a.lat + (b.lat - a.lat) * f, lon: a.lon + (b.lon - a.lon) * f, ele: a.ele + (b.ele - a.ele) * f }
    }

    pub fn gpx(&self, name: &str, every: usize) -> String {
        let mut s = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"roll-fixtures\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n",
        );
        s.push_str(&format!("  <trk><name>{name}</name><trkseg>\n"));
        for p in self.points.iter().step_by(every.max(1)) {
            s.push_str(&format!(
                "    <trkpt lat=\"{:.7}\" lon=\"{:.7}\"><ele>{:.1}</ele><time>{}</time></trkpt>\n",
                p.lat,
                p.lon,
                p.ele,
                p.t.format("%Y-%m-%dT%H:%M:%SZ")
            ));
        }
        s.push_str("  </trkseg></trk>\n</gpx>\n");
        s
    }

    fn local(&self, t: DateTime<Utc>) -> String {
        t.with_timezone(&self.offset).format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string()
    }

    /// Google Maps Timeline phone export (`Timeline.json`).
    pub fn timeline_json(&self, home: (f64, f64)) -> String {
        let ll = |lat: f64, lon: f64| format!("{lat:.7}°, {lon:.7}°");
        let mut segments = Vec::new();
        let mut last_end: Option<(DateTime<Utc>, usize)> = None;
        for &(start, end, idx) in &self.stops {
            let stop = &self.stop_list[idx];
            if let Some((prev_end, prev_idx)) = last_end
                && start > prev_end
            {
                let prev = &self.stop_list[prev_idx];
                let path: Vec<Value> = self
                    .points
                    .iter()
                    .filter(|p| p.t >= prev_end && p.t <= start)
                    .step_by(5)
                    .map(|p| json!({"point": ll(p.lat, p.lon), "time": self.local(p.t)}))
                    .collect();
                segments.push(json!({
                    "startTime": self.local(prev_end), "endTime": self.local(start),
                    "startTimeTimezoneUtcOffsetMinutes": self.offset.local_minus_utc() / 60,
                    "endTimeTimezoneUtcOffsetMinutes": self.offset.local_minus_utc() / 60,
                    "timelinePath": path
                }));
                segments.push(json!({
                    "startTime": self.local(prev_end), "endTime": self.local(start),
                    "activity": {
                        "start": {"latLng": ll(prev.lat, prev.lon)}, "end": {"latLng": ll(stop.lat, stop.lon)},
                        "distanceMeters": 12000.0, "topCandidate": {"type": "IN_PASSENGER_VEHICLE", "probability": 0.9}
                    }
                }));
            }
            segments.push(json!({
                "startTime": self.local(start), "endTime": self.local(end),
                "visit": {"hierarchyLevel": 0, "probability": 0.9, "topCandidate": {
                    "placeId": format!("fixture-{idx}"), "semanticType": "UNKNOWN", "probability": 0.8,
                    "placeLocation": {"latLng": ll(stop.lat, stop.lon)}
                }}
            }));
            last_end = Some((end, idx));
        }
        let raw: Vec<Value> = self
            .points
            .iter()
            .step_by(30)
            .map(|p| json!({"position": {"LatLng": ll(p.lat, p.lon), "accuracyMeters": 12, "altitudeMeters": p.ele, "source": "WIFI", "timestamp": self.local(p.t)}}))
            .collect();
        serde_json::to_string_pretty(&json!({
            "semanticSegments": segments,
            "rawSignals": raw,
            "userLocationProfile": {"frequentPlaces": [{"placeId": "home", "placeLocation": ll(home.0, home.1), "label": "HOME"}]}
        }))
        .unwrap()
    }

    /// Legacy Takeout `Records.json`.
    pub fn records_json(&self) -> String {
        let locs: Vec<Value> = self
            .points
            .iter()
            .step_by(5)
            .map(|p| {
                json!({
                    "latitudeE7": (p.lat * 1e7).round() as i64, "longitudeE7": (p.lon * 1e7).round() as i64,
                    "accuracy": 15, "altitude": p.ele.round() as i64,
                    "timestamp": p.t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
                })
            })
            .collect();
        serde_json::to_string_pretty(&json!({ "locations": locs })).unwrap()
    }

    /// Semantic Location History, one month file.
    pub fn semantic_json(&self) -> String {
        let e7 = |v: f64| (v * 1e7).round() as i64;
        let ts = |t: DateTime<Utc>| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        let mut objs = Vec::new();
        let mut last: Option<(DateTime<Utc>, usize)> = None;
        for &(start, end, idx) in &self.stops {
            let stop = &self.stop_list[idx];
            if let Some((prev_end, prev_idx)) = last {
                let prev = &self.stop_list[prev_idx];
                let way: Vec<Value> = self
                    .points
                    .iter()
                    .filter(|p| p.t >= prev_end && p.t <= start)
                    .step_by(10)
                    .map(|p| json!({"latE7": e7(p.lat), "lngE7": e7(p.lon)}))
                    .collect();
                objs.push(json!({"activitySegment": {
                    "startLocation": {"latitudeE7": e7(prev.lat), "longitudeE7": e7(prev.lon)},
                    "endLocation": {"latitudeE7": e7(stop.lat), "longitudeE7": e7(stop.lon)},
                    "duration": {"startTimestamp": ts(prev_end), "endTimestamp": ts(start)},
                    "activityType": "IN_PASSENGER_VEHICLE",
                    "waypointPath": {"waypoints": way}
                }}));
            }
            objs.push(json!({"placeVisit": {
                "location": {"latitudeE7": e7(stop.lat), "longitudeE7": e7(stop.lon), "name": stop.name},
                "duration": {"startTimestamp": ts(start), "endTimestamp": ts(end)}
            }}));
            last = Some((end, idx));
        }
        serde_json::to_string_pretty(&json!({ "timelineObjects": objs })).unwrap()
    }
}
