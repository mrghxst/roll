//! Add photos (4.11): bring a folder from outside the archive in, usually as
//! one event. The folder is indexed read-only as a staging root; its items
//! are staged (`missing = 2`) and stay out of every archive view. Roll
//! checks them against the archive (copies it already has, the camera
//! clock, places), proposes one event, and copies or moves them in as one
//! undoable batch: the files land in the event folder (or their category's
//! folder), dates, places and tags are written into the copies, and the
//! source stays untouched when copying.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use crate::datefix::{self, DateSet, SmartOffset};
use crate::db::now_ms;
use crate::edits::{self, Change};
use crate::events::PlaceOf;
use crate::model::{Scope, problem};
use crate::paths::join_rel;
use crate::planner::{self, PlanFolder};

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Count {
    pub label: String,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportInfo {
    pub id: i32,
    /// The staging root: `Query.staged` shows its items.
    pub root_id: i32,
    pub source: String,
    /// "checking", "applying" or "applied".
    pub state: String,
    /// "folder", "takeout" (a Google Takeout export) or "whatsapp" (a chat
    /// export).
    pub kind: String,
    /// What Roll read from the export: "Dates from the chat for 50 of 52
    /// items."
    pub facts: Vec<String>,
    /// The event name: the owner's, or proposed from the folder name, or
    /// "{Month YYYY} {Place}".
    pub name: String,
    pub custom_name: bool,
    pub count: i32,
    pub photos: i32,
    pub videos: i32,
    pub from_local: String,
    pub to_local: String,
    pub cameras: Vec<Count>,
    pub formats: Vec<Count>,
    /// Exact copies of archive items (same BLAKE3).
    pub exact: Vec<i32>,
    /// Look like archive items that are at least as large (dHash).
    pub similar: Vec<i32>,
    /// Repeats of other items in the folder; one of each is added.
    pub twins: Vec<i32>,
    /// Items the owner left out.
    pub left_out: Vec<i32>,
    /// Items still waiting for a thumbnail while indexing runs.
    pub pending: i32,
    /// Camera sets with what looks wrong about their clocks, and items
    /// without a trusted date.
    pub date_sets: Vec<DateSet>,
    /// The offset phone photos of the same moments suggest.
    pub smart: Option<SmartOffset>,
    pub no_date: i32,
    pub no_place: i32,
    /// The place the located items share.
    pub place: Option<String>,
    /// What sorting proposes: "Photos 38", "Screenshots 2".
    pub categories: Vec<Count>,
    pub dates_ok: bool,
    pub places_ok: bool,
    /// Every item to bring in, in time order.
    pub ids: Vec<i32>,
    /// Items the applied batch brought into the library.
    pub added: Vec<i32>,
    pub batch_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportOptions {
    /// "copy" (the source stays untouched) or "move".
    pub mode: String,
    /// Leave out items the archive already has.
    pub skip_known: bool,
    /// The library folder to add to; the first one when empty.
    pub root_id: Option<i32>,
    /// Add as one event with its own folder (a card, a trip). Off for
    /// exports that span years: items go to their category folders.
    pub as_event: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions { mode: "copy".into(), skip_known: true, root_id: None, as_event: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportPlan {
    pub root_id: i32,
    pub root_path: String,
    pub event: String,
    /// Media files to copy or move (with group members).
    pub files: i32,
    pub sidecars: i32,
    /// Items left in the source (already in the archive, or left out).
    pub skipped: i32,
    #[ts(type = "number")]
    pub bytes: i64,
    /// Free space where the files go; -1 when unknown.
    #[ts(type = "number")]
    pub free: i64,
    pub folders: Vec<PlanFolder>,
    /// Dates, places and tags are written into the files (ExifTool found).
    pub writes: bool,
}

/// Folder names that say nothing about the event: card and camera folders.
static GENERIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(dcim|\d{3}[a-z_]{5}|\d{3}_?[a-z]*|camera( ?(card|roll|uploads))?|camera[-_ ]card|card|sd( ?card)?|untitled|no name|eos_digital|new folder|import(s)?|photos|pictures|images|bilder|fotos|[a-z]:?)$",
    )
    .unwrap()
});

/// The source folder's name when it names the event ("2019 Lake Garda").
pub fn folder_name(source: &Path) -> Option<String> {
    let name = source.file_name()?.to_string_lossy().trim().to_string();
    (!name.is_empty() && !GENERIC.is_match(&name)).then_some(name)
}

#[derive(Debug, Clone)]
struct Item {
    id: i64,
    kind: i64,
    format: String,
    camera: Option<String>,
    phone: bool,
    local: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    problems: i64,
    date_source: Option<String>,
    phash: Option<u64>,
    width: i64,
    height: i64,
    category: Option<String>,
    category_name: Option<String>,
    review: i64,
}

/// "Canon EOS R6", not "Canon Canon EOS R6".
fn camera_label(make: Option<&str>, model: Option<&str>) -> Option<String> {
    match (make, model) {
        (None, None) => None,
        (Some(mk), None) => Some(mk.to_string()),
        (None, Some(m)) => Some(m.to_string()),
        (Some(mk), Some(m)) => {
            let brand = mk.split_whitespace().next().unwrap_or("").to_lowercase();
            Some(if m.to_lowercase().starts_with(&brand) { m.to_string() } else { format!("{mk} {m}") })
        }
    }
}

fn items(c: &Connection, root: i64) -> rusqlite::Result<Vec<Item>> {
    let mut st = c.prepare(
        "SELECT a.id, a.kind, a.format, a.make, a.model, a.taken_local, a.lat, a.lon, a.problems, a.date_source, a.phash,
                coalesce(a.width, 0), coalesce(a.height, 0), c.key, c.name, a.review
         FROM assets a LEFT JOIN categories c ON c.id = a.category_id
         WHERE a.root_id = ?1 AND a.missing = 2 AND a.trashed_at IS NULL AND (a.group_role IS NULL OR a.group_role = 'primary')
         ORDER BY coalesce(a.taken_local, '9999'), a.rel_path",
    )?;
    st.query_map([root], |r| {
        let make: Option<String> = r.get(3)?;
        let model: Option<String> = r.get(4)?;
        let camera = camera_label(make.as_deref(), model.as_deref());
        Ok(Item {
            id: r.get(0)?,
            kind: r.get(1)?,
            format: r.get(2)?,
            phone: datefix::is_phone(make.as_deref(), model.as_deref()),
            camera,
            local: r.get(5)?,
            lat: r.get(6)?,
            lon: r.get(7)?,
            problems: r.get(8)?,
            date_source: r.get(9)?,
            phash: r.get::<_, Option<i64>>(10)?.map(|v| v as u64),
            width: r.get(11)?,
            height: r.get(12)?,
            category: r.get(13)?,
            category_name: r.get(14)?,
            review: r.get(15)?,
        })
    })?
    .collect()
}

fn counts(list: impl Iterator<Item = String>) -> Vec<Count> {
    let mut m: HashMap<String, i32> = HashMap::new();
    for l in list {
        *m.entry(l).or_default() += 1;
    }
    let mut v: Vec<Count> = m.into_iter().map(|(label, count)| Count { label, count }).collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
    v
}

/// Staged items with the same content as another item: in the archive
/// (`twins = false`), or earlier in the same folder (`twins = true`; Takeout
/// repeats album photos in its year folders). BLAKE3 hashes missing on
/// either side (the hash job may not have run yet) are made now.
fn copies(c: &Connection, root: i64, twins: bool) -> rusqlite::Result<Vec<i64>> {
    struct Side {
        id: i64,
        blake3: Option<String>,
        root: String,
        rel: String,
    }
    let other = if twins { "a.root_id = s.root_id AND a.missing = 2 AND a.id < s.id" } else { "a.missing = 0" };
    let pairs: Vec<(Side, Side)> = {
        let mut st = c.prepare(&format!(
            "SELECT s.id, s.blake3, rs.path, s.rel_path, a.id, a.blake3, ra.path, a.rel_path
             FROM assets s JOIN roots rs ON rs.id = s.root_id
             JOIN assets a ON a.size = s.size AND a.quick_hash = s.quick_hash AND {other} AND a.trashed_at IS NULL
             JOIN roots ra ON ra.id = a.root_id
             WHERE s.root_id = ?1 AND s.missing = 2"
        ))?;
        st.query_map([root], |r| {
            Ok((
                Side { id: r.get(0)?, blake3: r.get(1)?, root: r.get(2)?, rel: r.get(3)? },
                Side { id: r.get(4)?, blake3: r.get(5)?, root: r.get(6)?, rel: r.get(7)? },
            ))
        })?
        .collect::<Result<_, _>>()?
    };
    let mut known: HashMap<i64, Option<String>> = HashMap::new();
    let mut hash = |side: Side| -> Option<String> {
        known
            .entry(side.id)
            .or_insert_with(|| {
                side.blake3.or_else(|| {
                    let h = roll_media::hash::blake3_file(&join_rel(Path::new(&side.root), &side.rel)).ok()?;
                    let _ = c.execute("UPDATE assets SET blake3 = ?2 WHERE id = ?1", params![side.id, h]);
                    Some(h)
                })
            })
            .clone()
    };
    let mut out = Vec::new();
    for (s, a) in pairs {
        let id = s.id;
        if out.contains(&id) {
            continue;
        }
        let (x, y) = (hash(s), hash(a));
        if x.is_some() && x == y {
            out.push(id);
        }
    }
    Ok(out)
}

/// Exact copies of archive items, and repeats within the folder.
fn exact_copies(c: &Connection, root: i64) -> rusqlite::Result<(Vec<i64>, Vec<i64>)> {
    let archive = copies(c, root, false)?;
    let mut twins = copies(c, root, true)?;
    twins.retain(|t| !archive.contains(t));
    Ok((archive, twins))
}

/// Staged photos that look like an archive photo of the same shape that is
/// at least as large: dHash within 2 bits, aspect within 1 %.
fn similar_copies(c: &Connection, staged: &[Item], skip: &HashSet<i64>) -> rusqlite::Result<Vec<i64>> {
    let todo: Vec<&Item> = staged.iter().filter(|i| i.kind == 0 && i.phash.is_some() && i.height > 0 && !skip.contains(&i.id)).collect();
    if todo.is_empty() {
        return Ok(Vec::new());
    }
    let archive: Vec<(u64, i64, i64)> = {
        let mut st = c.prepare(
            "SELECT phash, coalesce(width, 0), coalesce(height, 0) FROM assets
             WHERE missing = 0 AND trashed_at IS NULL AND kind = 0 AND phash IS NOT NULL AND height > 0",
        )?;
        st.query_map([], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?
    };
    Ok(todo
        .into_iter()
        .filter(|i| {
            let (h, aspect) = (i.phash.unwrap(), i.width as f64 / i.height as f64);
            archive.iter().any(|(ah, w, ht)| {
                (h ^ ah).count_ones() <= 2 && ((*w as f64 / *ht as f64) / aspect - 1.0).abs() < 0.01 && w * ht >= i.width * i.height
            })
        })
        .map(|i| i.id)
        .collect())
}

struct Row {
    id: i64,
    root_id: i64,
    source: String,
    state: String,
    name: Option<String>,
    dates_ok: bool,
    places_ok: bool,
    left_out: Vec<i64>,
    batch_id: Option<i64>,
    kind: String,
    facts: Vec<String>,
}

fn row(c: &Connection, id: i64) -> rusqlite::Result<Row> {
    c.query_row(
        "SELECT i.id, i.root_id, r.path, i.state, i.name, i.dates_ok, i.places_ok, i.left_out, i.batch_id, i.kind, i.facts
         FROM imports i JOIN roots r ON r.id = i.root_id WHERE i.id = ?1",
        [id],
        |r| {
            Ok(Row {
                id: r.get(0)?,
                root_id: r.get(1)?,
                source: r.get(2)?,
                state: r.get(3)?,
                name: r.get(4)?,
                dates_ok: r.get(5)?,
                places_ok: r.get(6)?,
                left_out: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
                batch_id: r.get(8)?,
                kind: r.get(9)?,
                facts: serde_json::from_str(&r.get::<_, String>(10)?).unwrap_or_default(),
            })
        },
    )
}

/// Start an import of `source`: a staging root and its import row. The
/// caller scans the root. Returns (import id, root id).
pub fn start(c: &Connection, source: &Path) -> anyhow::Result<(i64, i64)> {
    let p = source.to_string_lossy().into_owned();
    let roots: Vec<(i64, String, String)> = {
        let mut st = c.prepare("SELECT id, path, kind FROM roots")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?
    };
    for (id, path, kind) in &roots {
        let existing = Path::new(path);
        if kind == "library" && source.starts_with(existing) {
            anyhow::bail!("This folder is already in the library. Its new items show in the Inbox.");
        }
        if kind == "library" && existing.starts_with(source) {
            anyhow::bail!("This folder holds the library folder {path}. Pick the folder with the new photos.");
        }
        if kind == "staging" && existing == source {
            let open: Option<i64> =
                c.query_row("SELECT id FROM imports WHERE root_id = ?1 AND state != 'closed'", [id], |r| r.get(0)).optional()?;
            if let Some(i) = open {
                return Ok((i, *id));
            }
            c.execute(
                "UPDATE imports SET state = 'checking', dates_ok = 0, places_ok = 0, name = NULL, left_out = '[]' WHERE root_id = ?1",
                [id],
            )?;
            let i: i64 = c.query_row("SELECT id FROM imports WHERE root_id = ?1", [id], |r| r.get(0))?;
            return Ok((i, *id));
        }
    }
    c.execute("INSERT INTO roots (path, kind, added_at) VALUES (?1, 'staging', ?2)", params![p, now_ms()])?;
    let root = c.last_insert_rowid();
    c.execute("INSERT INTO imports (root_id, state, created_at) VALUES (?1, 'checking', ?2)", params![root, now_ms()])?;
    Ok((c.last_insert_rowid(), root))
}

/// Open imports, newest first.
pub fn list_ids(c: &Connection) -> rusqlite::Result<Vec<i64>> {
    let mut st = c.prepare("SELECT id FROM imports WHERE state != 'closed' ORDER BY id DESC")?;
    st.query_map([], |r| r.get(0))?.collect()
}

/// Read what an export says about its items, after each scan of the
/// staging root: WhatsApp chats give each file its sending time and sender;
/// Google Takeout JSON (already applied by the scan) gets the time zone of
/// its place, and album folders become `Album/<name>` tags, shared with the
/// copies of the same photo in the year folders.
pub fn prepare(c: &Connection, id: i64, tz: &dyn crate::dates::TzGuess) -> anyhow::Result<()> {
    let r = row(c, id)?;
    let source = PathBuf::from(&r.source);
    struct Staged {
        id: i64,
        dir: String,
        name: String,
        date_source: Option<String>,
    }
    let staged: Vec<Staged> = {
        let mut st = c.prepare("SELECT id, dir, name, date_source FROM assets WHERE root_id = ?1 AND missing = 2")?;
        st.query_map([r.root_id], |x| Ok(Staged { id: x.get(0)?, dir: x.get(1)?, name: x.get(2)?, date_source: x.get(3)? }))?
            .collect::<Result<_, _>>()?
    };
    let mut kind = "folder";
    let mut facts = Vec::new();

    // WhatsApp: chat texts next to the media (the export folder, or one
    // folder per chat inside it).
    let chats: Vec<PathBuf> = walkdir::WalkDir::new(&source)
        .max_depth(3)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file() && roll_meta::whatsapp::is_chat_file(&e.file_name().to_string_lossy()))
        .map(|e| e.into_path())
        .collect();
    for chat in &chats {
        let dir = chat.parent().unwrap_or(&source);
        let rel = crate::paths::rel_string(dir.strip_prefix(&source).unwrap_or(Path::new("")));
        let here: Vec<&Staged> = staged.iter().filter(|s| s.dir == rel).collect();
        let known: Vec<String> = here.iter().map(|s| s.name.to_lowercase()).collect();
        let Ok(text) = std::fs::read_to_string(chat) else { continue };
        let parsed = roll_meta::whatsapp::parse(&text, &known);
        let chat_name = roll_meta::whatsapp::chat_name(
            &chat.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            &dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        );
        let mut dated = 0;
        for s in &here {
            let Some(sent) = parsed.files.get(&s.name.to_lowercase()) else { continue };
            // A file sent as a document keeps its camera date.
            if !matches!(s.date_source.as_deref(), Some("exif_original" | "quicktime" | "xmp" | "sidecar")) {
                let local = sent.local.format("%Y-%m-%dT%H:%M:%S").to_string();
                let utc = sent.local.and_utc().timestamp_millis() - tz.offset_for_local(sent.local) as i64 * 60_000;
                let why = format!(
                    "Sent by {} in the WhatsApp chat{} on {}.",
                    sent.sender,
                    chat_name.as_deref().map(|n| format!(" with {n}")).unwrap_or_default(),
                    sent.local.format("%-d %b %Y at %H:%M")
                );
                let value = json!({"utc": utc, "local": local, "offset": null, "source": "chat", "conf": 0.95});
                edits::set(
                    c,
                    None,
                    s.id,
                    &Change { field: "date", value, source: "import", source_ref: None, conf: Some(0.95), why: Some(why) },
                )?;
            }
            crate::tags::add(c, s.id, "Source/WhatsApp", "import", None)?;
            crate::tags::add(c, s.id, &format!("From/{}", sent.sender.replace('/', "-")), "import", None)?;
            dated += 1;
        }
        if !here.is_empty() {
            kind = "whatsapp";
            let who = chat_name.as_deref().map(|n| format!("WhatsApp chat with {n}")).unwrap_or_else(|| "WhatsApp chat".into());
            facts.push(format!("{who}: the chat gives the sending time and sender of {} of {} items.", dated, here.len()));
        }
    }

    // Google Takeout: the JSON was applied while scanning.
    let (json_dates, json_places): (i64, i64) = c.query_row(
        "SELECT coalesce(sum(date_source = 'takeout'), 0), coalesce(sum(loc_source = 'takeout'), 0) FROM assets WHERE root_id = ?1 AND missing = 2",
        [r.root_id],
        |x| Ok((x.get(0)?, x.get(1)?)),
    )?;
    if json_dates + json_places > 0 && kind == "folder" {
        kind = "takeout";
        // Takeout times are instants: the place gives the zone.
        let zoned: Vec<(i64, i64, f64, f64)> = {
            let mut st = c.prepare(
                "SELECT id, taken_utc, lat, lon FROM assets
                 WHERE root_id = ?1 AND missing = 2 AND date_source = 'takeout' AND tz_offset IS NULL AND lat IS NOT NULL AND taken_utc IS NOT NULL",
            )?;
            st.query_map([r.root_id], |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?, x.get(3)?)))?.collect::<Result<_, _>>()?
        };
        for (a, utc, lat, lon) in zoned {
            if let Some(off) = roll_geo::tz::zone_at(lat, lon).and_then(|z| roll_geo::tz::offset_at_utc(&z, utc)) {
                let local = chrono::DateTime::from_timestamp_millis(utc + off as i64 * 60_000).unwrap_or_default().naive_utc();
                c.execute(
                    "UPDATE assets SET taken_local = ?2, tz_offset = ?3, problems = problems & ~?4 WHERE id = ?1",
                    params![a, local.format("%Y-%m-%dT%H:%M:%S").to_string(), off, problem::NO_OFFSET],
                )?;
            }
        }
        facts.push(format!("Google Takeout export: Google's data gives dates for {json_dates} and places for {json_places} items."));
        // Album folders: everything below "Google Photos" except the year folders.
        static YEAR_FOLDER: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?i)^(photos from \d{4}|fotos von \d{4}|untitled|archive|trash|bin)$").unwrap());
        let mut albums: HashMap<String, Vec<i64>> = HashMap::new();
        for s in &staged {
            let segs: Vec<&str> = s.dir.split('/').collect();
            let album = segs
                .iter()
                .position(|x| x.eq_ignore_ascii_case("google photos") || x.eq_ignore_ascii_case("google fotos"))
                .and_then(|i| segs.get(i + 1));
            if let Some(a) = album.filter(|a| !YEAR_FOLDER.is_match(a)) {
                albums.entry(a.to_string()).or_default().push(s.id);
            }
        }
        if !albums.is_empty() {
            let mut list: Vec<(&String, &Vec<i64>)> = albums.iter().collect();
            list.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
            facts.push(format!(
                "Albums, kept as Album tags: {}.",
                list.iter().map(|(n, ids)| format!("{n} ({})", ids.len())).collect::<Vec<_>>().join(", ")
            ));
            for (album, ids) in &albums {
                let tag = format!("Album/{}", album.replace('/', "-"));
                for a in ids {
                    // The same photo in a year folder gets the tag too.
                    let same: Vec<i64> = {
                        let mut st = c.prepare_cached(
                            "SELECT b.id FROM assets a JOIN assets b ON b.size = a.size AND b.quick_hash = a.quick_hash AND b.root_id = a.root_id AND b.missing = 2
                             WHERE a.id = ?1",
                        )?;
                        st.query_map([a], |x| x.get(0))?.collect::<Result<_, _>>()?
                    };
                    for b in same {
                        crate::tags::add(c, b, &tag, "import", None)?;
                    }
                }
            }
        }
    }
    c.execute("UPDATE imports SET kind = ?2, facts = ?3 WHERE id = ?1", params![id, kind, json!(facts).to_string()])?;
    Ok(())
}

