//! Applying and undoing batches of file operations, safely across crashes.
//!
//! Each operation is marked `performing` (with anything needed to undo it)
//! before the file system changes, and `done` in the same transaction that
//! updates the database. On start, operations left `performing` are checked
//! against the file system and finished or rolled back, then the batch
//! continues.

use std::path::{Path, PathBuf};

use roll_meta::exiftool::ExifToolPool;
use roll_meta::write::{self, MetaEdit, Snapshot, WriteMode};
use roll_meta::{DateSource, Gps, MediaKind, Taken};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::db::{Db, now_ms};
use crate::fsops;
use crate::paths::{file_name, join_rel, rel_dir};
use crate::scan::mtime_ms;

pub struct Tools<'a> {
    pub exiftool: Option<&'a ExifToolPool>,
    pub write_mode: WriteMode,
}

#[derive(Debug, Clone)]
struct OpRow {
    id: i64,
    kind: String,
    asset: Option<i64>,
    src: Option<String>,
    dst: Option<String>,
    payload: Value,
    state: String,
}

fn load_ops(c: &Connection, batch: i64, states: &[&str], reverse: bool) -> rusqlite::Result<Vec<OpRow>> {
    let list = states.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(",");
    let order = if reverse { "DESC" } else { "ASC" };
    let mut st = c.prepare(&format!(
        "SELECT id, kind, asset_id, src, dst, payload, state FROM ops WHERE batch_id = ?1 AND state IN ({list}) ORDER BY seq {order}"
    ))?;
    let rows = st.query_map([batch], |r| {
        let p: Option<String> = r.get(5)?;
        Ok(OpRow {
            id: r.get(0)?,
            kind: r.get(1)?,
            asset: r.get(2)?,
            src: r.get(3)?,
            dst: r.get(4)?,
            payload: p.and_then(|p| serde_json::from_str(&p).ok()).unwrap_or(Value::Null),
            state: r.get(6)?,
        })
    })?;
    rows.collect()
}

fn root_of(c: &Connection, op: &OpRow) -> rusqlite::Result<PathBuf> {
    let id = op.payload.get("root").and_then(Value::as_i64).unwrap_or(0);
    c.query_row("SELECT path FROM roots WHERE id = ?1", [id], |r| r.get::<_, String>(0)).map(PathBuf::from)
}

fn set_state(c: &Connection, op: i64, state: &str, payload: Option<&Value>, error: Option<&str>) -> rusqlite::Result<()> {
    match payload {
        Some(p) => {
            c.execute("UPDATE ops SET state = ?2, payload = ?3, error = ?4 WHERE id = ?1", params![op, state, p.to_string(), error])?
        }
        None => c.execute("UPDATE ops SET state = ?2, error = ?3 WHERE id = ?1", params![op, state, error])?,
    };
    Ok(())
}

/// The staging root an import operation takes its file from.
fn from_root(c: &Connection, op: &OpRow) -> rusqlite::Result<Option<(i64, PathBuf)>> {
    let Some(id) = op.payload.get("from_root").and_then(Value::as_i64) else { return Ok(None) };
    let path: String = c.query_row("SELECT path FROM roots WHERE id = ?1", [id], |r| r.get(0))?;
    Ok(Some((id, PathBuf::from(path))))
}

/// Point an asset or sidecar at a path in another root: the archive after
/// an import (`presence` 0) or the staging root after its undo (2).
fn relocate_to(c: &Connection, op: &OpRow, root_id: i64, to: &str, root: &Path, presence: i64) -> rusqlite::Result<()> {
    if let Some(sid) = op.payload.get("sidecar").and_then(Value::as_i64) {
        c.execute("UPDATE sidecars SET root_id = ?2, rel_path = ?3 WHERE id = ?1", params![sid, root_id, to])?;
    } else if let Some(a) = op.asset {
        let mtime = std::fs::metadata(join_rel(root, to)).map(|m| mtime_ms(&m)).ok();
        c.execute(
            "UPDATE assets SET root_id = ?2, rel_path = ?3, dir = ?4, name = ?5, mtime = coalesce(?6, mtime), missing = ?7, inbox = 0 WHERE id = ?1",
            params![a, root_id, to, rel_dir(to), file_name(to), mtime, presence],
        )?;
    }
    Ok(())
}

