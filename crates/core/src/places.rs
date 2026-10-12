//! Fix places: imported location history, grouped suggestions for items
//! without a location, the manual tools, and the time zone follow-up.
//!
//! Suggestions come from, best first:
//! 1. imported tracks (GPX, KML, GeoJSON, Google Timeline), interpolated;
//! 2. the owner's own geotagged photos close in time ("time neighbors");
//! 3. place names in folder names ("Italy 2019"), from the gazetteer.
//!
//! Items whose file states no UTC offset are matched on local time: their
//! wall-clock time is right, their UTC is a guess.

use std::collections::HashMap;
use std::path::Path;

use chrono::NaiveDateTime;
use roll_geo::gazetteer::Gazetteer;
use roll_geo::interp::{self, Options};
use roll_geo::tracks::{self, TrackPoint};
use roll_geo::{Place, distance_m, tz};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use crate::db::now_ms;
use crate::edits::{self, Change};
use crate::model::Scope;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TrackInfo {
    pub id: i32,
    pub name: String,
    pub format: String,
    pub count: i32,
    #[ts(type = "number | null")]
    pub start_utc: Option<i64>,
    #[ts(type = "number | null")]
    pub end_utc: Option<i64>,
    #[ts(type = "number")]
    pub imported_at: i64,
}

const POINT_BYTES: usize = 28;

fn pack(points: &[TrackPoint]) -> Vec<u8> {
    let mut b = Vec::with_capacity(points.len() * POINT_BYTES);
    for p in points {
        b.extend_from_slice(&p.utc_ms.to_le_bytes());
        b.extend_from_slice(&p.lat.to_le_bytes());
        b.extend_from_slice(&p.lon.to_le_bytes());
        b.extend_from_slice(&p.acc_m.to_le_bytes());
    }
    b
}

fn unpack(b: &[u8]) -> Vec<TrackPoint> {
    b.as_chunks::<POINT_BYTES>()
        .0
        .iter()
        .map(|c| TrackPoint {
            utc_ms: i64::from_le_bytes(c[0..8].try_into().unwrap()),
            lat: f64::from_le_bytes(c[8..16].try_into().unwrap()),
            lon: f64::from_le_bytes(c[16..24].try_into().unwrap()),
            acc_m: f32::from_le_bytes(c[24..28].try_into().unwrap()),
        })
        .collect()
}

/// Import a location history file. Points stay in the database; the file
/// itself is not kept.
pub fn import_track(c: &Connection, path: &Path) -> anyhow::Result<TrackInfo> {
    let (format, points) = tracks::read_file(path)?;
    if points.is_empty() {
        anyhow::bail!("{} has no timed positions.", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (start, end) = (points.first().map(|p| p.utc_ms), points.last().map(|p| p.utc_ms));
    c.execute(
        "INSERT INTO tracks (name, format, points, count, start_utc, end_utc, imported_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![name, format.label(), pack(&points), points.len() as i64, start, end, now_ms()],
    )?;
    let id = c.last_insert_rowid();
    Ok(TrackInfo {
        id: id as i32,
        name,
        format: format.label().into(),
        count: points.len() as i32,
        start_utc: start,
        end_utc: end,
        imported_at: now_ms(),
    })
}

pub fn tracks(c: &Connection) -> rusqlite::Result<Vec<TrackInfo>> {
    let mut st = c.prepare("SELECT id, name, format, count, start_utc, end_utc, imported_at FROM tracks ORDER BY start_utc")?;
    st.query_map([], |r| {
        Ok(TrackInfo {
            id: r.get(0)?,
            name: r.get(1)?,
            format: r.get(2)?,
            count: r.get(3)?,
            start_utc: r.get(4)?,
            end_utc: r.get(5)?,
            imported_at: r.get(6)?,
        })
    })?
    .collect()
}

pub fn delete_track(c: &Connection, id: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM tracks WHERE id = ?1", [id])?;
    Ok(())
}

/// A search result for "Set location…".
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlaceHit {
    pub label: String,
    pub name: String,
    /// "city", "region" or "country"; pasted coordinates are "city".
    pub kind: String,
    pub lat: f64,
    pub lon: f64,
    pub accuracy_m: f64,
}

impl From<roll_geo::gazetteer::Hit> for PlaceHit {
    fn from(h: roll_geo::gazetteer::Hit) -> Self {
        let kind = match h.kind {
            roll_geo::gazetteer::Kind::City => "city",
            roll_geo::gazetteer::Kind::Region => "region",
            roll_geo::gazetteer::Kind::Country => "country",
        };
        PlaceHit { label: h.label, name: h.name, kind: kind.into(), lat: h.lat, lon: h.lon, accuracy_m: h.accuracy_m }
    }
}

/// What setting places did, and the time zones to offer next.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LocationResult {
    pub count: i32,
    pub zones: Vec<ZoneProposal>,
}

/// A group of items that get the same kind of suggestion.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlaceGroup {
    pub key: String,
    /// "track", "neighbors" or "name".
    pub source: String,
    /// "lake-garda-2019.gpx", "41 of your photos", "the folder “Italy 2019”".
    pub evidence: String,
    /// Where: "Riva del Garda, Trentino-Alto Adige, Italy", or coordinates.
    pub place: String,
    pub count: i32,
    pub from_local: String,
    pub to_local: String,
    pub lat: f64,
    pub lon: f64,
    /// Typical accuracy radius of the suggestions in metres.
    pub accuracy_m: f64,
    pub ids: Vec<i32>,
}

