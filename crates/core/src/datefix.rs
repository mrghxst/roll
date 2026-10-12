//! Fix dates: find suspect sets (camera clocks, time zones, missing dates),
//! preview a fix with its side effects, and apply it as one undoable batch.

use std::collections::HashMap;
use std::sync::LazyLock;

use chrono::{Datelike, Duration, Months, NaiveDateTime};
use regex::Regex;
use roll_meta::filename::{from_file_name, from_folder_name};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use crate::dates::TzGuess;
use crate::edits::{self, Change};
use crate::model::{Scope, problem};

const FMT: &str = "%Y-%m-%dT%H:%M:%S";

fn parse(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, FMT).ok()
}

fn fmt(d: NaiveDateTime) -> String {
    d.format(FMT).to_string()
}

const PHONE_MAKES: &[&str] = &[
    "apple",
    "google",
    "samsung",
    "huawei",
    "xiaomi",
    "oneplus",
    "oppo",
    "vivo",
    "motorola",
    "nokia",
    "honor",
    "realme",
    "nothing",
    "fairphone",
    "lg",
];

pub fn is_phone(make: Option<&str>, model: Option<&str>) -> bool {
    let make = make.unwrap_or("").to_lowercase();
    let model = model.unwrap_or("").to_lowercase();
    PHONE_MAKES.iter().any(|m| make.starts_with(m)) || model.contains("iphone") || model.contains("pixel")
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DateSet {
    pub key: String,
    /// "camera" or "missing".
    pub kind: String,
    pub label: String,
    pub count: i32,
    pub from_local: String,
    pub to_local: String,
    /// Why Roll suspects the set; empty when nothing looks wrong.
    pub reasons: Vec<String>,
    /// Whether the files state a UTC offset.
    pub has_offset: bool,
    pub ids: Vec<i32>,
}

struct Row {
    id: i64,
    camera: String,
    phone: bool,
    local: NaiveDateTime,
    utc: i64,
    offset: Option<i32>,
    problems: i64,
    dir: String,
}

static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[^0-9])((?:19|20)\d{2})(?:[^0-9]|$)").unwrap());

fn month_year(d: &NaiveDateTime) -> String {
    d.format("%-d %b %Y").to_string()
}

/// Camera sets (by make, model and serial, split where the camera was not
/// used for a week) and the set of items without a trusted date.
pub fn sets(c: &Connection, now_ms: i64) -> rusqlite::Result<Vec<DateSet>> {
    sets_in(c, now_ms, Scope::Archive)
}