/// Take back an imported copy: removed when it still matches the original,
/// otherwise (changed since) sent to the Recycle Bin.
fn take_back_copy(src: &Path, dst: &Path) -> anyhow::Result<()> {
    if !dst.exists() {
        return Ok(());
    }
    if src.exists() && same_content(src, dst) {
        std::fs::remove_file(dst)?;
    } else {
        trash::delete(dst).map_err(|e| anyhow::anyhow!("{}: {e}", dst.display()))?;
    }
    Ok(())
}

/// Point an asset or sidecar at its new path.
fn relocate(c: &Connection, op: &OpRow, to: &str, root: &Path) -> rusqlite::Result<()> {
    if let Some(sid) = op.payload.get("sidecar").and_then(Value::as_i64) {
        c.execute("UPDATE sidecars SET rel_path = ?2 WHERE id = ?1", params![sid, to])?;
    } else if let Some(a) = op.asset {
        let mtime = std::fs::metadata(join_rel(root, to)).map(|m| mtime_ms(&m)).ok();
        c.execute(
            "UPDATE assets SET rel_path = ?2, dir = ?3, name = ?4, mtime = coalesce(?5, mtime) WHERE id = ?1",
            params![a, to, rel_dir(to), file_name(to), mtime],
        )?;
    }
    Ok(())
}

/// The metadata Roll owns for an asset: what was edited, plus tags.
#[allow(clippy::type_complexity)]
pub fn meta_edit(c: &Connection, asset: i64) -> rusqlite::Result<(MetaEdit, MediaKind, String)> {
    let edited = |field: &str| -> rusqlite::Result<bool> {
        c.query_row("SELECT EXISTS(SELECT 1 FROM field_history WHERE asset_id = ?1 AND field = ?2)", params![asset, field], |r| r.get(0))
    };
    let (kind, format, local, offset, lat, lon, alt, acc, rating, description): (
        i64,
        String,
        Option<String>,
        Option<i64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        i64,
        Option<String>,
    ) = c.query_row(
        "SELECT kind, format, taken_local, tz_offset, lat, lon, alt, loc_acc, rating, description FROM assets WHERE id = ?1",
        [asset],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?)),
    )?;
    // Dates and places from Google Takeout JSON are not in the file yet.
    let (date_from_json, place_from_json): (bool, bool) = c.query_row(
        "SELECT coalesce(date_source = 'takeout', 0), coalesce(loc_source = 'takeout', 0) FROM assets WHERE id = ?1",
        [asset],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let tags = crate::tags::of_asset(c, asset)?;
    let mut e = MetaEdit::default();
    if !tags.is_empty() || edited("tags")? || edited("category")? || edited("favorite")? || edited("private")? {
        e.tags = Some(tags);
    }
    if edited("date")? || date_from_json {
        e.taken = local
            .as_deref()
            .and_then(|l| chrono::NaiveDateTime::parse_from_str(l, "%Y-%m-%dT%H:%M:%S").ok())
            .map(|l| Taken::new(l, offset.map(|o| o as i32), DateSource::ExifOriginal));
    }
    if edited("location")? || place_from_json {
        e.gps = Some(lat.zip(lon).map(|(lat, lon)| Gps { lat, lon, alt, accuracy_m: acc }));
    }
    if rating > 0 || edited("rating")? {
        e.rating = Some(rating as u8);
    }
    if edited("description")? {
        e.description = Some(description);
    }
    if edited("orientation")? {
        e.orientation =
            Some(c.query_row("SELECT coalesce(orientation, 1) FROM assets WHERE id = ?1", [asset], |r| r.get::<_, i64>(0))? as u16);
    }
    Ok((e, if kind == 1 { MediaKind::Video } else { MediaKind::Photo }, format))
}

