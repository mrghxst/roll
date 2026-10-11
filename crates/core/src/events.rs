//! Events: runs of Photos that belong together, found from time gaps and
//! distance from home (section 4.9).
//!
//! - Gaps split: more than 4 h at home, more than 12 h away.
//! - Away means more than 50 km from home; items without a location take
//!   the state of the nearest located item within 12 h.
//! - Consecutive away runs merge into one trip.
//! - At least 8 items; "saved on" dates (file times) don't count.
//! - Home is the setting, or else the most frequent night location.
//!
//! Proposed events are recomputed whenever the list is shown; confirmed
//! and dismissed ones stay, and their items are left out of detection.
//! Every owner action is one undoable batch: tags change through the edit
//! history, and `event_undo` keeps the event rows as they were.

use std::collections::{HashMap, HashSet};

use roll_geo::distance_m;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use crate::db::now_ms;
use crate::edits::{self, Change};

const HOUR: i64 = 3_600_000;
const AWAY_M: f64 = 50_000.0;
const MIN_ITEMS: usize = 8;
/// Trips continue across a day without photos, not across a week.
const TRIP_GAP: i64 = 72 * HOUR;

#[derive(Debug, Clone)]
pub struct Item {
    pub id: i64,
    pub utc: i64,
    pub local: String,
    pub pos: Option<(f64, f64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub ids: Vec<i64>,
    pub start_utc: i64,
    pub end_utc: i64,
    pub start_local: String,
    pub end_local: String,
    pub points: Vec<(f64, f64)>,
    pub away: bool,
}

/// The most frequent night location (20:00–06:00 local) on a ~10 km grid,
/// or the most frequent location at all.
pub fn home_of(items: &[Item]) -> Option<(f64, f64)> {
    let night = |i: &Item| i.local.get(11..13).and_then(|h| h.parse::<u32>().ok()).is_some_and(|h| !(6..20).contains(&h));
    let cell = |(la, lo): (f64, f64)| ((la * 10.0).round() as i64, (lo * 10.0).round() as i64);
    let pick = |filter: &dyn Fn(&Item) -> bool| {
        let mut counts: HashMap<(i64, i64), (usize, f64, f64)> = HashMap::new();
        for i in items.iter().filter(|i| filter(i)) {
            if let Some(p) = i.pos {
                let e = counts.entry(cell(p)).or_default();
                *e = (e.0 + 1, e.1 + p.0, e.2 + p.1);
            }
        }
        counts.into_values().max_by_key(|(n, _, _)| *n).map(|(n, la, lo)| (la / n as f64, lo / n as f64))
    };
    pick(&night).or_else(|| pick(&|_| true))
}

/// Events in `items` (any order).
pub fn detect(items: &[Item], home: Option<(f64, f64)>) -> Vec<Found> {
    let mut items: Vec<&Item> = items.iter().collect();
    items.sort_by_key(|i| (i.utc, i.id));
    let n = items.len();
    // Away state of located items, then borrowed by their neighbors.
    let own: Vec<Option<bool>> = items
        .iter()
        .map(|i| match (i.pos, home) {
            (Some(p), Some(h)) => Some(distance_m(p.0, p.1, h.0, h.1) > AWAY_M),
            _ => None,
        })
        .collect();
    let mut away = own.clone();
    let mut last: Option<usize> = None;
    let mut before: Vec<Option<usize>> = vec![None; n];
    for k in 0..n {
        before[k] = last;
        if own[k].is_some() {
            last = Some(k);
        }
    }
    let mut next: Option<usize> = None;
    for k in (0..n).rev() {
        if own[k].is_none() {
            let gap = |j: usize| (items[k].utc - items[j].utc).abs();
            let near = [before[k], next].into_iter().flatten().filter(|&j| gap(j) <= 12 * HOUR).min_by_key(|&j| gap(j));
            away[k] = near.and_then(|j| own[j]);
        } else {
            next = Some(k);
        }
    }
    let is_away = |k: usize| away[k] == Some(true);

    // Split by gaps.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    for k in 1..n {
        let limit = if is_away(k - 1) || is_away(k) { 12 * HOUR } else { 4 * HOUR };
        if items[k].utc - items[k - 1].utc > limit {
            runs.push((start, k));
            start = k;
        }
    }
    if n > 0 {
        runs.push((start, n));
    }
    // A run is away when most of its located items are.
    let run_away = |(a, b): (usize, usize)| {
        let located: Vec<bool> = (a..b).filter_map(|k| own[k].or(away[k])).collect();
        !located.is_empty() && located.iter().filter(|x| **x).count() * 2 > located.len()
    };
    // Days away merge into one trip.
    let mut merged: Vec<((usize, usize), bool)> = Vec::new();
    for r in runs {
        let a = run_away(r);
        match merged.last_mut() {
            Some((prev, true)) if a && items[r.0].utc - items[prev.1 - 1].utc <= TRIP_GAP => prev.1 = r.1,
            _ => merged.push((r, a)),
        }
    }
    merged
        .into_iter()
        .filter(|((a, b), _)| b - a >= MIN_ITEMS)
        .map(|((a, b), away)| {
            let run = &items[a..b];
            Found {
                ids: run.iter().map(|i| i.id).collect(),
                start_utc: run[0].utc,
                end_utc: run[run.len() - 1].utc,
                start_local: run[0].local.clone(),
                end_local: run[run.len() - 1].local.clone(),
                points: run.iter().filter_map(|i| i.pos).collect(),
                away,
            }
        })
        .collect()
}