/// [`sets`] for the archive or one import's items. Phone time zones always
/// come from the archive.
pub fn sets_in(c: &Connection, now_ms: i64, scope: Scope) -> rusqlite::Result<Vec<DateSet>> {
    let rows: Vec<Row> = {
        let mut st = c.prepare(&format!(
            "SELECT id, coalesce(make, ''), coalesce(model, ''), coalesce(serial, ''), taken_local, taken_utc, tz_offset, problems, dir
             FROM assets
             WHERE (make IS NOT NULL OR model IS NOT NULL) AND taken_local IS NOT NULL AND {} AND trashed_at IS NULL
               AND (group_role IS NULL OR group_role = 'primary')
             ORDER BY make, model, serial, taken_local",
            scope.sql("")
        ))?;
        st.query_map([], |r| {
            let make: String = r.get(1)?;
            let model: String = r.get(2)?;
            let serial: String = r.get(3)?;
            let local: String = r.get(4)?;
            Ok((
                r.get::<_, i64>(0)?,
                make,
                model,
                serial,
                local,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, String>(8)?,
            ))
        })?
        .filter_map(|row| {
            let (id, make, model, serial, local, utc, offset, problems, dir) = row.ok()?;
            // "NIKON CORPORATION" + "NIKON D750" reads as "NIKON D750".
            let brand = make.split_whitespace().next().unwrap_or("").to_lowercase();
            let label = if make.is_empty() || (!brand.is_empty() && model.to_lowercase().starts_with(&brand)) {
                model.clone()
            } else {
                format!("{make} {model}")
            };
            let camera = if serial.is_empty() { label } else { format!("{label} ({serial})") };
            Some(Row {
                id,
                phone: is_phone(Some(&make), Some(&model)),
                camera,
                local: parse(&local)?,
                utc,
                offset: offset.map(|o| o as i32),
                problems,
                dir,
            })
        })
        .collect()
    };
    // Phone photos with a stated offset: reference zones by day.
    let phone_zones: Vec<(NaiveDateTime, i32)> = {
        let mut st = c.prepare(
            "SELECT make, model, taken_local, tz_offset FROM assets
             WHERE tz_offset IS NOT NULL AND taken_local IS NOT NULL AND missing = 0 AND trashed_at IS NULL",
        )?;
        st.query_map([], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?))
        })?
        .filter_map(Result::ok)
        .filter(|(make, model, _, _)| is_phone(make.as_deref(), model.as_deref()))
        .filter_map(|(_, _, l, o)| Some((parse(&l)?, o as i32)))
        .collect()
    };

    let mut runs: Vec<Vec<&Row>> = Vec::new();
    for r in &rows {
        match runs.last_mut() {
            Some(run) if run[0].camera == r.camera && r.local - run.last().unwrap().local < Duration::days(7) => run.push(r),
            _ => runs.push(vec![r]),
        }
    }
    let mut out = Vec::new();
    for run in runs {
        let first = run[0];
        let last = run[run.len() - 1];
        let mut reasons = Vec::new();
        if run.iter().any(|r| r.problems & problem::SUSPICIOUS_DATE != 0) {
            let future = first.local.and_utc().timestamp_millis() > now_ms;
            reasons.push(if future {
                format!("The photos are dated in the future ({}).", month_year(&first.local))
            } else {
                format!("The camera's clock looks reset: the photos start on {}.", month_year(&first.local))
            });
        }
        // Folder names that state another year.
        let mut folder_years: HashMap<i32, usize> = HashMap::new();
        for r in &run {
            if let Some(y) = YEAR.captures(&r.dir).and_then(|c| c[1].parse::<i32>().ok()) {
                *folder_years.entry(y).or_default() += 1;
            }
        }
        if let Some((&fy, &n)) = folder_years.iter().max_by_key(|(_, n)| **n)
            && n * 2 > run.len()
            && run.iter().filter(|r| r.local.year() == fy).count() * 2 < run.len()
        {
            reasons.push(format!("The folder says {fy}, but the photos are dated {}.", first.local.year()));
        }
        // No stated zone, while phones on the same days state another one.
        let has_offset = run.iter().any(|r| r.offset.is_some());
        if !has_offset && !first.phone {
            let guessed = ((first.local.and_utc().timestamp_millis() - first.utc) / 60_000) as i32;
            let (lo, hi) = (first.local - Duration::days(1), last.local + Duration::days(1));
            let mut zones: HashMap<i32, usize> = HashMap::new();
            for (l, o) in &phone_zones {
                if *l >= lo && *l <= hi {
                    *zones.entry(*o).or_default() += 1;
                }
            }
            if let Some((&o, _)) = zones.iter().max_by_key(|(_, n)| **n)
                && o != guessed
            {
                reasons.push(format!(
                    "Phone photos from the same days are in {}; this camera states no time zone and is read as {}.",
                    crate::places::offset_label(o),
                    crate::places::offset_label(guessed)
                ));
            }
        }
        // Phones keep good time; only list them when something is wrong.
        if first.phone && reasons.is_empty() {
            continue;
        }
        out.push(DateSet {
            key: format!("camera:{}:{}", first.camera, first.local.format("%Y-%m-%d")),
            kind: "camera".into(),
            label: first.camera.clone(),
            count: run.len() as i32,
            from_local: fmt(first.local),
            to_local: fmt(last.local),
            reasons,
            has_offset,
            ids: run.iter().map(|r| r.id as i32).collect(),
        });
    }
    // Items without a trusted date.
    let missing: Vec<(i64, Option<String>)> = {
        let mut st = c.prepare(&format!(
            "SELECT id, taken_local FROM assets
             WHERE (problems & ?1 != 0 OR taken_local IS NULL OR date_source IN ('file_time', 'folder'))
               AND {} AND trashed_at IS NULL AND (group_role IS NULL OR group_role = 'primary')
             ORDER BY taken_local",
            scope.sql("")
        ))?;
        st.query_map([problem::NO_DATE], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    if !missing.is_empty() {
        let dated: Vec<&String> = missing.iter().filter_map(|m| m.1.as_ref()).collect();
        out.push(DateSet {
            key: "missing".into(),
            kind: "missing".into(),
            label: "Without a trusted date".into(),
            count: missing.len() as i32,
            from_local: dated.first().map(|s| s.to_string()).unwrap_or_default(),
            to_local: dated.last().map(|s| s.to_string()).unwrap_or_default(),
            reasons: vec!["The files state no date taken; Roll guessed from names, folders or file times.".into()],
            has_offset: false,
            ids: missing.iter().map(|m| m.0 as i32).collect(),
        });
    }
    let dismissed: std::collections::HashSet<String> = {
        let mut st = c.prepare("SELECT key FROM dismissed WHERE kind = 'date'")?;
        st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    out.retain(|s| !dismissed.contains(&s.key));
    out.sort_by(|a, b| b.reasons.is_empty().cmp(&a.reasons.is_empty()).reverse().then(b.count.cmp(&a.count)));
    Ok(out)
}

/// A known true date for one item.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DateRef {
    #[ts(type = "number")]
    pub asset_id: i64,
    /// "YYYY-MM-DDTHH:MM:SS", wall-clock time where the photo was taken.
    pub local: String,
    pub offset_min: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Amount {
    pub years: i32,
    pub days: i32,
    pub hours: i32,
    pub minutes: i32,
    pub seconds: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DateFix {
    #[ts(type = "Array<number>")]
    pub ids: Vec<i64>,
    /// "shift", "zone", "drift", "amount", "exact" or "suggested".
    pub mode: String,
    pub reference: Option<DateRef>,
    /// The second reference, for drift.
    pub second: Option<DateRef>,
    /// "zone": the offset to state, keeping wall-clock times.
    pub offset_min: Option<i32>,
    pub amount: Option<Amount>,
    /// "exact": the date for every item (asset_id is ignored).
    pub exact: Option<DateRef>,
    /// Also fix every photo from the same camera between these local
    /// times (cameras keep a wrong clock for months).
    pub camera_from: Option<String>,
    pub camera_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DateSample {
    pub id: i32,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatePreview {
    pub count: i32,
    /// "−1 y 2 h", "time zone UTC+2", "drift corrected".
    pub label: String,
    pub before_from: String,
    pub before_to: String,
    pub after_from: String,
    pub after_to: String,
    /// Items whose year changes.
    pub year_changes: i32,
    /// Sorted items whose `{year}` folder changes when organized.
    pub folder_changes: i32,
    pub samples: Vec<DateSample>,
}

struct Cur {
    id: i64,
    local: Option<NaiveDateTime>,
    utc: Option<i64>,
    offset: Option<i32>,
    source: Option<String>,
    camera: String,
    decided: bool,
}

fn load(c: &Connection, id: i64) -> rusqlite::Result<Cur> {
    c.query_row(
        "SELECT id, taken_local, taken_utc, tz_offset, date_source, coalesce(make, '') || '|' || coalesce(model, '') || '|' || coalesce(serial, ''),
                review >= 2 AND category_id IS NOT NULL
         FROM assets WHERE id = ?1",
        [id],
        |r| {
            Ok(Cur {
                id: r.get(0)?,
                local: r.get::<_, Option<String>>(1)?.as_deref().and_then(parse),
                utc: r.get(2)?,
                offset: r.get::<_, Option<i64>>(3)?.map(|o| o as i32),
                source: r.get(4)?,
                camera: r.get(5)?,
                decided: r.get(6)?,
            })
        },
    )
}

/// Calendar difference, as the owner would say it: "−1 y 2 h".
pub fn diff_label(from: NaiveDateTime, to: NaiveDateTime) -> String {
    if from == to {
        return "no change".into();
    }
    let (sign, a, b) = if to >= from { ('+', from, to) } else { ('−', to, from) };
    let mut months = (b.year() - a.year()) * 12 + b.month() as i32 - a.month() as i32;
    let mut mid = a.checked_add_months(Months::new(months.max(0) as u32)).unwrap_or(a);
    if mid > b {
        months -= 1;
        mid = a.checked_add_months(Months::new(months.max(0) as u32)).unwrap_or(a);
    }
    let rest = b - mid;
    let mut parts = Vec::new();
    let (y, m) = (months / 12, months % 12);
    if y > 0 {
        parts.push(format!("{y} y"));
    }
    if m > 0 {
        parts.push(format!("{m} mo"));
    }
    let secs = rest.num_seconds();
    let (d, h, mi, s) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60, secs % 60);
    for (v, u) in [(d, "d"), (h, "h"), (mi, "min"), (s, "s")] {
        if v > 0 {
            parts.push(format!("{v} {u}"));
        }
    }
    format!("{sign}{}", parts.join(" "))
}

fn true_of(c: &Connection, r: &DateRef) -> anyhow::Result<(Cur, NaiveDateTime)> {
    let cur = load(c, r.asset_id)?;
    let local = parse(&r.local).ok_or_else(|| anyhow::anyhow!("Enter the date as year, month, day and time."))?;
    if cur.local.is_none() {
        anyhow::bail!("The reference photo has no date to compare with.");
    }
    Ok((cur, local))
}

/// (id, old value, new value) of one changed date.
pub type DateChange = (i64, Value, Value);
type NewDate = Box<dyn Fn(&Cur) -> Option<(NaiveDateTime, Option<i32>)>>;

/// The new date of every item, and a label for the change.
pub fn compute(c: &Connection, fix: &DateFix, tz: &dyn TzGuess) -> anyhow::Result<(Vec<DateChange>, String)> {
    let mut ids = fix.ids.clone();
    if let (Some(from), Some(to), Some(r)) = (&fix.camera_from, &fix.camera_to, fix.reference.as_ref()) {
        let camera = load(c, r.asset_id)?.camera;
        let mut st = c.prepare(
            "SELECT id FROM assets WHERE coalesce(make, '') || '|' || coalesce(model, '') || '|' || coalesce(serial, '') = ?1
               AND taken_local BETWEEN ?2 AND ?3 AND missing = 0 AND trashed_at IS NULL",
        )?;
        ids.extend(st.query_map(params![camera, from, to], |r| r.get::<_, i64>(0))?.filter_map(Result::ok));
    }
    let ids = crate::trash::with_groups(c, &ids)?;
    let items: Vec<Cur> = ids.iter().map(|id| load(c, *id)).collect::<Result<_, _>>()?;

    // Per item: Some((new local, new offset)).
    let mut label = String::new();
    let new_of: NewDate = match fix.mode.as_str() {
        "shift" => {
            let r = fix.reference.as_ref().ok_or_else(|| anyhow::anyhow!("Pick a reference photo first."))?;
            let (cur, truth) = true_of(c, r)?;
            let delta = truth - cur.local.unwrap();
            label = diff_label(cur.local.unwrap(), truth);
            if let Some(o) = r.offset_min.filter(|o| Some(*o) != cur.offset) {
                label = format!("{label}, {}", crate::places::offset_label(o));
            }
            let offset = r.offset_min;
            Box::new(move |it: &Cur| it.local.map(|l| (l + delta, offset.or(it.offset))))
        }
        "drift" => {
            let (r1, r2) = (fix.reference.as_ref(), fix.second.as_ref());
            let (Some(r1), Some(r2)) = (r1, r2) else { anyhow::bail!("Drift needs two reference photos.") };
            let (c1, t1) = true_of(c, r1)?;
            let (c2, t2) = true_of(c, r2)?;
            let (l1, l2) = (c1.local.unwrap(), c2.local.unwrap());
            if l1 == l2 {
                anyhow::bail!("Pick two reference photos taken at different times.");
            }
            let (d1, d2) = ((t1 - l1).num_milliseconds() as f64, (t2 - l2).num_milliseconds() as f64);
            let span = (l2 - l1).num_milliseconds() as f64;
            label = format!("{} to {}, corrected for drift", diff_label(l1, t1), diff_label(l2, t2));
            let offset = r1.offset_min.or(r2.offset_min);
            Box::new(move |it: &Cur| {
                it.local.map(|l| {
                    let f = (l - l1).num_milliseconds() as f64 / span;
                    (l + Duration::milliseconds((d1 + (d2 - d1) * f).round() as i64), offset.or(it.offset))
                })
            })
        }
        "zone" => {
            let o = fix.offset_min.ok_or_else(|| anyhow::anyhow!("Pick a time zone."))?;
            label = format!("time zone {}", crate::places::offset_label(o));
            Box::new(move |it: &Cur| it.local.map(|l| (l, Some(o))))
        }
        "amount" | "smart" => {
            let a = fix.amount.clone().unwrap_or_default();
            let zone = if fix.mode == "smart" { fix.offset_min } else { None };
            let dur = Duration::days(a.days as i64)
                + Duration::hours(a.hours as i64)
                + Duration::minutes(a.minutes as i64)
                + Duration::seconds(a.seconds as i64);
            let years = a.years;
            let shift = move |l: NaiveDateTime| {
                let l = if years >= 0 {
                    l.checked_add_months(Months::new(years as u32 * 12))
                } else {
                    l.checked_sub_months(Months::new((-years) as u32 * 12))
                };
                l.map(|l| l + dur)
            };
            if let Some(it) = items.iter().find(|it| it.local.is_some()) {
                let l = it.local.unwrap();
                label = diff_label(l, shift(l).unwrap_or(l));
            }
            if let Some(o) = zone {
                label = format!("{label}, {}", crate::places::offset_label(o));
            }
            Box::new(move |it: &Cur| it.local.and_then(shift).map(|l| (l, zone.or(it.offset))))
        }
        "exact" => {
            let e = fix.exact.as_ref().ok_or_else(|| anyhow::anyhow!("Enter a date."))?;
            let l = parse(&e.local).ok_or_else(|| anyhow::anyhow!("Enter the date as year, month, day and time."))?;
            label = format!("set to {}", l.format("%-d %b %Y %H:%M"));
            let o = e.offset_min;
            Box::new(move |_: &Cur| Some((l, o)))
        }
        "suggested" => {
            let sugg: HashMap<i64, Candidate> = candidates(c, &ids)?.into_iter().map(|cand| (cand.id as i64, cand)).collect();
            label = "suggested dates".into();
            Box::new(move |it: &Cur| sugg.get(&it.id).and_then(|s| parse(&s.local)).map(|l| (l, it.offset)))
        }
        m => anyhow::bail!("Unknown fix {m}"),
    };
    let mut out = Vec::new();
    for it in &items {
        let Some((local, offset)) = new_of(it) else { continue };
        let old = edits::read(c, it.id, "date")?;
        // QuickTime dates without an offset are UTC: keep the instant when
        // only the zone changes.
        let utc = match (fix.mode.as_str(), it.source.as_deref(), it.offset, offset) {
            ("zone", Some("quicktime"), None, Some(o)) => {
                let u = it.utc.unwrap_or_default();
                let l = chrono::DateTime::from_timestamp_millis(u + o as i64 * 60_000).unwrap_or_default().naive_utc();
                out.push((it.id, old, json!({"utc": u, "local": fmt(l), "offset": o, "source": "manual", "conf": 1.0})));
                continue;
            }
            _ => local.and_utc().timestamp_millis() - offset.unwrap_or_else(|| tz.offset_for_local(local)) as i64 * 60_000,
        };
        let new = json!({"utc": utc, "local": fmt(local), "offset": offset, "source": "manual", "conf": 1.0});
        if new.get("utc") != old.get("utc") || new.get("local") != old.get("local") || new.get("offset") != old.get("offset") {
            out.push((it.id, old, new));
        }
    }
    Ok((out, label))
}

pub fn preview(c: &Connection, fix: &DateFix, tz: &dyn TzGuess) -> anyhow::Result<DatePreview> {
    let (changes, label) = compute(c, fix, tz)?;
    let get = |v: &Value| v.get("local").and_then(Value::as_str).unwrap_or("").to_string();
    let mut befores: Vec<String> = changes.iter().map(|(_, o, _)| get(o)).filter(|s| !s.is_empty()).collect();
    let mut afters: Vec<String> = changes.iter().map(|(_, _, n)| get(n)).collect();
    befores.sort();
    afters.sort();
    let mut year_changes = 0;
    let mut folder_changes = 0;
    for (id, o, n) in &changes {
        let (a, b) = (get(o), get(n));
        if a.get(..4) != b.get(..4) {
            year_changes += 1;
            if load(c, *id)?.decided {
                folder_changes += 1;
            }
        }
    }
    // The reference first, then an even spread.
    let mut samples = Vec::new();
    let refs: Vec<i64> = [fix.reference.as_ref(), fix.second.as_ref()].into_iter().flatten().map(|r| r.asset_id).collect();
    let step = (changes.len() / 8).max(1);
    for (i, (id, o, n)) in changes.iter().enumerate() {
        if refs.contains(id) || i % step == 0 {
            samples.push(DateSample { id: *id as i32, before: get(o), after: get(n) });
        }
    }
    samples.sort_by_key(|s| !refs.contains(&(s.id as i64)));
    samples.truncate(10);
    Ok(DatePreview {
        count: changes.len() as i32,
        label,
        before_from: befores.first().cloned().unwrap_or_default(),
        before_to: befores.last().cloned().unwrap_or_default(),
        after_from: afters.first().cloned().unwrap_or_default(),
        after_to: afters.last().cloned().unwrap_or_default(),
        year_changes,
        folder_changes,
        samples,
    })
}

/// Apply as one batch. Returns (batch id, items changed).
pub fn apply(c: &Connection, fix: &DateFix, tz: &dyn TzGuess) -> anyhow::Result<(i64, usize)> {
    let (changes, label) = compute(c, fix, tz)?;
    if changes.is_empty() {
        return Ok((0, 0));
    }
    let n = changes.len();
    let items = if n == 1 { "1 item".to_string() } else { format!("{n} items") };
    let reference = fix
        .reference
        .as_ref()
        .and_then(|r| c.query_row("SELECT name FROM assets WHERE id = ?1", [r.asset_id], |row| row.get::<_, String>(0)).ok());
    let batch_label = match fix.mode.as_str() {
        "zone" => format!("Set the time zone of {items} to {}", label.trim_start_matches("time zone ")),
        "exact" | "suggested" => format!("Date {items}"),
        _ => format!("Shift {items} {label}"),
    };
    let why = match (fix.mode.as_str(), &reference) {
        ("shift", Some(r)) => format!("Shifted {label} from the camera clock (reference: {r})."),
        ("drift", Some(r)) => format!("Shifted {label} (references: {r} and one more)."),
        ("zone", _) => format!("Time zone set to {}.", label.trim_start_matches("time zone ")),
        ("amount", _) => format!("Shifted {label} by hand."),
        ("smart", _) => format!("Shifted {label} to match phone photos of the same moments."),
        ("suggested", _) => "Date suggested from the file name, folder or neighbouring files.".into(),
        _ => "Date set by hand.".into(),
    };
    let batch = edits::begin_batch(c, "edit", &batch_label)?;
    for (id, _, new) in &changes {
        edits::set(
            c,
            Some(batch),
            *id,
            &Change { field: "date", value: new.clone(), source: "manual", source_ref: None, conf: Some(1.0), why: Some(why.clone()) },
        )?;
    }
    Ok((batch, n))
}

/// A camera clock offset found from phone photos of the same moments.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SmartOffset {
    /// Seconds to add to the items' times.
    #[ts(type = "number")]
    pub seconds: i64,
    /// "+9 h 28 min 5 s".
    pub label: String,
    /// The phones' time zone at that time, when they state one.
    pub offset_min: Option<i32>,
    /// Items with a look-alike phone photo at that offset.
    pub matched: i32,
    /// Items that could be compared (dated, with a thumbnail).
    pub compared: i32,
    /// The closest pairs, as evidence.
    pub pairs: Vec<OffsetPair>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OffsetPair {
    pub item: i32,
    pub phone: i32,
    pub item_local: String,
    pub phone_local: String,
}

/// Look-alike photos: dHash within this many of 64 bits.
const PAIR_BITS: u32 = 10;
/// Phone photos this far from the camera's dates are not compared.
const PAIR_WINDOW_S: i64 = 48 * 3600;
/// Pairs within this many seconds of each other agree on the offset.
const AGREE_S: i64 = 180;

struct Shot {
    id: i64,
    local: NaiveDateTime,
    hash: u64,
    offset: Option<i32>,
    phone: bool,
}

fn shots(c: &Connection, sql: &str, p: impl rusqlite::Params) -> rusqlite::Result<Vec<Shot>> {
    let mut st = c.prepare(sql)?;
    let rows = st.query_map(p, |r| {
        let (make, model): (Option<String>, Option<String>) = (r.get(4)?, r.get(5)?);
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<i64>>(3)?,
            is_phone(make.as_deref(), model.as_deref()),
        ))
    })?;
    Ok(rows
        .filter_map(Result::ok)
        .filter_map(|(id, l, h, o, phone)| Some(Shot { id, local: parse(&l)?, hash: h as u64, offset: o.map(|o| o as i32), phone }))
        .collect())
}

/// The smart offset (4.5): phone photos in the archive that look like the
/// items and were taken within 48 h of them; the median time difference of
/// the pairs that agree. `None` when fewer than three items pair up.
pub fn smart_offset(c: &Connection, ids: &[i64]) -> rusqlite::Result<Option<SmartOffset>> {
    if ids.is_empty() {
        return Ok(None);
    }
    let list = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let items: Vec<Shot> = shots(
        c,
        &format!(
            "SELECT id, taken_local, phash, tz_offset, make, model FROM assets
             WHERE id IN ({list}) AND kind = 0 AND phash IS NOT NULL AND taken_local IS NOT NULL"
        ),
        [],
    )?;
    let (Some(lo), Some(hi)) = (items.iter().map(|s| s.local).min(), items.iter().map(|s| s.local).max()) else { return Ok(None) };
    let window = Duration::seconds(PAIR_WINDOW_S);
    let mine: std::collections::HashSet<i64> = ids.iter().copied().collect();
    let phones: Vec<Shot> = shots(
        c,
        "SELECT id, taken_local, phash, tz_offset, make, model FROM assets
         WHERE missing = 0 AND trashed_at IS NULL AND kind = 0 AND phash IS NOT NULL AND taken_local BETWEEN ?1 AND ?2",
        params![fmt(lo - window), fmt(hi + window)],
    )?
    .into_iter()
    .filter(|s| !mine.contains(&s.id) && s.phone)
    .collect();

    // (item index, phone index, seconds, bits); the 8 closest looks per item.
    let mut cands: Vec<(usize, usize, i64, u32)> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let mut mine: Vec<(usize, usize, i64, u32)> = phones
            .iter()
            .enumerate()
            .filter_map(|(j, p)| {
                let d = (p.local - it.local).num_seconds();
                let bits = (it.hash ^ p.hash).count_ones();
                (d.abs() <= PAIR_WINDOW_S && bits <= PAIR_BITS).then_some((i, j, d, bits))
            })
            .collect();
        mine.sort_by_key(|x| x.3);
        cands.extend(mine.into_iter().take(8));
    }
    if cands.is_empty() {
        return Ok(None);
    }
    // The offset most items agree on: a sliding window over the differences.
    cands.sort_by_key(|x| x.2);
    let (mut best, mut best_n, mut end) = ((0, 0), 0, 0);
    for start in 0..cands.len() {
        end = end.max(start);
        while end + 1 < cands.len() && cands[end + 1].2 - cands[start].2 <= 2 * AGREE_S {
            end += 1;
        }
        let n = cands[start..=end].iter().map(|x| x.0).collect::<std::collections::HashSet<_>>().len();
        if n > best_n {
            (best, best_n) = ((start, end), n);
        }
    }
    if best_n < 3 {
        return Ok(None);
    }
    // Each item's closest look-alike inside that window.
    let mut per_item: HashMap<usize, (usize, i64, u32)> = HashMap::new();
    for &(i, j, d, bits) in &cands[best.0..=best.1] {
        if per_item.get(&i).is_none_or(|x| bits < x.2) {
            per_item.insert(i, (j, d, bits));
        }
    }
    let mut deltas: Vec<i64> = per_item.values().map(|x| x.1).collect();
    deltas.sort_unstable();
    let seconds = deltas[deltas.len() / 2];
    let mut zones: HashMap<i32, usize> = HashMap::new();
    for (j, _, _) in per_item.values() {
        if let Some(o) = phones[*j].offset {
            *zones.entry(o).or_default() += 1;
        }
    }
    let mut pairs: Vec<(&usize, &(usize, i64, u32))> = per_item.iter().collect();
    pairs.sort_by_key(|(i, x)| (x.2, items[**i].local));
    let first = items.iter().map(|s| s.local).min().unwrap_or(lo);
    Ok(Some(SmartOffset {
        seconds,
        label: diff_label(first, first + Duration::seconds(seconds)),
        offset_min: zones.into_iter().max_by_key(|(o, n)| (*n, *o)).map(|(o, _)| o),
        matched: per_item.len() as i32,
        compared: items.len() as i32,
        pairs: pairs
            .into_iter()
            .take(6)
            .map(|(i, (j, _, _))| OffsetPair {
                item: items[*i].id as i32,
                phone: phones[*j].id as i32,
                item_local: fmt(items[*i].local),
                phone_local: fmt(phones[*j].local),
            })
            .collect(),
    }))
}

/// A suggested date for an item without a trusted one.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Candidate {
    pub id: i32,
    pub local: String,
    /// "filename", "folder", "sequence" or "file_time".
    pub source: String,
    pub conf: f32,
    pub why: String,
}

static COUNTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([A-Za-z_]+?)_?(\d{3,6})\.[A-Za-z0-9]+$").unwrap());

/// Best date candidate per item, from the file name, the camera's counter
/// between two dated neighbours, the folder name, or the file time.
pub fn candidates(c: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<Candidate>> {
    let mut out = Vec::new();
    for id in ids {
        let (name, dir, root, mtime): (String, String, i64, i64) =
            c.query_row("SELECT name, dir, root_id, mtime FROM assets WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
        if let Some(n) = from_file_name(&name).filter(|n| !n.is_utc) {
            out.push(Candidate {
                id: *id as i32,
                local: fmt(n.local),
                source: "filename".into(),
                conf: 0.7,
                why: format!("The file name {name} contains the date."),
            });
            continue;
        }
        if let Some(cand) = sequence(c, *id, &name, &dir, root)? {
            out.push(cand);
            continue;
        }
        if let Some(n) = dir.rsplit('/').find_map(from_folder_name) {
            let seg = dir.rsplit('/').find(|s| from_folder_name(s).is_some()).unwrap_or("");
            out.push(Candidate {
                id: *id as i32,
                local: fmt(n.local),
                source: "folder".into(),
                conf: 0.4,
                why: format!("The folder “{seg}” names the date."),
            });
            continue;
        }
        if let Some(d) = chrono::DateTime::from_timestamp_millis(mtime) {
            let l = d.with_timezone(&chrono::Local).naive_local();
            out.push(Candidate {
                id: *id as i32,
                local: fmt(l),
                source: "file_time".into(),
                conf: 0.2,
                why: "The file's modification time; often the copy date.".into(),
            });
        }
    }
    Ok(out)
}

/// IMG_1234 between dated IMG_1233 and IMG_1235 of the same folder.
fn sequence(c: &Connection, id: i64, name: &str, dir: &str, root: i64) -> rusqlite::Result<Option<Candidate>> {
    let Some(m) = COUNTER.captures(name) else { return Ok(None) };
    let (prefix, n): (String, i64) = (m[1].to_string(), m[2].parse().unwrap_or(0));
    let mut st = c.prepare(
        "SELECT name, taken_local FROM assets WHERE root_id = ?1 AND dir = ?2 AND id != ?3 AND taken_local IS NOT NULL
           AND date_source IN ('exif_original', 'exif_digitized', 'quicktime', 'xmp', 'manual') AND name LIKE ?4",
    )?;
    let mut below: Option<(i64, NaiveDateTime)> = None;
    let mut above: Option<(i64, NaiveDateTime)> = None;
    for row in st.query_map(params![root, dir, id, format!("{prefix}%")], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (other, local) = row?;
        let Some(k) = COUNTER.captures(&other).filter(|m| m[1] == prefix).and_then(|m| m[2].parse::<i64>().ok()) else { continue };
        let Some(l) = parse(&local) else { continue };
        if k < n && below.is_none_or(|(b, _)| k > b) {
            below = Some((k, l));
        }
        if k > n && above.is_none_or(|(a, _)| k < a) {
            above = Some((k, l));
        }
    }
    let (Some((kb, lb)), Some((ka, la))) = (below, above) else { return Ok(None) };
    if ka - kb > 50 || la < lb || la - lb > Duration::days(2) {
        return Ok(None);
    }
    let f = (n - kb) as f64 / (ka - kb) as f64;
    let local = lb + Duration::milliseconds(((la - lb).num_milliseconds() as f64 * f) as i64);
    Ok(Some(Candidate {
        id: id as i32,
        local: fmt(local),
        source: "sequence".into(),
        conf: 0.6,
        why: format!("Between {prefix}{kb} and {prefix}{ka} from the same camera."),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dates::FixedTz;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    fn add(c: &Connection, rel: &str, make: &str, model: &str, local: &str, offset: Option<i64>) -> i64 {
        let id = insert_asset(c, rel);
        let l = parse(local).unwrap();
        let utc = l.and_utc().timestamp_millis() - offset.unwrap_or(120) * 60_000;
        c.execute(
            "UPDATE assets SET make = ?2, model = ?3, taken_local = ?4, taken_utc = ?5, tz_offset = ?6, date_source = 'exif_original' WHERE id = ?1",
            params![id, make, model, local, utc, offset],
        )
        .unwrap();
        id
    }

    #[test]
    fn labels() {
        let a = parse("2020-08-03T11:08:28").unwrap();
        let b = parse("2019-08-03T09:08:28").unwrap();
        assert_eq!(diff_label(a, b), "−1 y 2 h");
        assert_eq!(diff_label(b, a), "+1 y 2 h");
        assert_eq!(diff_label(a, a + Duration::seconds(125)), "+2 min 5 s");
    }

    #[test]
    fn detects_and_shifts_a_camera_set() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let a = add(&c, "Old events/Italy 2019/DSC_0001.JPG", "NIKON CORPORATION", "NIKON D750", "2020-08-03T11:08:28", None);
        let b = add(&c, "Old events/Italy 2019/DSC_0002.JPG", "NIKON CORPORATION", "NIKON D750", "2020-08-03T13:39:12", None);
        add(&c, "Dump/IMG_1001.JPG", "Apple", "iPhone XS", "2019-08-03T09:06:57", Some(120));
        let s = sets(&c, i64::MAX).unwrap();
        assert_eq!(s.len(), 1, "phones without problems are not listed: {s:?}");
        assert_eq!(s[0].label, "NIKON D750");
        assert!(s[0].reasons[0].contains("The folder says 2019"), "{:?}", s[0].reasons);

        let fix = DateFix {
            ids: vec![a, b],
            mode: "shift".into(),
            reference: Some(DateRef { asset_id: a, local: "2019-08-03T09:08:28".into(), offset_min: Some(120) }),
            second: None,
            offset_min: None,
            amount: None,
            exact: None,
            camera_from: None,
            camera_to: None,
        };
        let p = preview(&c, &fix, &FixedTz(60)).unwrap();
        assert_eq!(p.count, 2);
        assert_eq!(p.label, "−1 y 2 h, UTC+2");
        assert_eq!(p.after_from, "2019-08-03T09:08:28");
        assert_eq!(p.after_to, "2019-08-03T11:39:12");
        assert_eq!(p.year_changes, 2);
        let (batch, n) = apply(&c, &fix, &FixedTz(60)).unwrap();
        assert_eq!(n, 2);
        let (local, off, utc): (String, i64, i64) = c
            .query_row("SELECT taken_local, tz_offset, taken_utc FROM assets WHERE id = ?1", [b], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!((local.as_str(), off), ("2019-08-03T11:39:12", 120));
        assert_eq!(utc, parse("2019-08-03T09:39:12").unwrap().and_utc().timestamp_millis());
        // The fixed set stays listed (it is a dedicated camera) but is no
        // longer suspect.
        assert!(sets(&c, i64::MAX).unwrap()[0].reasons.is_empty());
        edits::undo_batch(&c, batch).unwrap();
        let local: String = c.query_row("SELECT taken_local FROM assets WHERE id = ?1", [b], |r| r.get(0)).unwrap();
        assert_eq!(local, "2020-08-03T13:39:12");
    }

    #[test]
    fn home_zone_drift_and_amounts() {
        let db = Db::memory().unwrap();
        let c = db.write();
        // Canon on home time (UTC+2) in New York, read as UTC+2; phones say UTC−4.
        let cam = add(&c, "Dump/IMG_8801.JPG", "Canon", "Canon EOS 80D", "2021-06-10T14:50:13", None);
        add(&c, "Dump/IMG_5001.HEIC", "Apple", "iPhone 12", "2021-06-10T08:40:00", Some(-240));
        let s = sets(&c, i64::MAX).unwrap();
        assert!(s[0].reasons.iter().any(|r| r.contains("UTC−4")), "{:?}", s[0].reasons);
        let base = DateFix {
            ids: vec![cam],
            mode: "shift".into(),
            reference: Some(DateRef { asset_id: cam, local: "2021-06-10T08:50:13".into(), offset_min: Some(-240) }),
            second: None,
            offset_min: None,
            amount: None,
            exact: None,
            camera_from: None,
            camera_to: None,
        };
        let (changes, _) = compute(&c, &base, &FixedTz(120)).unwrap();
        // The instant stays: only the clock's zone was wrong.
        assert_eq!(changes[0].1["utc"], changes[0].2["utc"]);
        assert_eq!(changes[0].2["offset"], -240);

        // Drift: +120 s at the first reference, +420 s a day later.
        let d1 = add(&c, "Dump/DSCF2001.JPG", "FUJIFILM", "X-T3", "2019-08-03T08:02:00", None);
        let d2 = add(&c, "Dump/DSCF2002.JPG", "FUJIFILM", "X-T3", "2019-08-04T08:07:00", None);
        let mid = add(&c, "Dump/DSCF2003.JPG", "FUJIFILM", "X-T3", "2019-08-03T20:04:30", None);
        let drift = DateFix {
            ids: vec![d1, d2, mid],
            mode: "drift".into(),
            reference: Some(DateRef { asset_id: d1, local: "2019-08-03T08:00:00".into(), offset_min: None }),
            second: Some(DateRef { asset_id: d2, local: "2019-08-04T08:00:00".into(), offset_min: None }),
            ..base.clone()
        };
        let (changes, _) = compute(&c, &drift, &FixedTz(120)).unwrap();
        let m = changes.iter().find(|(id, _, _)| *id == mid).unwrap();
        assert_eq!(m.2["local"], "2019-08-03T20:00:00");

        let amount = DateFix {
            ids: vec![cam],
            mode: "amount".into(),
            reference: None,
            amount: Some(Amount { years: -1, hours: 2, ..Default::default() }),
            ..base.clone()
        };
        let (changes, label) = compute(&c, &amount, &FixedTz(120)).unwrap();
        assert_eq!(changes[0].2["local"], "2020-06-10T16:50:13");
        assert_eq!(label, "−11 mo 30 d 22 h");

        let zone = DateFix { ids: vec![cam], mode: "zone".into(), reference: None, offset_min: Some(-240), ..base };
        let (changes, _) = compute(&c, &zone, &FixedTz(120)).unwrap();
        assert_eq!(changes[0].2["local"], "2021-06-10T14:50:13");
        assert_eq!(changes[0].2["offset"], -240);
    }

    #[test]
    fn candidates_for_undated_items() {
        let db = Db::memory().unwrap();
        let c = db.write();
        add(&c, "Dump/IMG_1233.JPG", "Apple", "iPhone 12", "2020-02-14T18:04:00", Some(60));
        add(&c, "Dump/IMG_1235.JPG", "Apple", "iPhone 12", "2020-02-14T18:06:00", Some(60));
        let lone = insert_asset(&c, "Dump/IMG_1234.JPG");
        let named = insert_asset(&c, "Dump/Screenshot_20200301-101500.png");
        let foldered = insert_asset(&c, "2019-07 Holidays/scan.jpg");
        let got = candidates(&c, &[lone, named, foldered]).unwrap();
        assert_eq!((got[0].source.as_str(), got[0].local.as_str()), ("sequence", "2020-02-14T18:05:00"));
        assert_eq!((got[1].source.as_str(), got[1].local.as_str()), ("filename", "2020-03-01T10:15:00"));
        assert_eq!(got[2].source, "folder");
        assert!(got[2].local.starts_with("2019-07"));
    }
}