/// One item's suggestion, kept for accepting a group.
#[derive(Debug, Clone)]
pub struct Suggested {
    pub id: i64,
    pub place: Place,
    pub source: &'static str,
    pub why: String,
}

#[derive(Debug, Clone)]
struct Candidate {
    id: i64,
    utc: i64,
    /// Local wall time as if it were UTC, for matching items without an offset.
    local: i64,
    local_str: String,
    utc_reliable: bool,
    dir: String,
}

fn local_ms(s: &str) -> Option<i64> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok().map(|d| d.and_utc().timestamp_millis())
}

/// A point list in UTC and the same points in local wall time.
struct Points {
    name: String,
    by_utc: Vec<TrackPoint>,
    by_local: Vec<TrackPoint>,
}

impl Points {
    fn new(name: String, mut by_utc: Vec<TrackPoint>, mut by_local: Vec<TrackPoint>) -> Self {
        by_utc.sort_by_key(|p| p.utc_ms);
        by_local.sort_by_key(|p| p.utc_ms);
        Points { name, by_utc, by_local }
    }

    /// Track points in local time, from the zone at each point.
    fn from_track(name: String, pts: Vec<TrackPoint>) -> Self {
        let mut zone_cache: HashMap<(i32, i32), Option<String>> = HashMap::new();
        let local: Vec<TrackPoint> = pts
            .iter()
            .filter_map(|p| {
                let zone = zone_cache
                    .entry(((p.lat * 10.0) as i32, (p.lon * 10.0) as i32))
                    .or_insert_with(|| tz::zone_at(p.lat, p.lon))
                    .clone()?;
                let off = tz::offset_at_utc(&zone, p.utc_ms)?;
                Some(TrackPoint { utc_ms: p.utc_ms + off as i64 * 60_000, ..*p })
            })
            .collect();
        Points::new(name, pts, local)
    }

    fn at(&self, c: &Candidate, o: &Options) -> Option<Place> {
        if c.utc_reliable { interp::at(&self.by_utc, c.utc, o) } else { interp::at(&self.by_local, c.local, o) }
    }
}

fn area(gaz: Option<&Gazetteer>, lat: f64, lon: f64) -> String {
    match gaz.and_then(|g| g.reverse(lat, lon)) {
        Some(e) => e.label(),
        None => format!("{lat:.4}, {lon:.4}"),
    }
}

/// Per-item suggestions by group key.
pub type ByGroup = HashMap<String, Vec<Suggested>>;

/// Compute suggestion groups. Returns the groups for the UI and the
/// per-item suggestions by group key.
pub fn suggest(c: &Connection, gaz: Option<&Gazetteer>, window_min: i64) -> anyhow::Result<(Vec<PlaceGroup>, ByGroup)> {
    suggest_in(c, gaz, window_min, Scope::Archive)
}

