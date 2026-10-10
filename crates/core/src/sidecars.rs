//! Sidecar files: `<file>.<ext>.xmp` and `<file>.xmp`, Apple `.aae` edit
//! files and Google Takeout `.json`. They are matched to their media file,
//! and their values are applied when they are new or changed.

use std::collections::HashMap;
use std::path::Path;

use roll_meta::{takeout, xmp, DateSource};
use rusqlite::{params, Connection, OptionalExtension};

use crate::dates::TzGuess;
use crate::db::Db;
use crate::model::problem;
use crate::paths::{file_name, join_rel, rel_dir};
use crate::scan::Found;
use crate::tags;

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name)
}

/// The media file a sidecar belongs to, among the names in its folder.
pub fn owner<'a>(sidecar: &str, names: &'a [(i64, String)]) -> Option<&'a (i64, String)> {
    let lower = sidecar.to_lowercase();
    if let Some(base) = lower.strip_suffix(".xmp") {
        // `IMG_1.HEIC.xmp` first, then `IMG_1.xmp` (prefer a photo).
        if let Some(n) = names.iter().find(|(_, n)| n.to_lowercase() == base) {
            return Some(n);
        }
        let mut same_stem: Vec<&(i64, String)> = names.iter().filter(|(_, n)| stem(n).to_lowercase() == base).collect();
        same_stem.sort_by_key(|(_, n)| roll_meta::detect::classify_name(n) != roll_meta::detect::FileClass::Media(roll_meta::MediaKind::Photo));
        return same_stem.into_iter().next();
    }
    if let Some(base) = lower.strip_suffix(".aae") {
        return names.iter().find(|(_, n)| stem(n).to_lowercase() == base);
    }
    if lower.ends_with(".json") {
        return names.iter().find(|(_, n)| takeout::json_matches(sidecar, n));
    }
    None
}

fn kind_of(name: &str) -> &'static str {
    let l = name.to_lowercase();
    if l.ends_with(".xmp") {
        "xmp"
    } else if l.ends_with(".aae") {
        "aae"
    } else {
        "takeout"
    }
}

/// Record sidecars and apply new or changed ones. Returns how many matched.
pub fn sync(db: &Db, root_id: i64, root: &Path, found: &[Found], tz: &dyn TzGuess, touched: &mut Vec<i64>) -> anyhow::Result<usize> {
    let mut by_dir: HashMap<String, Vec<(i64, String)>> = HashMap::new();
    let mut matched = 0;
    db.tx(|tx| {
        for f in found {
            let dir = rel_dir(&f.rel);
            if !by_dir.contains_key(&dir) {
                let mut st = tx.prepare_cached("SELECT id, name FROM assets WHERE root_id = ?1 AND dir = ?2 AND missing = 0")?;
                let names = st.query_map(params![root_id, dir], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
                by_dir.insert(dir.clone(), names);
            }
            let names = &by_dir[&dir];
            let name = file_name(&f.rel);
            let Some((asset, _)) = owner(name, names) else { continue };
            matched += 1;
            let prev: Option<(i64, i64)> = tx
                .query_row("SELECT size, mtime FROM sidecars WHERE root_id = ?1 AND rel_path = ?2", params![root_id, f.rel], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            let kind = kind_of(name);
            tx.execute(
                "INSERT INTO sidecars (root_id, rel_path, asset_id, kind, size, mtime) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(root_id, rel_path) DO UPDATE SET asset_id = ?3, size = ?5, mtime = ?6",
                params![root_id, f.rel, asset, kind, f.size, f.mtime],
            )?;
            if prev == Some((f.size, f.mtime)) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(join_rel(root, &f.rel)) else { continue };
            let applied = match kind {
                "xmp" => apply_xmp(tx, *asset, &xmp::parse(&text))?,
                "takeout" => takeout::parse(&text).map(|t| apply_takeout(tx, *asset, &t, tz)).transpose()?.unwrap_or(false),
                _ => false,
            };
            if applied {
                touched.push(*asset);
            }
        }
        Ok(())
    })?;
    Ok(matched)
}

fn edited(c: &Connection, asset: i64, field: &str) -> rusqlite::Result<bool> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM field_history WHERE asset_id = ?1 AND field = ?2)", params![asset, field], |r| r.get(0))
}

