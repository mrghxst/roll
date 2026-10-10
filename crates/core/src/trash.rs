//! Roll's trash. Moving to the trash is an undoable edit and leaves files
//! untouched. Emptying sends the files, with their sidecars, to the
//! system Recycle Bin; Roll never deletes permanently.

use std::path::PathBuf;

use rusqlite::{Connection, params};
use serde_json::json;

use crate::edits::{self, Change};
use crate::paths::join_rel;

/// The ids plus every member of their groups.
pub fn with_groups(c: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<i64>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let list = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let mut st = c.prepare(&format!(
        "SELECT id FROM assets WHERE id IN ({list}) OR group_id IN (SELECT group_id FROM assets WHERE id IN ({list}) AND group_id IS NOT NULL)"
    ))?;
    st.query_map([], |r| r.get(0))?.collect()
}

/// Move items to Roll's trash (or back). Returns the batch.
pub fn set_trashed(c: &Connection, ids: &[i64], trashed: bool) -> rusqlite::Result<i64> {
    let all = with_groups(c, ids)?;
    let label =
        if trashed { format!("Move {} to the trash", plural(ids.len())) } else { format!("Restore {} from the trash", plural(ids.len())) };
    let batch = edits::begin_batch(c, if trashed { "trash" } else { "restore" }, &label)?;
    for id in all {
        edits::set(
            c,
            Some(batch),
            id,
            &Change { field: "trashed", value: json!(trashed), source: "manual", source_ref: None, conf: None, why: None },
        )?;
    }
    Ok(batch)
}

fn plural(n: usize) -> String {
    if n == 1 { "1 item".into() } else { format!("{n} items") }
}

/// Files of trashed assets and their sidecars.
fn trashed_files(c: &Connection, ids: Option<&[i64]>) -> rusqlite::Result<Vec<(i64, Vec<PathBuf>)>> {
    let filter = match ids {
        Some(ids) => format!("AND a.id IN ({})", ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")),
        None => String::new(),
    };
    let mut st = c.prepare(&format!(
        "SELECT a.id, r.path, a.rel_path FROM assets a JOIN roots r ON r.id = a.root_id WHERE a.trashed_at IS NOT NULL {filter}"
    ))?;
    let rows: Vec<(i64, String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, root, rel) in rows {
        let root = PathBuf::from(root);
        let mut files = vec![join_rel(&root, &rel)];
        let mut st = c.prepare_cached("SELECT rel_path FROM sidecars WHERE asset_id = ?1")?;
        for s in st.query_map([id], |r| r.get::<_, String>(0))? {
            files.push(join_rel(&root, &s?));
        }
        out.push((id, files));
    }
    Ok(out)
}

/// Send trashed items to the system Recycle Bin and forget them.
pub fn empty(c: &Connection, ids: Option<&[i64]>) -> anyhow::Result<usize> {
    let items = trashed_files(c, ids)?;
    let batch = edits::begin_batch(c, "empty_trash", &format!("Move {} to the Recycle Bin", plural(items.len())))?;
    let mut done = 0;
    for (id, files) in items {
        let existing: Vec<PathBuf> = files.into_iter().filter(|f| f.exists()).collect();
        if !existing.is_empty() {
            trash::delete_all(&existing)?;
        }
        c.execute("DELETE FROM sidecars WHERE asset_id = ?1", [id])?;
        c.execute("DELETE FROM assets WHERE id = ?1", [id])?;
        done += 1;
    }
    c.execute("UPDATE batches SET summary = ?2 WHERE id = ?1", params![batch, json!({"recycled": done}).to_string()])?;
    crate::tags::prune(c)?;
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    #[test]
    fn trash_and_undo() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let a = insert_asset(&c, "a.jpg");
        let b = set_trashed(&c, &[a], true).unwrap();
        let t: Option<i64> = c.query_row("SELECT trashed_at FROM assets WHERE id = ?1", [a], |r| r.get(0)).unwrap();
        assert!(t.is_some());
        edits::undo_batch(&c, b).unwrap();
        let t: Option<i64> = c.query_row("SELECT trashed_at FROM assets WHERE id = ?1", [a], |r| r.get(0)).unwrap();
        assert!(t.is_none());
    }
}