/// [`suggest`] for the archive or one import's items. Neighbours always come
/// from the archive; an import's group keys start with `import<root>:`.
pub fn suggest_in(c: &Connection, gaz: Option<&Gazetteer>, window_min: i64, scope: Scope) -> anyhow::Result<(Vec<PlaceGroup>, ByGroup)> {
    let candidates: Vec<Candidate> = {
        let mut st = c.prepare(&format!(
            "SELECT id, taken_utc, taken_local, tz_offset, date_source, dir FROM assets
             WHERE lat IS NULL AND {} AND trashed_at IS NULL AND (group_role IS NULL OR group_role = 'primary')
             ORDER BY coalesce(taken_local, '9999'), id",
            scope.sql("")
        ))?;
        st.query_map([], |r| {
            let local: Option<String> = r.get(2)?;
            let offset: Option<i64> = r.get(3)?;
            let source: Option<String> = r.get(4)?;
            Ok(Candidate {
                id: r.get(0)?,
                utc: r.get::<_, Option<i64>>(1)?.unwrap_or(i64::MIN),
                local: local.as_deref().and_then(local_ms).unwrap_or(i64::MIN),
                local_str: local.unwrap_or_default(),
                utc_reliable: offset.is_some() || source.as_deref() == Some("quicktime"),
                dir: r.get(5)?,
            })
        })?
        .collect::<Result<_, _>>()?
    };
    let mut sources: Vec<Points> = Vec::new();
    {
        let mut st = c.prepare("SELECT name, points FROM tracks ORDER BY start_utc")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
        for row in rows {
            let (name, blob) = row?;
            sources.push(Points::from_track(name, unpack(&blob)));
        }
    }
    let own = {
        let mut st = c.prepare(
            "SELECT taken_utc, taken_local, tz_offset, date_source, lat, lon, coalesce(loc_acc, 10) FROM assets
             WHERE lat IS NOT NULL AND taken_utc IS NOT NULL AND missing = 0 AND trashed_at IS NULL
               AND coalesce(loc_source, 'file') IN ('file', 'manual', 'sidecar', 'takeout')",
        )?;
        let mut by_utc = Vec::new();
        let mut by_local = Vec::new();
        for row in st.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, f64>(5)?,
                r.get::<_, f64>(6)?,
            ))
        })? {
            let (utc, local, offset, source, lat, lon, acc) = row?;
            let p = TrackPoint { utc_ms: utc, lat, lon, acc_m: acc as f32 };
            if offset.is_some() || source.as_deref() == Some("quicktime") {
                by_utc.push(p);
            }
            if let Some(l) = local.as_deref().and_then(local_ms) {
                by_local.push(TrackPoint { utc_ms: l, ..p });
            }
        }
        Points::new(String::new(), by_utc, by_local)
    };
    let dismissed: std::collections::HashSet<String> = {
        let mut st = c.prepare("SELECT key FROM dismissed WHERE kind = 'place'")?;
        st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
    };

    let opts = Options { window_ms: window_min.max(1) * 60_000, ..Default::default() };
    // (source kind, source index or name key, suggestion)
    let mut found: Vec<(&Candidate, &'static str, String, Place)> = Vec::new();
    let mut name_cache: HashMap<String, Option<roll_geo::gazetteer::Hit>> = HashMap::new();
    for cand in &candidates {
        if cand.local != i64::MIN || cand.utc != i64::MIN {
            let mut best: Option<(&'static str, String, Place)> = None;
            for s in &sources {
                if let Some(p) = s.at(cand, &opts)
                    && best.as_ref().is_none_or(|b| p.accuracy_m < b.2.accuracy_m)
                {
                    best = Some(("track", s.name.clone(), p));
                }
            }
            // Own photos only when no track knows better.
            if best.is_none()
                && let Some(p) = own.at(cand, &opts)
            {
                best = Some(("neighbors", String::new(), p));
            }
            if let Some((kind, key, p)) = best {
                found.push((cand, kind, key, p));
                continue;
            }
        }
        if let Some(g) = gaz {
            let hit = name_cache.entry(cand.dir.clone()).or_insert_with(|| {
                // The innermost folder that names a place.
                cand.dir.rsplit('/').find_map(|seg| g.in_text(seg))
            });
            if let Some(h) = hit {
                found.push((cand, "name", h.label.clone(), Place { lat: h.lat, lon: h.lon, accuracy_m: h.accuracy_m }));
            }
        }
    }

    // Group: same source, close in time, close in space.
    struct Acc<'a> {
        kind: &'static str,
        key: String,
        items: Vec<(&'a Candidate, Place)>,
        last_local: i64,
    }
    let mut groups: Vec<Acc> = Vec::new();
    for (cand, kind, key, place) in found {
        let t = if cand.local != i64::MIN { cand.local } else { cand.utc };
        let fits = |g: &Acc| {
            g.kind == kind && g.key == key && {
                if kind == "name" {
                    true
                } else {
                    let (la, lo) = centroid(&g.items);
                    t - g.last_local < 12 * 3_600_000 && distance_m(la, lo, place.lat, place.lon) < 50_000.0
                }
            }
        };
        let idx = if kind == "name" { groups.iter().position(fits) } else { groups.last().filter(|g| fits(g)).map(|_| groups.len() - 1) };
        match idx {
            Some(i) => {
                groups[i].items.push((cand, place));
                groups[i].last_local = groups[i].last_local.max(t);
            }
            None => groups.push(Acc { kind, key, items: vec![(cand, place)], last_local: t }),
        }
    }

    let mut out = Vec::new();
    let mut by_key = HashMap::new();
    for g in groups {
        let (lat, lon) = centroid(&g.items);
        let place = match g.kind {
            "name" => g.key.clone(),
            _ => area(gaz, lat, lon),
        };
        let from = g.items.iter().map(|(c, _)| c.local_str.as_str()).filter(|s| !s.is_empty()).min().unwrap_or("").to_string();
        let to = g.items.iter().map(|(c, _)| c.local_str.as_str()).max().unwrap_or("").to_string();
        let prefix = match scope {
            Scope::Archive => String::new(),
            Scope::Staged(root) => format!("import{root}:"),
        };
        let key = format!("{prefix}{}:{}:{}:{}", g.kind, g.key, from.get(..10).unwrap_or(""), to.get(..10).unwrap_or(""));
        if dismissed.contains(&key) {
            continue;
        }
        let mut accs: Vec<f64> = g.items.iter().map(|(_, p)| p.accuracy_m).collect();
        accs.sort_by(f64::total_cmp);
        let evidence = match g.kind {
            "track" => g.key.clone(),
            "neighbors" => "your geotagged photos taken around the same time".to_string(),
            _ => {
                let dir = g.items[0].0.dir.rsplit('/').next().unwrap_or("").to_string();
                format!("the folder “{dir}”")
            }
        };
        let why_base = match g.kind {
            "track" => format!("Interpolated from {}", g.key),
            "neighbors" => "From your photos taken just before and after".to_string(),
            _ => format!("From the folder name, {}", g.key),
        };
        let suggested: Vec<Suggested> = g
            .items
            .iter()
            .map(|(c, p)| Suggested {
                id: c.id,
                place: *p,
                source: match g.kind {
                    "track" => "track",
                    "neighbors" => "neighbors",
                    _ => "name",
                },
                why: format!("{why_base}, ±{}.", radius(p.accuracy_m)),
            })
            .collect();
        out.push(PlaceGroup {
            key: key.clone(),
            source: g.kind.to_string(),
            evidence,
            place,
            count: g.items.len() as i32,
            from_local: from,
            to_local: to,
            lat,
            lon,
            accuracy_m: accs[accs.len() / 2],
            ids: g.items.iter().map(|(c, _)| c.id as i32).collect(),
        });
        by_key.insert(key, suggested);
    }
    // Largest groups first.
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.from_local.cmp(&b.from_local)));
    Ok((out, by_key))
}