/// Everything the Add photos steps show.
pub fn info(c: &Connection, id: i64, now: i64, place: &PlaceOf) -> anyhow::Result<ImportInfo> {
    let r = row(c, id)?;
    let all = items(c, r.root_id)?;
    let left: HashSet<i64> = r.left_out.iter().copied().collect();
    let (exact, twins) = exact_copies(c, r.root_id)?;
    let exact_set: HashSet<i64> = exact.iter().chain(&twins).copied().collect();
    let similar = similar_copies(c, &all, &exact_set)?;
    let pending: i64 =
        c.query_row("SELECT count(*) FROM assets WHERE root_id = ?1 AND missing = 2 AND thumb = 0", [r.root_id], |x| x.get(0))?;
    let dated: Vec<&str> = all.iter().filter_map(|i| i.local.as_deref()).collect();
    let located: Vec<(f64, f64)> = all.iter().filter_map(|i| i.lat.zip(i.lon)).collect();
    let common = if located.is_empty() { None } else { place(&located) };
    let custom = r.name.is_some();
    let name = match &r.name {
        Some(n) => n.clone(),
        None => folder_name(Path::new(&r.source))
            .unwrap_or_else(|| dated.first().map(|d| crate::events::name(d, common.as_deref())).unwrap_or_else(|| "New photos".into())),
    };
    let cameras_only: Vec<i64> = all.iter().filter(|i| !i.phone && i.local.is_some() && !left.contains(&i.id)).map(|i| i.id).collect();
    let smart = datefix::smart_offset(c, &cameras_only)?;
    let no_date = all
        .iter()
        .filter(|i| i.local.is_none() || i.problems & problem::NO_DATE != 0 || matches!(i.date_source.as_deref(), Some("file_time")))
        .count();
    Ok(ImportInfo {
        id: r.id as i32,
        root_id: r.root_id as i32,
        source: r.source.clone(),
        state: r.state.clone(),
        kind: r.kind.clone(),
        facts: r.facts.clone(),
        name,
        custom_name: custom,
        count: all.len() as i32,
        photos: all.iter().filter(|i| i.kind == 0).count() as i32,
        videos: all.iter().filter(|i| i.kind == 1).count() as i32,
        from_local: dated.iter().min().map(|s| s.to_string()).unwrap_or_default(),
        to_local: dated.iter().max().map(|s| s.to_string()).unwrap_or_default(),
        cameras: counts(all.iter().map(|i| i.camera.clone().unwrap_or_else(|| "Unknown camera".into()))),
        formats: counts(all.iter().map(|i| i.format.to_uppercase())),
        exact: exact.iter().map(|i| *i as i32).collect(),
        similar: similar.iter().map(|i| *i as i32).collect(),
        twins: twins.iter().map(|i| *i as i32).collect(),
        left_out: r.left_out.iter().map(|i| *i as i32).collect(),
        pending: pending as i32,
        date_sets: datefix::sets_in(c, now, Scope::Staged(r.root_id))?,
        smart,
        no_date: no_date as i32,
        no_place: all.iter().filter(|i| i.lat.is_none()).count() as i32,
        place: common,
        categories: counts(all.iter().map(|i| match (&i.category_name, i.category.as_deref()) {
            (Some(n), Some(k)) if k != "unsorted" => n.clone(),
            _ => "Not sorted yet".into(),
        })),
        dates_ok: r.dates_ok,
        places_ok: r.places_ok,
        ids: all.iter().map(|i| i.id as i32).collect(),
        added: match r.batch_id {
            Some(b) => {
                let mut st = c.prepare(
                    "SELECT asset_id FROM ops WHERE batch_id = ?1 AND kind IN ('copy', 'move') AND state = 'done'
                       AND json_extract(payload, '$.sidecar') IS NULL ORDER BY seq",
                )?;
                st.query_map([b], |x| x.get::<_, i32>(0))?.collect::<Result<_, _>>()?
            }
            None => Vec::new(),
        },
        batch_id: r.batch_id.map(|b| b as i32),
    })
}