const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// "August 2019 Rome", or "August 2019" without a place.
pub fn name(start_local: &str, place: Option<&str>) -> String {
    let y = start_local.get(..4).unwrap_or("");
    let m = start_local.get(5..7).and_then(|m| m.parse::<usize>().ok()).and_then(|m| MONTHS.get(m.wrapping_sub(1))).copied().unwrap_or("");
    let base = format!("{m} {y}").trim().to_string();
    match place {
        Some(p) if !p.is_empty() => format!("{base} {p}"),
        _ => base,
    }
}

/// The place an event's points share: the city if all are in one, else
/// the region, else the most common country.
pub fn common_place(points: &[(f64, f64)], gaz: &roll_geo::gazetteer::Gazetteer) -> Option<String> {
    let step = (points.len() / 60).max(1);
    let entries: Vec<&roll_geo::gazetteer::Entry> = points.iter().step_by(step).filter_map(|(la, lo)| gaz.reverse(*la, *lo)).collect();
    let first = entries.first()?;
    if entries.iter().all(|e| e.kind == roll_geo::gazetteer::Kind::City && e.name == first.name && e.cc == first.cc) {
        return Some(first.name.clone());
    }
    if !first.region.is_empty() && entries.iter().all(|e| e.region == first.region && e.cc == first.cc) {
        return Some(first.region.clone());
    }
    let mut by: HashMap<&str, usize> = HashMap::new();
    for e in &entries {
        *by.entry(e.country.as_str()).or_default() += 1;
    }
    by.into_iter().filter(|(c, _)| !c.is_empty()).max_by_key(|(c, n)| (*n, std::cmp::Reverse(*c))).map(|(c, _)| c.to_string())
}

// ---- database ----------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EventInfo {
    pub id: i32,
    pub name: String,
    /// "proposed" or "confirmed".
    pub state: String,
    pub start_local: String,
    pub end_local: String,
    pub count: i32,
    pub place: Option<String>,
    pub away: bool,
    pub own_folder: bool,
    /// Up to 8, spread over the event.
    pub samples: Vec<i32>,
}