/// After writing, record the file's new size and time, and its sidecar, so
/// the scanner does not treat Roll's own write as an outside change.
fn refresh_after_write(c: &Connection, asset: i64, root_id: i64, root: &Path, rel: &str) -> rusqlite::Result<()> {
    let path = join_rel(root, rel);
    if let Ok(m) = std::fs::metadata(&path) {
        let qh = roll_media::hash::quick_hash(&path).unwrap_or_default();
        c.execute(
            "UPDATE assets SET size = ?2, mtime = ?3, quick_hash = ?4, blake3 = NULL WHERE id = ?1",
            params![asset, m.len() as i64, mtime_ms(&m), qh],
        )?;
    }
    let side_rel = format!("{rel}.xmp");
    match std::fs::metadata(join_rel(root, &side_rel)) {
        Ok(m) => {
            c.execute(
                "INSERT INTO sidecars (root_id, rel_path, asset_id, kind, size, mtime) VALUES (?1, ?2, ?3, 'xmp', ?4, ?5)
                 ON CONFLICT(root_id, rel_path) DO UPDATE SET asset_id = ?3, size = ?4, mtime = ?5",
                params![root_id, side_rel, asset, m.len() as i64, mtime_ms(&m)],
            )?;
        }
        Err(_) => {
            c.execute("DELETE FROM sidecars WHERE root_id = ?1 AND rel_path = ?2", params![root_id, side_rel])?;
        }
    }
    Ok(())
}

/// Read-only files get their metadata written too: the flag is lifted for
/// the write and put back afterwards.
fn with_writable<T, E: Into<anyhow::Error>>(file: &Path, f: impl FnOnce() -> Result<T, E>) -> anyhow::Result<T> {
    let perm = std::fs::metadata(file)?.permissions();
    if !perm.readonly() {
        return f().map_err(Into::into);
    }
    let mut writable = perm.clone();
    #[allow(clippy::permissions_set_readonly_false)]
    writable.set_readonly(false);
    std::fs::set_permissions(file, writable)?;
    let out = f().map_err(Into::into);
    std::fs::set_permissions(file, perm)?;
    out
}

fn exiftool<'a>(tools: &Tools<'a>) -> anyhow::Result<&'a ExifToolPool> {
    tools.exiftool.ok_or_else(|| anyhow::anyhow!("ExifTool is needed to save metadata, and it was not found."))
}