fn centroid(items: &[(&Candidate, Place)]) -> (f64, f64) {
    let n = items.len().max(1) as f64;
    (items.iter().map(|(_, p)| p.lat).sum::<f64>() / n, items.iter().map(|(_, p)| p.lon).sum::<f64>() / n)
}

/// "40 m", "3 km".
pub fn radius(m: f64) -> String {
    if m < 1000.0 { format!("{} m", (m / 10.0).round() as i64 * 10) } else { format!("{} km", (m / 1000.0).round() as i64) }
}

fn location_value(p: &Place, source: &str) -> Value {
    json!({"lat": p.lat, "lon": p.lon, "alt": null, "acc": p.accuracy_m, "source": source})
}

/// Set suggested locations as one undoable batch. Returns the batch id.
pub fn accept(c: &Connection, items: &[Suggested], label: &str) -> rusqlite::Result<i64> {
    let batch = edits::begin_batch(c, "edit", label)?;
    let ids: Vec<i64> = items.iter().map(|s| s.id).collect();
    let all = crate::trash::with_groups(c, &ids)?;
    let by_id: HashMap<i64, &Suggested> = items.iter().map(|s| (s.id, s)).collect();
    // Group members (a Live Photo's video) share the primary's place.
    let group_of: HashMap<i64, i64> = {
        let mut m = HashMap::new();
        for id in &all {
            if by_id.contains_key(id) {
                continue;
            }
            let primary: Option<i64> = c
                .query_row(
                    "SELECT p.id FROM assets a JOIN assets p ON p.group_id = a.group_id AND p.id != a.id WHERE a.id = ?1 AND a.group_id IS NOT NULL",
                    [id],
                    |r| r.get(0),
                )
                .ok();
            if let Some(p) = primary.filter(|p| by_id.contains_key(p)) {
                m.insert(*id, p);
            }
        }
        m
    };
    for id in all {
        let Some(s) = by_id.get(&id).or_else(|| group_of.get(&id).and_then(|p| by_id.get(p))) else { continue };
        edits::set(
            c,
            Some(batch),
            id,
            &Change {
                field: "location",
                value: location_value(&s.place, s.source),
                source: s.source,
                source_ref: None,
                conf: Some(interp::confidence(s.place.accuracy_m)),
                why: Some(s.why.clone()),
            },
        )?;
    }
    Ok(batch)
}