pub fn set_name(c: &Connection, id: i64, name: Option<&str>) -> rusqlite::Result<()> {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    c.execute("UPDATE imports SET name = ?2 WHERE id = ?1", params![id, name])?;
    Ok(())
}

/// "Dates look right" and "Places look right".
pub fn mark(c: &Connection, id: i64, dates_ok: Option<bool>, places_ok: Option<bool>) -> rusqlite::Result<()> {
    if let Some(v) = dates_ok {
        c.execute("UPDATE imports SET dates_ok = ?2 WHERE id = ?1", params![id, v])?;
    }
    if let Some(v) = places_ok {
        c.execute("UPDATE imports SET places_ok = ?2 WHERE id = ?1", params![id, v])?;
    }
    Ok(())
}

pub fn leave_out(c: &Connection, id: i64, ids: &[i64], out: bool) -> rusqlite::Result<()> {
    let r = row(c, id)?;
    let mut set: Vec<i64> = r.left_out;
    if out {
        for i in ids {
            if !set.contains(i) {
                set.push(*i);
            }
        }
    } else {
        set.retain(|i| !ids.contains(i));
    }
    c.execute("UPDATE imports SET left_out = ?2 WHERE id = ?1", params![id, json!(set).to_string()])?;
    Ok(())
}

/// One file to bring in.
struct Move {
    asset: i64,
    sidecar: Option<i64>,
    from: String,
    to: String,
}