/// Do one operation. The caller has marked it `performing`.
fn perform(db: &Db, tools: &Tools, op: &mut OpRow) -> anyhow::Result<()> {
    let root = root_of(&db.read(), op)?;
    let root_id = op.payload.get("root").and_then(Value::as_i64).unwrap_or(0);
    match op.kind.as_str() {
        "mkdir" => {
            std::fs::create_dir_all(join_rel(&root, op.dst.as_deref().unwrap_or_default()))?;
            db.tx(|tx| set_state(tx, op.id, "done", None, None))?;
        }
        "rmdir" => {
            let removed = fsops::remove_dir_if_empty(&join_rel(&root, op.src.as_deref().unwrap_or_default()));
            op.payload["removed"] = json!(removed);
            db.tx(|tx| set_state(tx, op.id, "done", Some(&op.payload), None))?;
        }
        "move" | "copy" if op.payload.get("from_root").is_some() => {
            let (_, src_root) = from_root(&db.read(), op)?.ok_or_else(|| anyhow::anyhow!("The import's source is gone."))?;
            let (from, to) = (op.src.clone().unwrap_or_default(), op.dst.clone().unwrap_or_default());
            let (src, dst) = (join_rel(&src_root, &from), join_rel(&root, &to));
            if op.kind == "copy" {
                let hash = fsops::copy_verified(&src, &dst)?;
                db.tx(|tx| {
                    relocate_to(tx, op, root_id, &to, &root, crate::model::presence::ARCHIVE)?;
                    if op.payload.get("sidecar").is_none()
                        && let Some(a) = op.asset
                    {
                        tx.execute("UPDATE assets SET blake3 = ?2 WHERE id = ?1", params![a, hash])?;
                    }
                    set_state(tx, op.id, "done", None, None)
                })?;
            } else {
                fsops::move_file(&src, &dst)?;
                db.tx(|tx| {
                    relocate_to(tx, op, root_id, &to, &root, crate::model::presence::ARCHIVE)?;
                    set_state(tx, op.id, "done", None, None)
                })?;
            }
        }
        "move" => {
            let (from, to) = (op.src.clone().unwrap_or_default(), op.dst.clone().unwrap_or_default());
            fsops::move_file(&join_rel(&root, &from), &join_rel(&root, &to))?;
            db.tx(|tx| {
                relocate(tx, op, &to, &root)?;
                set_state(tx, op.id, "done", None, None)
            })?;
        }
        "write_meta" => {
            let asset = op.asset.unwrap_or_default();
            let rel = op.src.clone().unwrap_or_default();
            let file = join_rel(&root, &rel);
            let pool = exiftool(tools)?;
            let (edit, kind, format) = meta_edit(&db.read(), asset)?;
            // The snapshot is stored before writing, so even an interrupted
            // write can be undone.
            let snap: Snapshot = match op.payload.get("snapshot") {
                Some(s) => serde_json::from_value(s.clone())?,
                None => {
                    let s = write::snapshot(pool, &file, kind, &format, &edit, tools.write_mode)?;
                    op.payload["snapshot"] = serde_json::to_value(&s)?;
                    op.payload["edit"] = serde_json::to_value(&edit)?;
                    db.tx(|tx| set_state(tx, op.id, "performing", Some(&op.payload), None))?;
                    s
                }
            };
            let edit: MetaEdit = op.payload.get("edit").cloned().map(serde_json::from_value).transpose()?.unwrap_or(edit);
            with_writable(&file, || write::perform(pool, &file, kind, &format, &edit, &snap))?;
            db.tx(|tx| {
                refresh_after_write(tx, asset, root_id, &root, &rel)?;
                tx.execute("UPDATE assets SET meta_dirty = 0 WHERE id = ?1", [asset])?;
                set_state(tx, op.id, "done", None, None)
            })?;
        }
        other => anyhow::bail!("Unknown operation {other}"),
    }
    Ok(())
}