/// Set (or with `None` clear) the location of items by hand.
pub fn set_location(c: &Connection, ids: &[i64], place: Option<Place>, label: &str, why: Option<String>) -> rusqlite::Result<(i64, usize)> {
    let all = crate::trash::with_groups(c, ids)?;
    let batch = edits::begin_batch(c, "edit", label)?;
    let mut n = 0;
    for id in all {
        let value = place.as_ref().map(|p| location_value(p, "manual")).unwrap_or(Value::Null);
        if edits::set(
            c,
            Some(batch),
            id,
            &Change { field: "location", value, source: "manual", source_ref: None, conf: None, why: why.clone() },
        )? {
            n += 1;
        }
    }
    if n == 0 {
        c.execute("DELETE FROM batches WHERE id = ?1", [batch])?;
    }
    Ok((batch, n))
}

pub fn dismiss(c: &Connection, kind: &str, key: &str) -> rusqlite::Result<()> {
    c.execute("INSERT OR REPLACE INTO dismissed (kind, key, at) VALUES (?1, ?2, ?3)", params![kind, key, now_ms()])?;
    Ok(())
}

/// Time zone follow-up: items with a location but no stated offset, by the
/// zone at their place and the offset it gives for their date.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ZoneProposal {
    pub zone: String,
    pub offset_min: i32,
    pub count: i32,
    pub ids: Vec<i32>,
}

type PlaceRow = (f64, f64, Option<String>, Option<i64>, Option<String>, Option<i64>);