/// Photos that can form events: primaries in Photos (or its subcategories)
/// with a capture date that is neither a file time nor suspicious (a
/// camera reset to 2000-01-01), not yet in a confirmed or dismissed event.
pub fn items(c: &Connection) -> rusqlite::Result<Vec<Item>> {
    let mut st = c.prepare(
        "SELECT a.id, a.taken_utc, a.taken_local, a.lat, a.lon FROM assets a JOIN categories cat ON cat.id = a.category_id
         WHERE a.missing = 0 AND a.trashed_at IS NULL AND (a.group_role IS NULL OR a.group_role = 'primary')
           AND a.taken_utc IS NOT NULL AND a.taken_local IS NOT NULL AND coalesce(a.date_source, '') != 'file_time'
           AND (a.problems & ?1) = 0
           AND (cat.key = 'photos' OR cat.parent_id = (SELECT id FROM categories WHERE key = 'photos'))
           AND NOT EXISTS (SELECT 1 FROM event_assets ea JOIN events e ON e.id = ea.event_id
                           WHERE ea.asset_id = a.id AND e.state IN ('confirmed', 'dismissed'))",
    )?;
    st.query_map([crate::model::problem::NO_DATE | crate::model::problem::SUSPICIOUS_DATE], |r| {
        let (lat, lon): (Option<f64>, Option<f64>) = (r.get(3)?, r.get(4)?);
        Ok(Item { id: r.get(0)?, utc: r.get(1)?, local: r.get(2)?, pos: lat.zip(lon) })
    })?
    .collect()
}

/// Names the place of a set of points.
pub type PlaceOf<'a> = dyn Fn(&[(f64, f64)]) -> Option<String> + 'a;

fn key_of(f: &Found) -> String {
    format!("{}|{}", f.start_local.get(..10).unwrap_or(""), f.end_local.get(..10).unwrap_or(""))
}

/// Replace the proposed events with what detection finds now. A proposal
/// the owner renamed keeps its name while its days stay the same.
pub fn refresh(c: &Connection, home: Option<(f64, f64)>, place: &PlaceOf) -> rusqlite::Result<usize> {
    let all = items(c)?;
    let home = home.or_else(|| home_of(&all));
    let found = detect(&all, home);
    let old: HashMap<String, (i64, String, bool)> = {
        let mut st = c.prepare("SELECT key, id, name, custom_name FROM events WHERE state = 'proposed' AND key IS NOT NULL")?;
        st.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0))))?.collect::<Result<_, _>>()?
    };
    let mut kept: HashSet<i64> = HashSet::new();
    for f in &found {
        let key = key_of(f);
        let center = (!f.points.is_empty()).then(|| {
            let n = f.points.len() as f64;
            (f.points.iter().map(|p| p.0).sum::<f64>() / n, f.points.iter().map(|p| p.1).sum::<f64>() / n)
        });
        let place_name = if f.points.is_empty() { None } else { place(&f.points) };
        let id = match old.get(&key) {
            Some((id, old_name, custom)) => {
                let name = if *custom { old_name.clone() } else { name(&f.start_local, place_name.as_deref()) };
                c.execute(
                    "UPDATE events SET name = ?2, start_utc = ?3, end_utc = ?4, lat = ?5, lon = ?6, place = ?7 WHERE id = ?1",
                    params![id, name, f.start_utc, f.end_utc, center.map(|c| c.0), center.map(|c| c.1), place_name],
                )?;
                c.execute("DELETE FROM event_assets WHERE event_id = ?1", [id])?;
                *id
            }
            None => {
                c.execute(
                    "INSERT INTO events (name, start_utc, end_utc, lat, lon, place, state, created_at, key) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'proposed', ?7, ?8)",
                    params![
                        name(&f.start_local, place_name.as_deref()),
                        f.start_utc,
                        f.end_utc,
                        center.map(|c| c.0),
                        center.map(|c| c.1),
                        place_name,
                        now_ms(),
                        key
                    ],
                )?;
                c.last_insert_rowid()
            }
        };
        let mut ins = c.prepare_cached("INSERT OR IGNORE INTO event_assets (event_id, asset_id) VALUES (?1, ?2)")?;
        for a in &f.ids {
            ins.execute(params![id, a])?;
        }
        kept.insert(id);
    }
    for (id, _, _) in old.values() {
        if !kept.contains(id) {
            c.execute("DELETE FROM events WHERE id = ?1", [id])?;
        }
    }
    Ok(found.len())
}