struct Computed {
    plan: ImportPlan,
    /// Items going to the event folder.
    event_ids: Vec<i64>,
    /// Of those, items without a decided category (they become Photos).
    unsorted: Vec<i64>,
    moves: Vec<Move>,
    mkdirs: Vec<String>,
    /// (asset, relative path in the archive) to write metadata into.
    writes: Vec<(i64, String)>,
}

fn compute(c: &Connection, id: i64, opts: &ImportOptions, name: &str, write: bool) -> anyhow::Result<Computed> {
    let r = row(c, id)?;
    anyhow::ensure!(r.state == "checking", "This import was already applied.");
    let (lib_id, lib_path): (i64, String) = match opts.root_id {
        Some(rid) => c.query_row("SELECT id, path FROM roots WHERE id = ?1 AND kind = 'library'", [rid], |x| Ok((x.get(0)?, x.get(1)?)))?,
        None => c
            .query_row("SELECT id, path FROM roots WHERE kind = 'library' ORDER BY id LIMIT 1", [], |x| Ok((x.get(0)?, x.get(1)?)))
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("Add a library folder first."))?,
    };
    let all = items(c, r.root_id)?;
    let mut skip: HashSet<i64> = r.left_out.iter().copied().collect();
    let (exact, twins) = exact_copies(c, r.root_id)?;
    skip.extend(&twins);
    if opts.skip_known {
        let exact: HashSet<i64> = exact.into_iter().collect();
        skip.extend(similar_copies(c, &all, &exact)?);
        skip.extend(exact);
    }
    let bringing: Vec<&Item> = all.iter().filter(|i| !skip.contains(&i.id)).collect();
    let cats = planner::load_categories(c)?;
    let sources: HashMap<i64, String> = {
        let mut st = c.prepare(
            "SELECT at.asset_id, substr(t.path, 8) FROM asset_tags at JOIN tags t ON t.id = at.tag_id
             JOIN assets a ON a.id = at.asset_id WHERE a.root_id = ?1 AND t.path LIKE 'Source/%'",
        )?;
        st.query_map([r.root_id], |x| Ok((x.get(0)?, x.get(1)?)))?.collect::<Result<_, _>>()?
    };
    let start = bringing.iter().filter_map(|i| i.local.clone()).min();
    let event_dir = planner::event_dir(name, start);

    // Not as an event: items still unsorted wait in one folder named after
    // the source, for Review and Organize to place later.
    let waiting =
        format!("Imported/{}", crate::template::safe_segment(&folder_name(Path::new(&r.source)).unwrap_or_else(|| name.to_string())));

    // Where each item goes: its category's folder when it is sorted as
    // something other than a photo (or always, when not added as an event),
    // else the event folder.
    let mut event_ids = Vec::new();
    let mut unsorted = Vec::new();
    let mut dirs: Vec<(&Item, String)> = Vec::new();
    for i in &bringing {
        let key = i.category.as_deref().unwrap_or("unsorted");
        let photo = key == "photos" || planner::parent_of(&cats, key) == Some("photos");
        let decided = i.review >= 2 && key != "unsorted";
        let dir = if decided && (!photo || !opts.as_event) {
            planner::category_dir(&cats, key, i.local.clone(), sources.get(&i.id).cloned(), i.camera.clone())
        } else {
            None
        };
        match dir {
            Some(d) => dirs.push((i, d)),
            None if !opts.as_event => dirs.push((i, waiting.clone())),
            None => {
                event_ids.push(i.id);
                if !(decided && photo) {
                    unsorted.push(i.id);
                }
                dirs.push((i, event_dir.clone()));
            }
        }
    }

    // Group members and sidecars travel with their item.
    let members: HashMap<i64, Vec<(i64, String, String)>> = {
        let mut st = c.prepare(
            "SELECT p.id, a.id, a.rel_path, a.name FROM assets p JOIN assets a ON a.group_id = p.group_id AND a.id != p.id
             WHERE p.root_id = ?1 AND p.missing = 2 AND p.group_id IS NOT NULL AND (p.group_role IS NULL OR p.group_role = 'primary')
               AND a.root_id = ?1 AND a.missing = 2",
        )?;
        let mut m: HashMap<i64, Vec<(i64, String, String)>> = HashMap::new();
        for x in st.query_map([r.root_id], |x| Ok((x.get::<_, i64>(0)?, x.get(1)?, x.get(2)?, x.get(3)?)))? {
            let (p, a, rel, n) = x?;
            m.entry(p).or_default().push((a, rel, n));
        }
        m
    };
    let own: HashMap<i64, (String, String, i64)> = {
        let mut st = c.prepare("SELECT id, rel_path, name, size FROM assets WHERE root_id = ?1 AND missing = 2")?;
        st.query_map([r.root_id], |x| Ok((x.get(0)?, (x.get(1)?, x.get(2)?, x.get(3)?))))?.collect::<Result<_, _>>()?
    };
    let sidecars: HashMap<i64, Vec<(i64, String, i64)>> = {
        // Takeout JSON stays behind when its values are written into the files.
        let mut st = c.prepare(&format!(
            "SELECT asset_id, id, rel_path, size FROM sidecars WHERE root_id = ?1 AND asset_id IS NOT NULL {}",
            if write { "AND kind != 'takeout'" } else { "" }
        ))?;
        let mut m: HashMap<i64, Vec<(i64, String, i64)>> = HashMap::new();
        for x in st.query_map([r.root_id], |x| Ok((x.get::<_, i64>(0)?, x.get(1)?, x.get(2)?, x.get(3)?)))? {
            let (a, sid, rel, size) = x?;
            m.entry(a).or_default().push((sid, rel, size));
        }
        m
    };
    let lib_root = PathBuf::from(&lib_path);
    let mut claimed: HashSet<String> = {
        let mut st =
            c.prepare("SELECT rel_path FROM assets WHERE root_id = ?1 UNION ALL SELECT rel_path FROM sidecars WHERE root_id = ?1")?;
        st.query_map([lib_id], |x| x.get::<_, String>(0))?.filter_map(Result::ok).map(|p| planner::case_key(&p)).collect()
    };
    let mut moves = Vec::new();
    let mut folders: HashMap<String, PlanFolder> = HashMap::new();
    let mut bytes = 0i64;
    let mut files = 0;
    let mut n_sidecars = 0;
    let mut writes = Vec::new();
    for (i, dir) in &dirs {
        let mut unit: Vec<(i64, String, String)> = vec![{
            let (rel, n, _) = own.get(&i.id).cloned().unwrap_or_default();
            (i.id, rel, n)
        }];
        unit.extend(members.get(&i.id).cloned().unwrap_or_default());
        let names = |k: u32| -> Vec<(i64, Option<i64>, String, String)> {
            let mut out = Vec::new();
            for (a, rel, n) in &unit {
                let name = if k > 1 { crate::template::numbered(n, k) } else { n.clone() };
                let to = if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") };
                out.push((*a, None, rel.clone(), to));
                for (sid, srel, _) in sidecars.get(a).into_iter().flatten() {
                    let sname = planner::sidecar_name(srel.rsplit('/').next().unwrap_or(srel), n, &name);
                    let sto = if dir.is_empty() { sname } else { format!("{dir}/{sname}") };
                    out.push((*a, Some(*sid), srel.clone(), sto));
                }
            }
            out
        };
        let mut k = 1;
        let chosen = loop {
            let list = names(k);
            let free = list.iter().all(|(_, _, _, to)| !claimed.contains(&planner::case_key(to)) && !join_rel(&lib_root, to).exists());
            if free || k > 999 {
                break list;
            }
            k += 1;
        };
        for (a, sid, from, to) in chosen {
            claimed.insert(planner::case_key(&to));
            match sid {
                Some(s) => {
                    n_sidecars += 1;
                    bytes += sidecars.get(&a).and_then(|v| v.iter().find(|x| x.0 == s)).map(|x| x.2).unwrap_or(0);
                }
                None => {
                    files += 1;
                    bytes += own.get(&a).map(|x| x.2).unwrap_or(0);
                    if write {
                        writes.push((a, to.clone()));
                    }
                }
            }
            moves.push(Move { asset: a, sidecar: sid, from, to });
        }
        let f = folders.entry(dir.clone()).or_insert_with(|| PlanFolder {
            root_id: lib_id as i32,
            path: dir.clone(),
            count: 0,
            samples: Vec::new(),
            new: !join_rel(&lib_root, dir).is_dir(),
        });
        f.count += 1;
        if f.samples.len() < 12 {
            f.samples.push(i.id as i32);
        }
    }
    let mut mkdirs: Vec<String> = Vec::new();
    for (dir, f) in &folders {
        if !f.new {
            continue;
        }
        let mut acc = String::new();
        for part in dir.split('/') {
            acc = if acc.is_empty() { part.to_string() } else { format!("{acc}/{part}") };
            if !join_rel(&lib_root, &acc).is_dir() && !mkdirs.contains(&acc) {
                mkdirs.push(acc.clone());
            }
        }
    }
    mkdirs.sort_by_key(|d| d.matches('/').count());
    let mut fl: Vec<PlanFolder> = folders.into_values().collect();
    fl.sort_by(|a, b| b.count.cmp(&a.count).then(a.path.cmp(&b.path)));
    let free = crate::fsops::free_space(&lib_root).map(|f| f as i64).unwrap_or(-1);
    Ok(Computed {
        plan: ImportPlan {
            root_id: lib_id as i32,
            root_path: lib_path,
            event: name.to_string(),
            files,
            sidecars: n_sidecars,
            skipped: (all.len() - bringing.len()) as i32,
            bytes,
            free,
            folders: fl,
            writes: write,
        },
        event_ids,
        unsorted,
        moves,
        mkdirs,
        writes,
    })
}