pub fn zone_proposals(c: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<ZoneProposal>> {
    let mut by: HashMap<(String, i32), Vec<i32>> = HashMap::new();
    for id in ids {
        let row: Option<PlaceRow> = c
            .query_row(
                "SELECT lat, lon, taken_local, taken_utc, date_source, tz_offset FROM assets WHERE id = ?1 AND lat IS NOT NULL",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .ok();
        let Some((lat, lon, local, utc, source, offset)) = row else { continue };
        if offset.is_some() {
            continue;
        }
        let Some(zone) = tz::zone_at(lat, lon) else { continue };
        let off = if source.as_deref() == Some("quicktime") {
            utc.and_then(|u| tz::offset_at_utc(&zone, u))
        } else {
            local
                .as_deref()
                .and_then(|l| NaiveDateTime::parse_from_str(l, "%Y-%m-%dT%H:%M:%S").ok())
                .and_then(|l| tz::offset_for_local(&zone, l))
        };
        if let Some(off) = off {
            by.entry((zone, off)).or_default().push(*id as i32);
        }
    }
    let mut out: Vec<ZoneProposal> =
        by.into_iter().map(|((zone, offset_min), ids)| ZoneProposal { zone, offset_min, count: ids.len() as i32, ids }).collect();
    out.sort_by_key(|z| std::cmp::Reverse(z.count));
    Ok(out)
}

/// Set the stated offset. Wall-clock time stays for photos (the camera
/// showed local time); for videos with a UTC QuickTime date, UTC stays.
pub fn set_offset(c: &Connection, batch: i64, id: i64, offset_min: i32, why: &str) -> rusqlite::Result<bool> {
    let cur = edits::read(c, id, "date")?;
    let (Some(local), Some(utc)) = (cur.get("local").and_then(Value::as_str), cur.get("utc").and_then(Value::as_i64)) else {
        return Ok(false);
    };
    let source = cur.get("source").and_then(Value::as_str).unwrap_or("manual").to_string();
    let Some(l) = NaiveDateTime::parse_from_str(local, "%Y-%m-%dT%H:%M:%S").ok() else { return Ok(false) };
    let (new_local, new_utc) = if source == "quicktime" {
        (DateTimeUtil::local_from_utc(utc, offset_min), utc)
    } else {
        (l, l.and_utc().timestamp_millis() - offset_min as i64 * 60_000)
    };
    let value = json!({
        "utc": new_utc,
        "local": new_local.format("%Y-%m-%dT%H:%M:%S").to_string(),
        "offset": offset_min,
        "source": source,
        "conf": cur.get("conf"),
    });
    edits::set(
        c,
        Some(batch),
        id,
        &Change { field: "date", value, source: "manual", source_ref: None, conf: None, why: Some(why.to_string()) },
    )
}

struct DateTimeUtil;

impl DateTimeUtil {
    fn local_from_utc(utc_ms: i64, offset_min: i32) -> NaiveDateTime {
        chrono::DateTime::from_timestamp_millis(utc_ms + offset_min as i64 * 60_000).unwrap_or_default().naive_utc()
    }
}

/// "UTC+2", "UTC−4", "UTC+5:30".
pub fn offset_label(m: i32) -> String {
    let sign = if m < 0 { '−' } else { '+' };
    let a = m.abs();
    if a % 60 == 0 { format!("UTC{sign}{}", a / 60) } else { format!("UTC{sign}{}:{:02}", a / 60, a % 60) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    fn set_date(c: &Connection, id: i64, local: &str, offset: Option<i64>) {
        let l = NaiveDateTime::parse_from_str(local, "%Y-%m-%dT%H:%M:%S").unwrap();
        let utc = l.and_utc().timestamp_millis() - offset.unwrap_or(120) * 60_000;
        c.execute(
            "UPDATE assets SET taken_local = ?2, taken_utc = ?3, tz_offset = ?4, date_source = 'exif_original' WHERE id = ?1",
            params![id, local, utc, offset],
        )
        .unwrap();
    }

    #[test]
    fn pack_round_trip() {
        let p = vec![TrackPoint { utc_ms: 5, lat: 45.1, lon: -10.2, acc_m: 12.5 }];
        assert_eq!(unpack(&pack(&p)), p);
    }

    #[test]
    fn track_suggestions_match_local_time_without_offset() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let dir = tempfile::tempdir().unwrap();
        let gpx = dir.path().join("walk.gpx");
        // Lake Garda, 06:00–06:30 UTC, which is 08:00–08:30 in Italy.
        std::fs::write(
            &gpx,
            r#"<gpx><trk><trkseg>
            <trkpt lat="45.80" lon="10.80"><time>2019-08-03T06:00:00Z</time></trkpt>
            <trkpt lat="45.81" lon="10.80"><time>2019-08-03T06:30:00Z</time></trkpt>
            </trkseg></trk></gpx>"#,
        )
        .unwrap();
        let t = import_track(&c, &gpx).unwrap();
        assert_eq!(t.count, 2);
        // No offset in the file: matched on wall-clock time 08:15.
        let a = insert_asset(&c, "Dump/DSC_1.JPG");
        set_date(&c, a, "2019-08-03T08:15:00", None);
        // With an offset: matched on UTC.
        let b = insert_asset(&c, "Dump/IMG_2.JPG");
        set_date(&c, b, "2019-08-03T08:15:00", Some(120));
        // Far away in time: no suggestion.
        let x = insert_asset(&c, "Dump/IMG_3.JPG");
        set_date(&c, x, "2019-09-03T08:15:00", Some(120));
        let (groups, by_key) = suggest(&c, None, 30).unwrap();
        assert_eq!(groups.len(), 1, "{groups:?}");
        let g = &groups[0];
        assert_eq!((g.source.as_str(), g.count), ("track", 2));
        let s = &by_key[&g.key];
        assert!((s[0].place.lat - 45.805).abs() < 1e-6);
        let batch = accept(&c, s, "Set the location of 2 items from walk.gpx").unwrap();
        let src: String = c.query_row("SELECT loc_source FROM assets WHERE id = ?1", [a], |r| r.get(0)).unwrap();
        assert_eq!(src, "track");
        // Time zone follow-up: only the item without an offset.
        let z = zone_proposals(&c, &[a, b]).unwrap();
        assert_eq!(z.len(), 1);
        assert_eq!((z[0].zone.as_str(), z[0].offset_min, z[0].count), ("Europe/Rome", 120, 1));
        edits::undo_batch(&c, batch).unwrap();
        let lat: Option<f64> = c.query_row("SELECT lat FROM assets WHERE id = ?1", [a], |r| r.get(0)).unwrap();
        assert!(lat.is_none());
        // Dismissed groups stay away.
        dismiss(&c, "place", &g.key).unwrap();
        assert!(suggest(&c, None, 30).unwrap().0.is_empty());
    }

    #[test]
    fn neighbors_and_offsets() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let p1 = insert_asset(&c, "Dump/IMG_1.JPG");
        set_date(&c, p1, "2019-08-03T10:00:00", Some(120));
        c.execute("UPDATE assets SET lat = 41.9, lon = 12.5, loc_source = 'file' WHERE id = ?1", [p1]).unwrap();
        let p2 = insert_asset(&c, "Dump/IMG_2.JPG");
        set_date(&c, p2, "2019-08-03T10:20:00", Some(120));
        c.execute("UPDATE assets SET lat = 41.91, lon = 12.5, loc_source = 'file' WHERE id = ?1", [p2]).unwrap();
        let cam = insert_asset(&c, "Dump/DSC_1.JPG");
        set_date(&c, cam, "2019-08-03T10:10:00", None);
        let (groups, _) = suggest(&c, None, 30).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].source, "neighbors");
        let batch = edits::begin_batch(&c, "edit", "zone").unwrap();
        c.execute("UPDATE assets SET lat = 41.905, lon = 12.5 WHERE id = ?1", [cam]).unwrap();
        let z = zone_proposals(&c, &[cam]).unwrap();
        set_offset(&c, batch, cam, z[0].offset_min, "Time zone from the place").unwrap();
        let (local, off): (String, i64) =
            c.query_row("SELECT taken_local, tz_offset FROM assets WHERE id = ?1", [cam], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((local.as_str(), off), ("2019-08-03T10:10:00", 120));
        assert_eq!(offset_label(-240), "UTC−4");
        assert_eq!(offset_label(330), "UTC+5:30");
        assert_eq!(radius(42.0), "40 m");
        assert_eq!(radius(4_600.0), "5 km");
    }
}