pub fn list(c: &Connection) -> rusqlite::Result<Vec<EventInfo>> {
    let mut st = c.prepare(
        "SELECT e.id, e.name, e.state, e.place, e.own_folder, e.lat, e.lon,
                (SELECT min(a.taken_local) FROM event_assets ea JOIN assets a ON a.id = ea.asset_id WHERE ea.event_id = e.id),
                (SELECT max(a.taken_local) FROM event_assets ea JOIN assets a ON a.id = ea.asset_id WHERE ea.event_id = e.id)
         FROM events e WHERE e.state IN ('proposed', 'confirmed') ORDER BY 8 DESC, e.id DESC",
    )?;
    type Row = (i64, String, String, Option<String>, bool, Option<f64>, Option<f64>, Option<String>, Option<String>);
    let rows: Vec<Row> = st
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, i64>(4)? != 0, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?))
        })?
        .collect::<Result<_, _>>()?;
    let home_far = |la: Option<f64>, lo: Option<f64>, home: Option<(f64, f64)>| {
        la.zip(lo).zip(home).is_some_and(|((la, lo), h)| distance_m(la, lo, h.0, h.1) > AWAY_M)
    };
    let home = home_of(&items(c)?);
    let mut out = Vec::new();
    for (id, name, state, place, own_folder, lat, lon, start, end) in rows {
        let ids = assets_of(c, id)?;
        if ids.is_empty() {
            continue;
        }
        let step = (ids.len() / 8).max(1);
        out.push(EventInfo {
            id: id as i32,
            name,
            state,
            start_local: start.unwrap_or_default(),
            end_local: end.unwrap_or_default(),
            count: ids.len() as i32,
            place,
            away: home_far(lat, lon, home),
            own_folder,
            samples: ids.iter().step_by(step).take(8).map(|i| *i as i32).collect(),
        });
    }
    Ok(out)
}

/// Items of an event in time order.
pub fn assets_of(c: &Connection, event: i64) -> rusqlite::Result<Vec<i64>> {
    let mut st = c.prepare_cached(
        "SELECT ea.asset_id FROM event_assets ea JOIN assets a ON a.id = ea.asset_id
         WHERE ea.event_id = ?1 AND a.missing = 0 AND a.trashed_at IS NULL ORDER BY a.taken_utc, a.id",
    )?;
    st.query_map([event], |r| r.get(0))?.collect()
}