/// Sidecar values win over the file's own, as in Immich.
pub fn apply_xmp(c: &Connection, asset: i64, x: &xmp::XmpData) -> rusqlite::Result<bool> {
    for t in x.tags() {
        tags::add(c, asset, &t, "sidecar", None)?;
        if t.eq_ignore_ascii_case(tags::FAVORITE) {
            c.execute("UPDATE assets SET favorite = 1 WHERE id = ?1", [asset])?;
        }
    }
    if let (Some(t), false) = (x.taken(), edited(c, asset, "date")?) {
        let off = t.offset_min;
        c.execute(
            "UPDATE assets SET taken_local = ?2, tz_offset = ?3,
               taken_utc = CASE WHEN ?3 IS NULL THEN taken_utc ELSE ?4 END,
               date_source = ?5, date_conf = 0.95, problems = problems & ~?6 WHERE id = ?1",
            params![asset, t.local.format("%Y-%m-%dT%H:%M:%S").to_string(), off, t.utc_ms(), DateSource::Sidecar.as_str(), problem::NO_DATE],
        )?;
    }
    if let (Some(g), false) = (x.gps(), edited(c, asset, "location")?) {
        c.execute(
            "UPDATE assets SET lat = ?2, lon = ?3, alt = ?4, loc_acc = ?5, loc_source = 'sidecar', problems = problems & ~?6 WHERE id = ?1",
            params![asset, g.lat, g.lon, g.alt, g.accuracy_m.unwrap_or(10.0), problem::NO_LOCATION],
        )?;
    }
    if let Some(r) = x.rating() {
        c.execute("UPDATE assets SET rating = ?2 WHERE id = ?1", params![asset, r])?;
    }
    if let Some(d) = &x.description {
        c.execute("UPDATE assets SET description = ?2 WHERE id = ?1", params![asset, d])?;
    }
    Ok(true)
}

/// Takeout JSON fills what the file lacks.
pub fn apply_takeout(c: &Connection, asset: i64, t: &takeout::Takeout, tz: &dyn TzGuess) -> rusqlite::Result<bool> {
    if let Some(taken) = &t.taken {
        // Takeout times are UTC instants; the zone is guessed.
        let utc = taken.utc_ms();
        let local = taken.local + chrono::Duration::minutes(tz.offset_for_utc(utc) as i64);
        c.execute(
            "UPDATE assets SET taken_utc = ?2, taken_local = ?3, tz_offset = NULL, date_source = 'takeout', date_conf = 0.9,
               problems = (problems & ~?4) | ?5
             WHERE id = ?1 AND date_source IN ('file_time', 'folder', 'filename')",
            params![asset, utc, local.format("%Y-%m-%dT%H:%M:%S").to_string(), problem::NO_DATE, problem::NO_OFFSET],
        )?;
    }
    if let Some(g) = &t.gps {
        c.execute(
            "UPDATE assets SET lat = ?2, lon = ?3, alt = ?4, loc_acc = 10, loc_source = 'takeout', problems = problems & ~?5
             WHERE id = ?1 AND lat IS NULL",
            params![asset, g.lat, g.lon, g.alt, problem::NO_LOCATION],
        )?;
    }
    if let Some(d) = &t.description {
        c.execute("UPDATE assets SET description = ?2 WHERE id = ?1 AND description IS NULL", params![asset, d])?;
    }
    if t.favorite {
        c.execute("UPDATE assets SET favorite = 1 WHERE id = ?1", [asset])?;
        tags::add(c, asset, tags::FAVORITE, "takeout", None)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners() {
        let names = vec![(1, "IMG_1.HEIC".to_string()), (2, "IMG_1.MOV".to_string()), (3, "IMG_E1.JPG".to_string()), (4, "PXL_1.jpg".to_string())];
        assert_eq!(owner("IMG_1.HEIC.xmp", &names).unwrap().0, 1);
        assert_eq!(owner("IMG_1.xmp", &names).unwrap().0, 1);
        assert_eq!(owner("IMG_1.AAE", &names).unwrap().0, 1);
        assert_eq!(owner("PXL_1.jpg.json", &names).unwrap().0, 4);
        assert!(owner("other.json", &names).is_none());
    }
}