/// What applying would do.
pub fn preview(c: &Connection, id: i64, opts: &ImportOptions, name: &str, write: bool) -> anyhow::Result<ImportPlan> {
    Ok(compute(c, id, opts, name, write)?.plan)
}

/// Make the import batch: the event with its tags, Photos for unsorted
/// items in it, and the file operations for the journal. The batch is
/// `applying`; the caller runs it (and recovery finishes it after a crash).
pub fn begin(c: &Connection, id: i64, opts: &ImportOptions, name: &str, place: Option<&str>, write: bool) -> anyhow::Result<i64> {
    let r = row(c, id)?;
    anyhow::ensure!(r.dates_ok, "Check the dates first.");
    anyhow::ensure!(r.places_ok, "Check the places first.");
    let k = compute(c, id, opts, name, write)?;
    anyhow::ensure!(k.plan.files > 0, "Nothing to add: every item is left out or already in the archive.");
    if opts.mode == "copy" && k.plan.free >= 0 && k.plan.bytes > k.plan.free {
        anyhow::bail!(
            "Not enough space in {}: the copies need {} and {} is free.",
            k.plan.root_path,
            human(k.plan.bytes),
            human(k.plan.free)
        );
    }
    let verb = if opts.mode == "move" { "Move" } else { "Copy" };
    let label = if opts.as_event {
        format!("{verb} {} into {name}", plural(k.plan.files as usize))
    } else {
        format!("{verb} {} from {}", plural(k.plan.files as usize), folder_name(Path::new(&r.source)).unwrap_or_else(|| r.source.clone()))
    };
    c.execute(
        "INSERT INTO batches (kind, label, state, created_at, summary) VALUES ('import', ?1, 'applying', ?2, ?3)",
        params![label, now_ms(), serde_json::to_string(&k.plan)?],
    )?;
    let batch = c.last_insert_rowid();
    if !k.event_ids.is_empty() {
        let why = format!("Added as part of the event {name}.");
        for a in &k.unsorted {
            edits::set(
                c,
                Some(batch),
                *a,
                &Change {
                    field: "category",
                    value: edits::category_value(Some("photos"), "manual", None, 3),
                    source: "manual",
                    source_ref: None,
                    conf: None,
                    why: Some(why.clone()),
                },
            )?;
        }
        crate::events::create(c, batch, name, &k.event_ids, place)?;
    }
    let kind = if opts.mode == "move" { "move" } else { "copy" };
    let mut seq = 0;
    let mut st = c.prepare("INSERT INTO ops (batch_id, seq, kind, asset_id, src, dst, payload) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?;
    let root = k.plan.root_id;
    for d in &k.mkdirs {
        seq += 1;
        st.execute(params![batch, seq, "mkdir", None::<i64>, None::<String>, d, json!({"root": root}).to_string()])?;
    }
    for m in &k.moves {
        seq += 1;
        let payload = json!({"root": root, "from_root": r.root_id, "sidecar": m.sidecar});
        st.execute(params![batch, seq, kind, m.asset, m.from, m.to, payload.to_string()])?;
    }
    for (a, rel) in &k.writes {
        seq += 1;
        st.execute(params![batch, seq, "write_meta", a, rel, rel, json!({"root": root}).to_string()])?;
    }
    c.execute("UPDATE imports SET state = 'applied', batch_id = ?2, name = ?3 WHERE id = ?1", params![id, batch, name])?;
    Ok(batch)
}

/// Close an import. Before it is applied, the staging root and its items
/// are forgotten (the files stay where they are). After, only the items
/// left in the source are; the root stays so the batch can still be undone.
/// Returns the forgotten asset ids (for their thumbnails).
pub fn close(c: &Connection, id: i64) -> rusqlite::Result<Vec<i64>> {
    let r = row(c, id)?;
    let ids: Vec<i64> = {
        let mut st = c.prepare("SELECT id FROM assets WHERE root_id = ?1 AND missing != 0")?;
        st.query_map([r.root_id], |x| x.get(0))?.collect::<Result<_, _>>()?
    };
    if r.batch_id.is_none() {
        c.execute("DELETE FROM roots WHERE id = ?1", [r.root_id])?;
    } else {
        c.execute("DELETE FROM assets WHERE root_id = ?1 AND missing != 0", [r.root_id])?;
        c.execute("DELETE FROM sidecars WHERE root_id = ?1", [r.root_id])?;
        c.execute("UPDATE imports SET state = 'closed' WHERE id = ?1", [id])?;
    }
    Ok(ids)
}

fn plural(n: usize) -> String {
    if n == 1 { "1 item".into() } else { format!("{n} items") }
}

fn human(b: i64) -> String {
    let b = b as f64;
    if b >= 1e9 { format!("{:.1} GB", b / 1e9) } else { format!("{:.0} MB", (b / 1e6).max(1.0)) }
}