/// Keep the event row and items as they are before `batch` changes them.
fn snapshot(c: &Connection, batch: i64, event: i64) -> rusqlite::Result<()> {
    let row: Option<Value> = c
        .query_row(
            "SELECT name, start_utc, end_utc, lat, lon, place, state, own_folder, immich, immich_album_id, created_at, key, custom_name FROM events WHERE id = ?1",
            [event],
            |r| {
                Ok(json!({
                    "name": r.get::<_, String>(0)?, "start_utc": r.get::<_, Option<i64>>(1)?, "end_utc": r.get::<_, Option<i64>>(2)?,
                    "lat": r.get::<_, Option<f64>>(3)?, "lon": r.get::<_, Option<f64>>(4)?, "place": r.get::<_, Option<String>>(5)?,
                    "state": r.get::<_, String>(6)?, "own_folder": r.get::<_, i64>(7)?, "immich": r.get::<_, String>(8)?,
                    "immich_album_id": r.get::<_, Option<String>>(9)?, "created_at": r.get::<_, i64>(10)?,
                    "key": r.get::<_, Option<String>>(11)?, "custom_name": r.get::<_, i64>(12)?,
                }))
            },
        )
        .optional()?;
    let assets: Vec<i64> = {
        let mut st = c.prepare_cached("SELECT asset_id FROM event_assets WHERE event_id = ?1")?;
        st.query_map([event], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    c.execute(
        "INSERT OR IGNORE INTO event_undo (batch_id, event_id, row, assets) VALUES (?1, ?2, ?3, ?4)",
        params![batch, event, row.map(|r| r.to_string()), json!(assets).to_string()],
    )?;
    Ok(())
}

/// Put events back as they were before `batch` (called by undo).
pub fn undo(c: &Connection, batch: i64) -> rusqlite::Result<()> {
    let rows: Vec<(i64, Option<String>, String)> = {
        let mut st = c.prepare("SELECT event_id, row, assets FROM event_undo WHERE batch_id = ?1")?;
        st.query_map([batch], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?
    };
    for (id, row, assets) in rows {
        c.execute("DELETE FROM events WHERE id = ?1", [id])?;
        let Some(row) = row.and_then(|r| serde_json::from_str::<Value>(&r).ok()) else { continue };
        let s = |k: &str| row.get(k).and_then(Value::as_str).map(str::to_string);
        let i = |k: &str| row.get(k).and_then(Value::as_i64);
        let f = |k: &str| row.get(k).and_then(Value::as_f64);
        c.execute(
            "INSERT INTO events (id, name, start_utc, end_utc, lat, lon, place, state, own_folder, immich, immich_album_id, created_at, key, custom_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                id,
                s("name"),
                i("start_utc"),
                i("end_utc"),
                f("lat"),
                f("lon"),
                s("place"),
                s("state"),
                i("own_folder").unwrap_or(1),
                s("immich").unwrap_or_else(|| "default".into()),
                s("immich_album_id"),
                i("created_at").unwrap_or(0),
                s("key"),
                i("custom_name").unwrap_or(0)
            ],
        )?;
        let ids: Vec<i64> = serde_json::from_str(&assets).unwrap_or_default();
        for a in ids {
            c.execute("INSERT OR IGNORE INTO event_assets (event_id, asset_id) VALUES (?1, ?2)", params![id, a])?;
        }
    }
    c.execute("DELETE FROM event_undo WHERE batch_id = ?1", [batch])?;
    Ok(())
}

fn tag_of(name: &str) -> String {
    format!("Event/{}", name.replace('/', "-"))
}

/// Change the event tag of `ids` (and their group members) in `batch`.
fn retag(c: &Connection, batch: i64, ids: &[i64], remove: Option<&str>, add: Option<&str>) -> rusqlite::Result<usize> {
    let (remove, add) = (remove.map(tag_of), add.map(tag_of));
    let mut changed = 0;
    for id in crate::trash::with_groups(c, ids)? {
        let mut list: Vec<Value> = edits::read(c, id, "tags")?.as_array().cloned().unwrap_or_default();
        let same = |t: &Value, p: &str| t.get("p").and_then(Value::as_str).is_some_and(|x| x.eq_ignore_ascii_case(p));
        if let Some(r) = &remove {
            list.retain(|t| !same(t, r));
        }
        if let Some(a) = &add
            && !list.iter().any(|t| same(t, a))
        {
            list.push(json!({"p": a, "s": "event"}));
        }
        let ch = Change { field: "tags", value: Value::Array(list), source: "manual", source_ref: None, conf: None, why: None };
        if edits::set(c, Some(batch), id, &ch)? {
            changed += 1;
        }
    }
    Ok(changed)
}

fn info(c: &Connection, id: i64) -> rusqlite::Result<(String, String)> {
    c.query_row("SELECT name, state FROM events WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
}

/// Confirm (optionally renaming): the items get the tag `Event/<name>`.
pub fn confirm(c: &Connection, id: i64, new_name: Option<&str>) -> rusqlite::Result<i64> {
    let (old, _) = info(c, id)?;
    let name = new_name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&old).to_string();
    let batch = edits::begin_batch(c, "event", &format!("Confirm event {name}"))?;
    snapshot(c, batch, id)?;
    retag(c, batch, &assets_of(c, id)?, None, Some(&name))?;
    c.execute("UPDATE events SET state = 'confirmed', name = ?2, custom_name = 1 WHERE id = ?1", params![id, name])?;
    Ok(batch)
}

pub fn rename(c: &Connection, id: i64, new_name: &str) -> rusqlite::Result<i64> {
    let (old, state) = info(c, id)?;
    let name = new_name.trim();
    let batch = edits::begin_batch(c, "event", &format!("Rename event {old} to {name}"))?;
    snapshot(c, batch, id)?;
    if state == "confirmed" {
        retag(c, batch, &assets_of(c, id)?, Some(&old), Some(name))?;
    }
    c.execute("UPDATE events SET name = ?2, custom_name = 1 WHERE id = ?1", params![id, name])?;
    Ok(batch)
}

pub fn dismiss(c: &Connection, id: i64) -> rusqlite::Result<i64> {
    let (name, state) = info(c, id)?;
    let batch = edits::begin_batch(c, "event", &format!("Dismiss event {name}"))?;
    snapshot(c, batch, id)?;
    if state == "confirmed" {
        retag(c, batch, &assets_of(c, id)?, Some(&name), None)?;
    }
    c.execute("UPDATE events SET state = 'dismissed' WHERE id = ?1", [id])?;
    Ok(batch)
}

/// Merge `ids` into the earliest one. The result is confirmed when any of
/// them was, and keeps that event's name.
pub fn merge(c: &Connection, ids: &[i64]) -> rusqlite::Result<i64> {
    let mut evs: Vec<(i64, String, String, i64)> = Vec::new();
    for &id in ids {
        let (name, state) = info(c, id)?;
        let start: i64 = c.query_row("SELECT coalesce(start_utc, 0) FROM events WHERE id = ?1", [id], |r| r.get(0))?;
        evs.push((id, name, state, start));
    }
    evs.sort_by_key(|e| e.3);
    let Some(target) = evs.first().cloned() else { return edits::begin_batch(c, "event", "Merge events") };
    let confirmed = evs.iter().find(|e| e.2 == "confirmed");
    let name = confirmed.map(|e| e.1.clone()).unwrap_or_else(|| target.1.clone());
    let batch = edits::begin_batch(c, "event", &format!("Merge {} events into {name}", evs.len()))?;
    for e in &evs {
        snapshot(c, batch, e.0)?;
    }
    for e in &evs[1..] {
        let items = assets_of(c, e.0)?;
        if e.2 == "confirmed" && e.1 != name {
            retag(c, batch, &items, Some(&e.1), None)?;
        }
        c.execute("UPDATE OR IGNORE event_assets SET event_id = ?2 WHERE event_id = ?1", params![e.0, target.0])?;
        c.execute("DELETE FROM events WHERE id = ?1", [e.0])?;
    }
    if target.2 == "confirmed" && target.1 != name {
        retag(c, batch, &assets_of(c, target.0)?, Some(&target.1), None)?;
    }
    let state = if confirmed.is_some() { "confirmed" } else { "proposed" };
    if confirmed.is_some() {
        retag(c, batch, &assets_of(c, target.0)?, None, Some(&name))?;
    }
    let end: Option<i64> = c.query_row(
        "SELECT max(a.taken_utc) FROM event_assets ea JOIN assets a ON a.id = ea.asset_id WHERE ea.event_id = ?1",
        [target.0],
        |r| r.get(0),
    )?;
    c.execute(
        "UPDATE events SET name = ?2, state = ?3, end_utc = ?4, custom_name = 1, key = NULL WHERE id = ?1",
        params![target.0, name, state, end],
    )?;
    Ok(batch)
}

/// Split an event before `at` (an item of it): `at` and later items form a
/// new event with the same state and the name `new_name`.
pub fn split(c: &Connection, id: i64, at: i64, new_name: &str) -> rusqlite::Result<Option<i64>> {
    let items = assets_of(c, id)?;
    let Some(pos) = items.iter().position(|a| *a == at).filter(|p| *p > 0) else { return Ok(None) };
    let (name, state) = info(c, id)?;
    let batch = edits::begin_batch(c, "event", &format!("Split event {name}"))?;
    snapshot(c, batch, id)?;
    let (own_folder, immich): (i64, String) =
        c.query_row("SELECT own_folder, immich FROM events WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    c.execute(
        "INSERT INTO events (name, state, own_folder, immich, created_at, custom_name) VALUES (?1, ?2, ?3, ?4, ?5, 1)",
        params![new_name.trim(), state, own_folder, immich, now_ms()],
    )?;
    let new = c.last_insert_rowid();
    c.execute("INSERT INTO event_undo (batch_id, event_id, row, assets) VALUES (?1, ?2, NULL, '[]')", params![batch, new])?;
    let later = &items[pos..];
    for a in later {
        c.execute("UPDATE event_assets SET event_id = ?2 WHERE event_id = ?1 AND asset_id = ?3", params![id, new, a])?;
    }
    for (ev, ids) in [(id, &items[..pos]), (new, later)] {
        let (s, e): (Option<i64>, Option<i64>) = c.query_row(
            &format!(
                "SELECT min(taken_utc), max(taken_utc) FROM assets WHERE id IN ({})",
                ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
            ),
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        c.execute("UPDATE events SET start_utc = ?2, end_utc = ?3, key = NULL, custom_name = 1 WHERE id = ?1", params![ev, s, e])?;
    }
    if state == "confirmed" {
        retag(c, batch, later, Some(&name), Some(new_name.trim()))?;
    }
    Ok(Some(batch))
}

pub fn set_own_folder(c: &Connection, id: i64, on: bool) -> rusqlite::Result<()> {
    c.execute("UPDATE events SET own_folder = ?2 WHERE id = ?1", params![id, on])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZURICH: (f64, f64) = (47.37, 8.54);
    const ROME: (f64, f64) = (41.89, 12.49);

    fn at(id: i64, day: u32, hour: u32, min: u32, pos: Option<(f64, f64)>) -> Item {
        // 2019-08-01 00:00 UTC is 1564617600000; local = UTC + 2 h.
        let utc = 1_564_617_600_000 + (((day as i64 - 1) * 24 + hour as i64) * 60 + min as i64) * 60_000;
        let l = chrono::DateTime::from_timestamp_millis(utc + 2 * HOUR).unwrap().naive_utc();
        Item { id, utc, local: l.format("%Y-%m-%dT%H:%M:%S").to_string(), pos }
    }

    fn library() -> Vec<Item> {
        let mut v = Vec::new();
        let mut id = 0;
        let mut add = |day, hour, min, pos| {
            id += 1;
            v.push(at(id, day, hour, min, pos));
        };
        // Evenings at home over the month: home is Zürich.
        for d in [1, 3, 5, 20, 25, 28] {
            add(d, 19, 0, Some(ZURICH));
        }
        // A birthday at home: 10 photos in 3 hours.
        for k in 0..10 {
            add(2, 12, k * 18, Some(ZURICH));
        }
        // Rome, days 10–13, with nights in between and some photos without
        // GPS from a second camera.
        for d in 10..=13 {
            for k in 0..6 {
                add(d, 8 + k * 2, 0, Some(ROME));
            }
            add(d, 9, 30, None);
        }
        // An outing with only 7 photos.
        for k in 0..7 {
            add(22, 10, k * 10, Some(ZURICH));
        }
        v
    }

    #[test]
    fn finds_home_trips_and_parties() {
        let items = library();
        let home = home_of(&items).unwrap();
        assert!(distance_m(home.0, home.1, ZURICH.0, ZURICH.1) < 1000.0);
        let found = detect(&items, Some(home));
        assert_eq!(found.len(), 2, "{found:#?}");
        let party = &found[0];
        assert_eq!((party.ids.len(), party.away), (10, false));
        let trip = &found[1];
        assert!(trip.away);
        assert_eq!(trip.ids.len(), 28, "four days with the camera without GPS merge into one trip");
        assert_eq!(&trip.start_local[..10], "2019-08-10");
        assert_eq!(&trip.end_local[..10], "2019-08-13");
    }

    #[test]
    fn a_week_apart_is_two_trips() {
        let mut items = Vec::new();
        for k in 0..10 {
            items.push(at(k, 3, 9 + k as u32, 0, Some(ROME)));
            items.push(at(100 + k, 12, 9 + k as u32, 0, Some(ROME)));
        }
        assert_eq!(detect(&items, Some(ZURICH)).len(), 2);
        // At home, a 5 h gap splits; away, it doesn't.
        let gap = |pos| (0..16).map(|k| at(k, 5, if k < 8 { 8 + k as u32 / 4 } else { 15 + k as u32 / 4 }, 0, pos)).collect::<Vec<_>>();
        assert_eq!(detect(&gap(Some(ZURICH)), Some(ZURICH)).len(), 2);
        assert_eq!(detect(&gap(Some(ROME)), Some(ZURICH)).len(), 1);
    }

    #[test]
    fn names() {
        assert_eq!(name("2019-08-10T09:00:00", Some("Rome")), "August 2019 Rome");
        assert_eq!(name("2021-06-01T09:00:00", None), "June 2021");
    }

    #[test]
    fn actions_are_undoable() {
        let db = crate::db::Db::memory().unwrap();
        let c = db.write();
        let photos: i64 = c.query_row("SELECT id FROM categories WHERE key = 'photos'", [], |r| r.get(0)).unwrap();
        for it in library() {
            let id = crate::testutil::insert_asset(&c, &format!("p/{}.jpg", it.id));
            c.execute(
                "UPDATE assets SET taken_utc = ?2, taken_local = ?3, lat = ?4, lon = ?5, category_id = ?6, review = 2, date_source = 'exif_original' WHERE id = ?1",
                params![id, it.utc, it.local, it.pos.map(|p| p.0), it.pos.map(|p| p.1), photos],
            )
            .unwrap();
        }
        let place = |pts: &[(f64, f64)]| {
            Some(if distance_m(pts[0].0, pts[0].1, ROME.0, ROME.1) < 50_000.0 { "Rome" } else { "Zürich" }.to_string())
        };
        assert_eq!(refresh(&c, None, &place).unwrap(), 2);
        let evs = list(&c).unwrap();
        assert_eq!(evs.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["August 2019 Rome", "August 2019 Zürich"]);
        assert!(evs[0].away && !evs[1].away);
        let rome = evs[0].id as i64;

        // Refreshing again keeps ids; a renamed proposal keeps its name.
        rename(&c, rome, "Rome with Anna").unwrap();
        refresh(&c, None, &place).unwrap();
        assert_eq!(list(&c).unwrap()[0].name, "Rome with Anna");
        assert_eq!(list(&c).unwrap()[0].id as i64, rome);

        let batch = confirm(&c, rome, None).unwrap();
        let first = assets_of(&c, rome).unwrap()[0];
        assert!(crate::tags::of_asset(&c, first).unwrap().contains(&"Event/Rome with Anna".to_string()));
        // Confirmed events are not proposed again.
        refresh(&c, None, &place).unwrap();
        assert_eq!(list(&c).unwrap().iter().filter(|e| e.state == "confirmed").count(), 1);
        assert_eq!(list(&c).unwrap().len(), 2);

        // Split the confirmed trip on day 12: the later part gets its own tag.
        let items = assets_of(&c, rome).unwrap();
        let day12 = items[14];
        let sb = split(&c, rome, day12, "Rome, last days").unwrap().unwrap();
        assert_eq!(assets_of(&c, rome).unwrap().len(), 14);
        assert!(crate::tags::of_asset(&c, day12).unwrap().contains(&"Event/Rome, last days".to_string()));
        assert!(!crate::tags::of_asset(&c, day12).unwrap().contains(&"Event/Rome with Anna".to_string()));
        let later = list(&c).unwrap().into_iter().find(|e| e.name == "Rome, last days").unwrap().id as i64;

        // Merge back, then undo merge, split and confirm in turn.
        let mb = merge(&c, &[later, rome]).unwrap();
        assert_eq!(assets_of(&c, rome).unwrap().len(), 28);
        assert!(crate::tags::of_asset(&c, day12).unwrap().contains(&"Event/Rome with Anna".to_string()));
        for b in [mb, sb, batch] {
            crate::edits::undo_batch(&c, b).unwrap();
            undo(&c, b).unwrap();
        }
        assert_eq!(assets_of(&c, rome).unwrap().len(), 28);
        assert_eq!(info(&c, rome).unwrap(), ("Rome with Anna".to_string(), "proposed".to_string()));
        assert!(crate::tags::of_asset(&c, day12).unwrap().iter().all(|t| !t.starts_with("Event/")));
        assert!(info(&c, later).is_err(), "the split-off event is gone again");

        dismiss(&c, rome).unwrap();
        refresh(&c, None, &place).unwrap();
        assert_eq!(list(&c).unwrap().len(), 1, "dismissed trips stay dismissed");
    }
}
