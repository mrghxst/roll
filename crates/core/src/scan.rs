//! Indexing a root: walk, diff against the database, read new and changed
//! files in parallel, re-link moved files, apply sidecars, mark missing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;
use roll_meta::detect::{classify_name, extension, FileClass};
use roll_meta::{FileMeta, MediaKind, MetaReader};
use rusqlite::{params, Connection, OptionalExtension};
use walkdir::WalkDir;

use crate::dates::{self, TzGuess};
use crate::db::{now_ms, Db};
use crate::model::{problem, ScanReport};
use crate::paths::{join_rel, rel_dir, rel_string};
use crate::{sidecars, tags};

#[derive(Debug, Clone)]
pub struct Found {
    pub rel: String,
    pub size: i64,
    pub mtime: i64,
}

/// Progress callback: (phase, done, total).
pub type Progress<'a> = &'a (dyn Fn(&str, usize, usize) + Sync);

fn skip_dir(name: &str) -> bool {
    name.starts_with('.')
        || name.starts_with('$')
        || name.eq_ignore_ascii_case("System Volume Information")
        || name == "@eaDir"
        || name == "#recycle"
        || name == "#snapshot"
}

/// Cloud placeholders (OneDrive, iCloud) that would download when read.
#[cfg(windows)]
fn is_placeholder(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x4_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    meta.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

#[cfg(not(windows))]
fn is_placeholder(meta: &std::fs::Metadata) -> bool {
    // iCloud Drive keeps `.name.icloud` stubs, which are skipped by name.
    meta.len() == 0
}

pub fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub struct Walk {
    pub media: Vec<Found>,
    pub sidecars: Vec<Found>,
    pub placeholders: Vec<String>,
}

pub fn walk(root: &Path) -> Walk {
    let mut w = Walk { media: Vec::new(), sidecars: Vec::new(), placeholders: Vec::new() };
    let it = WalkDir::new(root).follow_links(false).into_iter().filter_entry(|e| e.depth() == 0 || !(e.file_type().is_dir() && skip_dir(&e.file_name().to_string_lossy())));
    for entry in it.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        let class = classify_name(&name);
        if class == FileClass::Ignored {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else { continue };
        let rel = rel_string(rel);
        let Ok(meta) = entry.metadata() else { continue };
        if is_placeholder(&meta) {
            w.placeholders.push(rel);
            continue;
        }
        let f = Found { rel, size: meta.len() as i64, mtime: mtime_ms(&meta) };
        match class {
            FileClass::Media(_) => w.media.push(f),
            FileClass::Sidecar => w.sidecars.push(f),
            FileClass::Ignored => {}
        }
    }
    w
}

pub struct ReadResult {
    pub found: Found,
    pub meta: FileMeta,
    pub quick_hash: String,
    pub error: Option<String>,
}

pub fn read_files(root: &Path, files: Vec<Found>, progress: Progress) -> Vec<ReadResult> {
    let total = files.len();
    let done = AtomicUsize::new(0);
    files
        .into_par_iter()
        .map_init(MetaReader::default, |reader, found| {
            let path = join_rel(root, &found.rel);
            let (meta, error) = match reader.read(&path) {
                Ok(m) => (m, None),
                Err(e) => {
                    let kind = match classify_name(&found.rel) {
                        FileClass::Media(k) => k,
                        _ => MediaKind::Photo,
                    };
                    (FileMeta { kind, format: extension(&found.rel), ..Default::default() }, Some(e.to_string()))
                }
            };
            let quick_hash = roll_media::hash::quick_hash(&path).unwrap_or_default();
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n % 64 == 0 || n == total {
                progress("Reading metadata", n, total);
            }
            ReadResult { found, meta, quick_hash, error }
        })
        .collect()
}

/// The derived columns of one asset, ready to insert or update.
pub struct Derived {
    pub kind: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub date: dates::DateDecision,
    pub signature: String,
    pub problems: i64,
    pub loc_source: Option<&'static str>,
    pub loc_acc: Option<f64>,
}

pub fn derive(r: &ReadResult, tz: &dyn TzGuess, now: i64) -> Derived {
    let m = &r.meta;
    let name = r.found.rel.rsplit('/').next().unwrap_or(&r.found.rel);
    let dir = rel_dir(&r.found.rel);
    let folders: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    let date = dates::decide(m, name, &folders, r.found.mtime, now, tz);
    let (w, h) = m.display_size().map(|(w, h)| (Some(w as i64), Some(h as i64))).unwrap_or((None, None));
    let mut problems = date.problems;
    if m.gps.is_none() {
        problems |= problem::NO_LOCATION;
    }
    if r.error.is_some() || (m.width.is_none() && m.kind == MediaKind::Photo) {
        problems |= problem::BROKEN;
    }
    let signature = format!(
        "{}:{}x{}:{}:{}",
        m.format,
        m.width.unwrap_or(0),
        m.height.unwrap_or(0),
        if m.has_camera() { "cam" } else { "nocam" },
        m.qtable.as_deref().or(m.icc_profile.as_deref()).or(m.codec.as_deref()).unwrap_or("-")
    );
    Derived {
        kind: if m.kind == MediaKind::Video { 1 } else { 0 },
        width: w,
        height: h,
        date,
        signature,
        problems,
        loc_source: m.gps.map(|_| "file"),
        loc_acc: m.gps.map(|g| g.accuracy_m.unwrap_or(10.0)),
    }
}

const ASSET_COLS: &str = "root_id, rel_path, dir, name, ext, size, mtime, quick_hash, kind, format, width, height, duration_ms, fps, has_audio, codec, orientation, rotation, taken_utc, taken_local, tz_offset, date_source, date_conf, lat, lon, alt, loc_acc, loc_source, make, model, serial, lens, software, screenshot, content_id, burst_id, icc, qtable, signature, has_xmp, has_makernotes, rating, description, problems, inbox, added_at";

fn insert(c: &Connection, root_id: i64, r: &ReadResult, d: &Derived, inbox: i64) -> rusqlite::Result<i64> {
    let m = &r.meta;
    let name = r.found.rel.rsplit('/').next().unwrap_or(&r.found.rel);
    let placeholders = (1..=46).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
    let sql = format!("INSERT INTO assets ({ASSET_COLS}) VALUES ({placeholders})");
    c.prepare_cached(&sql)?.execute(params![
        root_id,
        r.found.rel,
        rel_dir(&r.found.rel),
        name,
        extension(name),
        r.found.size,
        r.found.mtime,
        r.quick_hash,
        d.kind,
        m.format,
        d.width,
        d.height,
        m.duration_ms.map(|v| v as i64),
        m.fps,
        m.has_audio,
        m.codec,
        m.orientation,
        m.rotation,
        d.date.utc_ms,
        d.date.local_string(),
        d.date.offset,
        d.date.source,
        d.date.conf,
        m.gps.map(|g| g.lat),
        m.gps.map(|g| g.lon),
        m.gps.and_then(|g| g.alt),
        d.loc_acc,
        d.loc_source,
        m.make,
        m.model,
        m.serial,
        m.lens,
        m.software,
        m.screenshot_marker,
        m.content_id,
        m.burst_id,
        m.icc_profile,
        m.qtable,
        d.signature,
        m.has_xmp,
        m.has_maker_notes,
        m.rating.unwrap_or(0),
        m.description,
        d.problems,
        inbox,
        now_ms(),
    ])?;
    let id = c.last_insert_rowid();
    c.execute("INSERT OR REPLACE INTO asset_orig (asset_id, json) VALUES (?1, ?2)", params![id, serde_json::to_string(m).unwrap_or_default()])?;
    for t in &m.tags {
        tags::add(c, id, t, "file", None)?;
    }
    if m.tags.iter().any(|t| t.eq_ignore_ascii_case(tags::FAVORITE)) {
        c.execute("UPDATE assets SET favorite = 1 WHERE id = ?1", [id])?;
    }
    if m.tags.iter().any(|t| t.eq_ignore_ascii_case(tags::PRIVATE)) {
        c.execute("UPDATE assets SET private = 1 WHERE id = ?1", [id])?;
    }
    Ok(id)
}

/// A file changed outside Roll: refresh what the file says, but keep
/// fields the owner or Roll changed (they have history entries).
fn refresh(c: &Connection, id: i64, r: &ReadResult, d: &Derived) -> rusqlite::Result<()> {
    let m = &r.meta;
    c.execute(
        "UPDATE assets SET size = ?2, mtime = ?3, quick_hash = ?4, blake3 = NULL, format = ?5, width = ?6, height = ?7,
           duration_ms = ?8, fps = ?9, has_audio = ?10, codec = ?11, orientation = ?12, rotation = ?13,
           make = ?14, model = ?15, serial = ?16, lens = ?17, software = ?18, screenshot = ?19,
           content_id = ?20, burst_id = ?21, icc = ?22, qtable = ?23, signature = ?24, missing = 0, thumb = 0
         WHERE id = ?1",
        params![
            id,
            r.found.size,
            r.found.mtime,
            r.quick_hash,
            m.format,
            d.width,
            d.height,
            m.duration_ms.map(|v| v as i64),
            m.fps,
            m.has_audio,
            m.codec,
            m.orientation,
            m.rotation,
            m.make,
            m.model,
            m.serial,
            m.lens,
            m.software,
            m.screenshot_marker,
            m.content_id,
            m.burst_id,
            m.icc_profile,
            m.qtable,
            d.signature
        ],
    )?;
    let edited = |field: &str| -> rusqlite::Result<bool> {
        c.query_row("SELECT EXISTS(SELECT 1 FROM field_history WHERE asset_id = ?1 AND field = ?2)", params![id, field], |r| r.get(0))
    };
    if !edited("date")? {
        c.execute(
            "UPDATE assets SET taken_utc = ?2, taken_local = ?3, tz_offset = ?4, date_source = ?5, date_conf = ?6,
               problems = (problems & ~?7) | ?8 WHERE id = ?1",
            params![
                id,
                d.date.utc_ms,
                d.date.local_string(),
                d.date.offset,
                d.date.source,
                d.date.conf,
                problem::NO_DATE | problem::SUSPICIOUS_DATE | problem::NO_OFFSET,
                d.date.problems
            ],
        )?;
    }
    if !edited("location")? {
        c.execute(
            "UPDATE assets SET lat = ?2, lon = ?3, alt = ?4, loc_acc = ?5, loc_source = ?6,
               problems = CASE WHEN ?2 IS NULL THEN problems | ?7 ELSE problems & ~?7 END WHERE id = ?1",
            params![id, m.gps.map(|g| g.lat), m.gps.map(|g| g.lon), m.gps.and_then(|g| g.alt), d.loc_acc, d.loc_source, problem::NO_LOCATION],
        )?;
    }
    c.execute("INSERT OR REPLACE INTO asset_orig (asset_id, json) VALUES (?1, ?2)", params![id, serde_json::to_string(m).unwrap_or_default()])?;
    Ok(())
}

struct Existing {
    id: i64,
    size: i64,
    mtime: i64,
    quick_hash: String,
    blake3: Option<String>,
    missing: bool,
}

/// Scan one root. `inbox` marks new files for the Inbox (not on the first
/// scan of a root).
pub fn scan_root(db: &Db, root_id: i64, root: &Path, tz: &dyn TzGuess, inbox: bool, progress: Progress) -> anyhow::Result<(ScanReport, Vec<i64>)> {
    progress("Looking for files", 0, 0);
    let w = walk(root);
    let mut report = ScanReport { root_id: root_id as i32, seen: w.media.len() as i32, placeholders: w.placeholders.len() as i32, ..Default::default() };

    let existing: HashMap<String, Existing> = {
        let c = db.read();
        let mut st = c.prepare("SELECT id, rel_path, size, mtime, quick_hash, blake3, missing FROM assets WHERE root_id = ?1")?;
        let rows = st.query_map([root_id], |r| {
            Ok((r.get::<_, String>(1)?, Existing { id: r.get(0)?, size: r.get(2)?, mtime: r.get(3)?, quick_hash: r.get(4)?, blake3: r.get(5)?, missing: r.get::<_, i64>(6)? != 0 }))
        })?;
        rows.collect::<Result<_, _>>()?
    };

    let mut to_read = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for f in w.media {
        match existing.get(&f.rel) {
            Some(e) if e.size == f.size && e.mtime == f.mtime && !e.missing => {
                seen_ids.insert(e.id);
            }
            Some(e) => {
                seen_ids.insert(e.id);
                to_read.push(f);
            }
            None => to_read.push(f),
        }
    }
    let results = read_files(root, to_read, progress);

    // Assets in the database that were not found on disk: candidates for
    // re-linking to new files with the same size and quick hash.
    let mut gone: HashMap<(i64, String), Vec<(i64, Option<String>)>> = HashMap::new();
    for e in existing.values() {
        if !seen_ids.contains(&e.id) {
            gone.entry((e.size, e.quick_hash.clone())).or_default().push((e.id, e.blake3.clone()));
        }
    }

    let now = now_ms();
    let mut touched = Vec::new();
    let total = results.len();
    for (chunk_no, chunk) in results.chunks(500).enumerate() {
        db.tx(|tx| {
            for r in chunk {
                if let Some(e) = r.error.as_ref() {
                    report.errors.push(format!("{}: {e}", r.found.rel));
                }
                let d = derive(r, tz, now);
                if let Some(e) = existing.get(&r.found.rel) {
                    refresh(tx, e.id, r, &d)?;
                    report.updated += 1;
                    touched.push(e.id);
                    continue;
                }
                if let Some(id) = relink_candidate(&mut gone, root, r) {
                    tx.execute("UPDATE assets SET rel_path = ?2, dir = ?3, name = ?4, mtime = ?5, missing = 0 WHERE id = ?1", params![
                        id,
                        r.found.rel,
                        rel_dir(&r.found.rel),
                        r.found.rel.rsplit('/').next().unwrap_or_default(),
                        r.found.mtime
                    ])?;
                    seen_ids.insert(id);
                    report.relinked += 1;
                    continue;
                }
                let id = insert(tx, root_id, r, &d, if inbox { 1 } else { 0 })?;
                report.added += 1;
                touched.push(id);
            }
            Ok(())
        })?;
        progress("Saving", ((chunk_no + 1) * 500).min(total), total);
    }

    // Mark what is still gone as missing.
    let missing: Vec<i64> = existing.values().filter(|e| !seen_ids.contains(&e.id) && !e.missing).map(|e| e.id).collect();
    db.tx(|tx| {
        for id in &missing {
            tx.execute("UPDATE assets SET missing = 1 WHERE id = ?1", [id])?;
        }
        Ok(())
    })?;
    report.missing = missing.len() as i32;

    report.sidecars = sidecars::sync(db, root_id, root, &w.sidecars, tz, &mut touched)? as i32;
    Ok((report, touched))
}

fn relink_candidate(gone: &mut HashMap<(i64, String), Vec<(i64, Option<String>)>>, root: &Path, r: &ReadResult) -> Option<i64> {
    let list = gone.get_mut(&(r.found.size, r.quick_hash.clone()))?;
    let idx = match list.iter().position(|(_, b)| b.is_some()) {
        // Confirm with the full hash when the old file had one.
        Some(i) => {
            let full = roll_media::hash::blake3_file(&join_rel(root, &r.found.rel)).ok()?;
            if list[i].1.as_deref() != Some(full.as_str()) {
                return None;
            }
            i
        }
        None => 0,
    };
    Some(list.remove(idx).0)
}

pub fn root_path(c: &Connection, root_id: i64) -> rusqlite::Result<Option<PathBuf>> {
    c.query_row("SELECT path FROM roots WHERE id = ?1", [root_id], |r| r.get::<_, String>(0)).optional().map(|p| p.map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dates::FixedTz;

    fn noop(_: &str, _: usize, _: usize) {}

    #[test]
    fn walk_skips_system_files_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::create_dir_all(p.join(".hidden")).unwrap();
        std::fs::create_dir_all(p.join("$RECYCLE.BIN")).unwrap();
        std::fs::create_dir_all(p.join("2019/Photos")).unwrap();
        for f in ["2019/Photos/a.jpg", "2019/Photos/a.jpg.xmp", "2019/Thumbs.db", ".hidden/b.jpg", "$RECYCLE.BIN/c.jpg", "2019/._a.jpg", "notes.txt"] {
            std::fs::write(p.join(f), b"x").unwrap();
        }
        let w = walk(p);
        assert_eq!(w.media.iter().map(|f| f.rel.as_str()).collect::<Vec<_>>(), vec!["2019/Photos/a.jpg"]);
        assert_eq!(w.sidecars.len(), 1);
    }

    #[test]
    fn rescan_relinks_moved_files_and_marks_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::write(root.join("a/x.jpg"), vec![7u8; 1000]).unwrap();
        std::fs::write(root.join("a/y.jpg"), vec![8u8; 1000]).unwrap();
        let db = Db::memory().unwrap();
        db.write().execute("INSERT INTO roots (id, path, added_at) VALUES (1, ?1, 0)", [root.to_string_lossy()]).unwrap();
        let (r, _) = scan_root(&db, 1, root, &FixedTz(60), false, &noop).unwrap();
        assert_eq!(r.added, 2);
        let id: i64 = db.read().query_row("SELECT id FROM assets WHERE rel_path = 'a/x.jpg'", [], |r| r.get(0)).unwrap();
        db.write().execute("UPDATE assets SET rating = 4 WHERE id = ?1", [id]).unwrap();

        std::fs::create_dir_all(root.join("b")).unwrap();
        std::fs::rename(root.join("a/x.jpg"), root.join("b/x.jpg")).unwrap();
        std::fs::remove_file(root.join("a/y.jpg")).unwrap();
        let (r, _) = scan_root(&db, 1, root, &FixedTz(60), true, &noop).unwrap();
        assert_eq!((r.added, r.relinked, r.missing), (0, 1, 1));
        let (path, rating): (String, i64) = db.read().query_row("SELECT rel_path, rating FROM assets WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((path.as_str(), rating), ("b/x.jpg", 4));
        // Unchanged on the next scan.
        let (r, _) = scan_root(&db, 1, root, &FixedTz(60), true, &noop).unwrap();
        assert_eq!((r.added, r.updated, r.relinked, r.missing), (0, 0, 0, 0));
    }
}