/// Reverse one done operation. The caller has marked it `undoing`.
fn reverse(db: &Db, tools: &Tools, op: &OpRow) -> anyhow::Result<()> {
    let root = root_of(&db.read(), op)?;
    let root_id = op.payload.get("root").and_then(Value::as_i64).unwrap_or(0);
    match op.kind.as_str() {
        "mkdir" => {
            fsops::remove_dir_if_empty(&join_rel(&root, op.dst.as_deref().unwrap_or_default()));
        }
        "rmdir" => {
            if op.payload.get("removed").and_then(Value::as_bool).unwrap_or(false) {
                std::fs::create_dir_all(join_rel(&root, op.src.as_deref().unwrap_or_default()))?;
            }
        }
        "move" | "copy" if op.payload.get("from_root").is_some() => {
            let (src_id, src_root) = from_root(&db.read(), op)?.ok_or_else(|| anyhow::anyhow!("The import's source is gone."))?;
            let (from, to) = (op.src.clone().unwrap_or_default(), op.dst.clone().unwrap_or_default());
            let (src, dst) = (join_rel(&src_root, &from), join_rel(&root, &to));
            if op.kind == "copy" {
                take_back_copy(&src, &dst)?;
            } else if dst.exists() || !src.exists() {
                fsops::move_file(&dst, &src)?;
            }
            db.tx(|tx| relocate_to(tx, op, src_id, &from, &src_root, crate::model::presence::STAGED))?;
        }
        "move" => {
            let (from, to) = (op.src.clone().unwrap_or_default(), op.dst.clone().unwrap_or_default());
            let (src, dst) = (join_rel(&root, &from), join_rel(&root, &to));
            if dst.exists() || !src.exists() {
                fsops::move_file(&dst, &src)?;
            }
            db.tx(|tx| relocate(tx, op, &from, &root))?;
        }
        "write_meta" => {
            let asset = op.asset.unwrap_or_default();
            let rel = op.src.clone().unwrap_or_default();
            let snap: Snapshot = serde_json::from_value(op.payload.get("snapshot").cloned().unwrap_or(Value::Null))?;
            let file = join_rel(&root, &rel);
            with_writable(&file, || write::restore(exiftool(tools)?, &file, &snap).map_err(anyhow::Error::from))?;
            db.tx(|tx| {
                refresh_after_write(tx, asset, root_id, &root, &rel)?;
                tx.execute("UPDATE assets SET meta_dirty = 1 WHERE id = ?1", [asset])
            })?;
        }
        _ => {}
    }
    db.tx(|tx| set_state(tx, op.id, "undone", None, None))?;
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub done: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

/// Progress: (done, total). Return false to stop between operations; the
/// batch stays `applying` and continues on the next start.
pub type Check<'a> = &'a dyn Fn(usize, usize) -> bool;

/// Apply a planned batch.
pub fn apply(db: &Db, tools: &Tools, batch: i64, check: Check) -> anyhow::Result<Report> {
    db.write().execute("UPDATE batches SET state = 'applying' WHERE id = ?1 AND state IN ('planned', 'applying')", [batch])?;
    let ops = load_ops(&db.read(), batch, &["pending"], false)?;
    let total = ops.len();
    let mut report = Report::default();
    for (i, mut op) in ops.into_iter().enumerate() {
        if !check(i, total) {
            return Ok(report);
        }
        set_state(&db.write(), op.id, "performing", None, None)?;
        match perform(db, tools, &mut op) {
            Ok(()) => report.done += 1,
            Err(e) => {
                report.failed += 1;
                let msg = format!("{}: {e}", op.src.as_deref().or(op.dst.as_deref()).unwrap_or(&op.kind));
                set_state(&db.write(), op.id, "failed", None, Some(&e.to_string()))?;
                report.errors.push(msg);
            }
        }
    }
    let state = if report.done == 0 && report.failed > 0 { "failed" } else { "applied" };
    db.write().execute("UPDATE batches SET state = ?2, applied_at = ?3 WHERE id = ?1", params![batch, state, now_ms()])?;
    Ok(report)
}

/// Undo an applied batch: file operations in reverse, or field edits.
pub fn undo(db: &Db, tools: &Tools, batch: i64, check: Check) -> anyhow::Result<Report> {
    let kind: Option<String> = db.read().query_row("SELECT kind FROM batches WHERE id = ?1", [batch], |r| r.get(0)).optional()?;
    let mut report = Report::default();
    if !matches!(kind.as_deref(), Some("organize" | "import")) {
        db.tx(|tx| {
            report.done = crate::edits::undo_batch(tx, batch)?.len();
            crate::events::undo(tx, batch)?;
            tx.execute("UPDATE batches SET state = 'undone', undone_at = ?2 WHERE id = ?1", params![batch, now_ms()])?;
            Ok(())
        })?;
        return Ok(report);
    }
    db.write().execute("UPDATE batches SET state = 'undoing' WHERE id = ?1", [batch])?;
    let ops = load_ops(&db.read(), batch, &["done"], true)?;
    let total = ops.len();
    for (i, op) in ops.into_iter().enumerate() {
        if !check(i, total) {
            return Ok(report);
        }
        set_state(&db.write(), op.id, "undoing", None, None)?;
        match reverse(db, tools, &op) {
            Ok(()) => report.done += 1,
            Err(e) => {
                report.failed += 1;
                report.errors.push(format!("{}: {e}", op.dst.as_deref().or(op.src.as_deref()).unwrap_or(&op.kind)));
                set_state(&db.write(), op.id, "done", None, Some(&e.to_string()))?;
            }
        }
    }
    let state = if report.failed > 0 { "applied" } else { "undone" };
    db.tx(|tx| {
        tx.execute("UPDATE batches SET state = ?2, undone_at = ?3 WHERE id = ?1", params![batch, state, now_ms()])?;
        // An import also tagged and named its event in the same batch.
        if state == "undone" && kind.as_deref() == Some("import") {
            crate::edits::undo_batch(tx, batch)?;
            crate::events::undo(tx, batch)?;
            tx.execute("UPDATE imports SET state = 'checking', batch_id = NULL WHERE batch_id = ?1", [batch])?;
        }
        Ok(())
    })?;
    Ok(report)
}

/// Bring interrupted operations in line with the file system. Returns how
/// many batches were found unfinished; they are then completed.
pub fn recover(db: &Db, tools: &Tools) -> anyhow::Result<usize> {
    let batches: Vec<(i64, String)> = {
        let c = db.read();
        let mut st = c.prepare("SELECT id, state FROM batches WHERE state IN ('applying', 'undoing') ORDER BY id")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    for (batch, state) in &batches {
        let applying = state == "applying";
        let stuck = load_ops(&db.read(), *batch, &["performing", "undoing"], false)?;
        for mut op in stuck {
            let root = root_of(&db.read(), &op)?;
            let root_id = op.payload.get("root").and_then(Value::as_i64).unwrap_or(0);
            match op.kind.as_str() {
                "move" | "copy" if op.payload.get("from_root").is_some() => {
                    let Some((src_id, src_root)) = from_root(&db.read(), &op)? else {
                        set_state(&db.write(), op.id, "failed", None, Some("The import's source is gone."))?;
                        continue;
                    };
                    let (from, to) = (op.src.clone().unwrap_or_default(), op.dst.clone().unwrap_or_default());
                    let (src, dst) = (join_rel(&src_root, &from), join_rel(&root, &to));
                    let _ = std::fs::remove_file(fsops::partial_path(&dst));
                    let _ = std::fs::remove_file(fsops::partial_path(&src));
                    let at_dst = dst.exists() && (!src.exists() || same_content(&src, &dst));
                    if applying {
                        if op.kind == "move" && at_dst && src.exists() {
                            // A finished cross-volume copy: the source can go.
                            std::fs::remove_file(&src)?;
                        }
                    } else if op.kind == "copy" {
                        take_back_copy(&src, &dst)?;
                    } else if dst.exists() {
                        if src.exists() && same_content(&src, &dst) {
                            std::fs::remove_file(&dst)?;
                        } else if !src.exists() {
                            fsops::move_file(&dst, &src)?;
                        }
                    }
                    let done = at_dst && applying;
                    db.tx(|tx| {
                        if done {
                            relocate_to(tx, &op, root_id, &to, &root, crate::model::presence::ARCHIVE)?;
                        } else {
                            relocate_to(tx, &op, src_id, &from, &src_root, crate::model::presence::STAGED)?;
                        }
                        set_state(
                            tx,
                            op.id,
                            if done {
                                "done"
                            } else if applying {
                                "pending"
                            } else {
                                "undone"
                            },
                            None,
                            None,
                        )
                    })?;
                }
                "move" => {
                    let (from, to) = (op.src.clone().unwrap_or_default(), op.dst.clone().unwrap_or_default());
                    let (src, dst) = (join_rel(&root, &from), join_rel(&root, &to));
                    let _ = std::fs::remove_file(fsops::partial_path(if op.state == "performing" { &dst } else { &src }));
                    // Where is the file now?
                    let at_dst = dst.exists() && (!src.exists() || same_content(&src, &dst));
                    if at_dst && src.exists() {
                        // A finished cross-volume copy: the source can go.
                        std::fs::remove_file(&src)?;
                    }
                    db.tx(|tx| {
                        relocate(tx, &op, if at_dst { &to } else { &from }, &root)?;
                        set_state(
                            tx,
                            op.id,
                            if at_dst {
                                "done"
                            } else if applying {
                                "pending"
                            } else {
                                "undone"
                            },
                            None,
                            None,
                        )
                    })?;
                }
                "mkdir" | "rmdir" => {
                    let next = if op.state == "performing" { "pending" } else { "done" };
                    set_state(&db.write(), op.id, next, None, None)?;
                }
                "write_meta" if op.state == "performing" => {
                    // Writing again is harmless; the stored snapshot still
                    // describes the state before the first attempt.
                    if op.payload.get("snapshot").is_some() {
                        if let Err(e) = perform(db, tools, &mut op) {
                            set_state(&db.write(), op.id, "failed", None, Some(&e.to_string()))?;
                        }
                    } else {
                        set_state(&db.write(), op.id, "pending", None, None)?;
                    }
                }
                "write_meta" => {
                    // Undo interrupted: restoring again is harmless too.
                    if let Err(e) = reverse(db, tools, &op) {
                        set_state(&db.write(), op.id, "done", None, Some(&e.to_string()))?;
                    }
                }
                _ => set_state(&db.write(), op.id, "pending", None, None)?,
            }
        }
        if applying {
            apply(db, tools, *batch, &|_, _| true)?;
        } else {
            undo(db, tools, *batch, &|_, _| true)?;
        }
    }
    Ok(batches.len())
}

fn same_content(a: &Path, b: &Path) -> bool {
    match (roll_media::hash::blake3_file(a), roll_media::hash::blake3_file(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dates::FixedTz;
    use crate::planner::{self, PlanOptions};
    use std::collections::BTreeMap;

    fn noop(_: &str, _: usize, _: usize) {}

    /// Relative path → BLAKE3 for every file under a folder.
    fn manifest(root: &Path) -> BTreeMap<String, String> {
        walkdir::WalkDir::new(root)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| (crate::paths::rel_string(e.path().strip_prefix(root).unwrap()), roll_media::hash::blake3_file(e.path()).unwrap()))
            .collect()
    }

    fn dirs(root: &Path) -> Vec<String> {
        let mut v: Vec<String> = walkdir::WalkDir::new(root)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_dir())
            .map(|e| crate::paths::rel_string(e.path().strip_prefix(root).unwrap()))
            .collect();
        v.sort();
        v
    }

    /// A small library: two screenshots by name, a WhatsApp image with a
    /// sidecar, an undecided image, a case-only name collision.
    fn library() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for (rel, bytes) in [
            ("Dump/Screenshot_20200301-101500.png", vec![1u8; 2000]),
            ("Dump/Screenshot_20200302-101500.png", vec![2u8; 2000]),
            ("Dump/Old/screenshot_20200301-101500.PNG", vec![3u8; 2000]),
            ("Dump/IMG-20200102-WA0004.jpg", vec![4u8; 2000]),
            ("Dump/IMG-20200102-WA0004.jpg.xmp", b"<x:xmpmeta xmlns:x='adobe:ns:meta/'></x:xmpmeta>".to_vec()),
            ("Dump/unknown.jpg", vec![5u8; 2000]),
            ("Dump/notes.txt", b"keep me".to_vec()),
        ] {
            let p = join_rel(root, rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        }
        let db = Db::memory().unwrap();
        db.write().execute("INSERT INTO roots (id, path, added_at) VALUES (1, ?1, 0)", [root.to_string_lossy()]).unwrap();
        scan::scan_root(&db, 1, root, &FixedTz(60), false, &noop).unwrap();
        crate::rules::seed(&db.write()).unwrap();
        crate::classify::run(&db.write(), None).unwrap();
        // The WhatsApp image is decided by hand.
        let id: i64 = db.read().query_row("SELECT id FROM assets WHERE name LIKE 'IMG-%'", [], |r| r.get(0)).unwrap();
        crate::edits::set(
            &db.write(),
            None,
            id,
            &crate::edits::Change {
                field: "category",
                value: crate::edits::category_value(Some("memes"), "manual", None, 3),
                source: "manual",
                source_ref: None,
                conf: None,
                why: None,
            },
        )
        .unwrap();
        (dir, db)
    }

    use crate::scan;

    fn tools() -> Tools<'static> {
        Tools { exiftool: None, write_mode: WriteMode::SidecarOnly }
    }

    fn plan_moves_only(db: &Db) -> planner::PlanSummary {
        planner::plan(&db.write(), &PlanOptions { write_metadata: false, ..Default::default() }).unwrap()
    }

    #[test]
    fn apply_and_undo_restore_the_exact_tree() {
        let (dir, db) = library();
        let root = dir.path();
        let (before, before_dirs) = (manifest(root), dirs(root));
        let s = plan_moves_only(&db);
        assert_eq!(s.undecided, 1, "unknown.jpg has no decision");
        assert_eq!(s.moved, 4);
        // Case-only twins collide only on case-insensitive file systems.
        let insensitive = cfg!(any(windows, target_os = "macos"));
        assert_eq!(s.conflicts.len(), usize::from(insensitive), "the case-only duplicate name gets a number");
        apply(&db, &tools(), s.batch_id as i64, &|_, _| true).unwrap();
        let after = manifest(root);
        assert!(after.contains_key("2020/Screenshots/Screenshot_20200302-101500.png"), "{after:?}");
        assert!(after.contains_key("2020/Memes/IMG-20200102-WA0004.jpg.xmp"), "sidecar moved along: {after:?}");
        // The case-only twin gets a number instead of overwriting.
        let twins = after.keys().filter(|k| k.to_lowercase().starts_with("2020/screenshots/screenshot_20200301-101500")).count();
        assert_eq!(twins, 2, "{after:?}");
        assert_eq!(after.keys().any(|k| k.contains(" (2).")), insensitive, "{after:?}");
        assert!(after.contains_key("Dump/notes.txt") && after.contains_key("Dump/unknown.jpg"));
        assert!(!root.join("Dump/Old").exists(), "emptied folder removed");
        // The database follows the files.
        let paths: Vec<String> =
            db.read().prepare("SELECT rel_path FROM assets").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        for p in &paths {
            assert!(join_rel(root, p).exists(), "{p}");
        }
        // Nothing left to do.
        assert_eq!(plan_moves_only(&db).moved, 0);

        undo(&db, &tools(), s.batch_id as i64, &|_, _| true).unwrap();
        assert_eq!(manifest(root), before);
        assert_eq!(dirs(root), before_dirs);
    }

    #[test]
    fn crash_during_apply_recovers() {
        let (dir, db) = library();
        let root = dir.path();
        let s = plan_moves_only(&db);
        let batch = s.batch_id as i64;
        // Stop after two operations, then simulate a crash in the middle of
        // the third: the file moved, but the database was not updated.
        apply(&db, &tools(), batch, &|i, _| i < 2).unwrap();
        let op: OpRow = load_ops(&db.read(), batch, &["pending"], false).unwrap().into_iter().find(|o| o.kind == "move").unwrap();
        set_state(&db.write(), op.id, "performing", None, None).unwrap();
        fsops::move_file(&join_rel(root, op.src.as_deref().unwrap()), &join_rel(root, op.dst.as_deref().unwrap())).unwrap();

        assert_eq!(recover(&db, &tools()).unwrap(), 1);
        let state: String = db.read().query_row("SELECT state FROM batches WHERE id = ?1", [batch], |r| r.get(0)).unwrap();
        assert_eq!(state, "applied");
        let paths: Vec<String> =
            db.read().prepare("SELECT rel_path FROM assets").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        for p in &paths {
            assert!(join_rel(root, p).exists(), "{p} missing after recovery");
        }
        let pending: i64 =
            db.read().query_row("SELECT count(*) FROM ops WHERE batch_id = ?1 AND state != 'done'", [batch], |r| r.get(0)).unwrap();
        assert_eq!(pending, 0);
    }

    #[test]
    fn crash_before_the_move_retries_it() {
        let (dir, db) = library();
        let root = dir.path();
        let s = plan_moves_only(&db);
        let batch = s.batch_id as i64;
        let before = manifest(root);
        let op = load_ops(&db.read(), batch, &["pending"], false).unwrap().into_iter().find(|o| o.kind == "move").unwrap();
        db.write().execute("UPDATE batches SET state = 'applying' WHERE id = ?1", [batch]).unwrap();
        set_state(&db.write(), op.id, "performing", None, None).unwrap();
        recover(&db, &tools()).unwrap();
        undo(&db, &tools(), batch, &|_, _| true).unwrap();
        assert_eq!(manifest(root), before);
    }
}
