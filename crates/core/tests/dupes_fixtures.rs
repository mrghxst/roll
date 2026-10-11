//! Every duplicate the fixture manifest records (`duplicate_of`) ends up in
//! one group with its original, and the original is suggested to keep.
//! Skipped when `fixtures-out/` has not been generated.

use std::path::{Path, PathBuf};

use roll_core::dates::FixedTz;
use roll_core::db::Db;
use roll_core::thumbs::{self, ThumbStore};
use roll_core::{classify, dupes, groups, rules, scan};
use serde_json::Value;

fn noop(_: &str, _: usize, _: usize) {}

#[test]
fn fixture_duplicates_are_grouped() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out");
    let Ok(manifest) = std::fs::read_to_string(root.join("manifest.json")) else {
        eprintln!("fixtures-out not found; skipping");
        return;
    };
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    let lib = root.join("library");
    let db = Db::memory().unwrap();
    db.write().execute("INSERT INTO roots (id, path, kind, added_at) VALUES (1, ?1, 'library', 0)", [lib.to_string_lossy()]).unwrap();
    scan::scan_root(&db, 1, &lib, &FixedTz(120), false, &noop).unwrap();
    groups::rebuild(&db, 1).unwrap();
    let store = ThumbStore::memory().unwrap();
    let pool = rayon::ThreadPoolBuilder::new().build().unwrap();
    thumbs::run_pending(&db, &store, roll_media::ffmpeg::Ffmpeg::locate().as_ref(), &pool, &|_, _| true).unwrap();
    // As in the app: rules classify after thumbnails.
    rules::seed(&db.write()).unwrap();
    classify::run(&db.write(), None).unwrap();
    // What the hash job does: exact-copy candidates only.
    let todo: Vec<(i64, String)> = {
        let c = db.read();
        let mut st = c
            .prepare(
                "SELECT a.id, a.rel_path FROM assets a WHERE EXISTS
                   (SELECT 1 FROM assets b WHERE b.size = a.size AND b.quick_hash = a.quick_hash AND b.id != a.id)",
            )
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
    };
    for (id, rel) in todo {
        let h = roll_media::hash::blake3_file(&roll_core::paths::join_rel(Path::new(&lib), &rel)).unwrap();
        db.write().execute("UPDATE assets SET blake3 = ?2 WHERE id = ?1", rusqlite::params![id, h]).unwrap();
    }

    let found = dupes::groups(&db.read()).unwrap();
    let id_of = |rel: &str| -> i32 { db.read().query_row("SELECT id FROM assets WHERE rel_path = ?1", [rel], |r| r.get(0)).unwrap() };
    let mut missing = Vec::new();
    for it in manifest["items"].as_array().unwrap() {
        let (Some(path), Some(orig)) = (it["path"].as_str(), it["duplicate_of"].as_str()) else { continue };
        let (Some(rel), Some(orig_rel)) = (path.strip_prefix("library/"), orig.strip_prefix("library/")) else { continue };
        let (a, b) = (id_of(rel), id_of(orig_rel));
        match found.iter().find(|g| g.ids.contains(&a) && g.ids.contains(&b)) {
            None => missing.push(format!("{rel} is not grouped with {orig_rel}")),
            Some(g) if g.kind != "exact" && g.ids[0] != b => missing.push(format!("{rel}: keeps {} instead of the original", g.ids[0])),
            _ => {}
        }
    }
    assert!(missing.is_empty(), "{}\nfound: {found:#?}", missing.join("\n"));
    let bursts = found.iter().filter(|g| g.kind == "burst").count();
    assert_eq!(bursts, 1);
    // Nothing else in the fixtures is a duplicate.
    assert_eq!(found.iter().filter(|g| g.kind == "similar").count(), 0, "{found:#?}");
    eprintln!("{} groups: {:?}", found.len(), found.iter().map(|g| (&g.kind, g.ids.len())).collect::<Vec<_>>());
    for g in found.iter().filter(|g| g.kind == "similar") {
        let names: Vec<String> =
            g.ids.iter().map(|id| db.read().query_row("SELECT rel_path FROM assets WHERE id = ?1", [id], |r| r.get(0)).unwrap()).collect();
        eprintln!("similar: {names:?}");
    }
}
