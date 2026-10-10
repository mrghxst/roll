//! Scans the generated fixture library, groups it and makes every
//! thumbnail. Skipped when `fixtures-out/` has not been generated.

use std::path::PathBuf;

use roll_core::dates::FixedTz;
use roll_core::db::Db;
use roll_core::thumbs::{self, ThumbStore};
use roll_core::{groups, scan};
use serde_json::Value;

fn noop(_: &str, _: usize, _: usize) {}

#[test]
fn scan_group_and_thumbnail_fixtures() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out");
    let Ok(manifest) = std::fs::read_to_string(root.join("manifest.json")) else {
        eprintln!("fixtures-out not found; skipping");
        return;
    };
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    let lib = root.join("library");
    let db = Db::memory().unwrap();
    db.write().execute("INSERT INTO roots (id, path, added_at) VALUES (1, ?1, 0)", [lib.to_string_lossy()]).unwrap();

    let started = std::time::Instant::now();
    let (report, _) = scan::scan_root(&db, 1, &lib, &FixedTz(60), false, &noop).unwrap();
    let scan_time = started.elapsed();
    let items = manifest["items"].as_array().unwrap();
    let media: Vec<&Value> =
        items.iter().filter(|i| i["kind"] != "sidecar" && i["path"].as_str().unwrap().starts_with("library/")).collect();
    assert_eq!(report.added as usize, media.len(), "errors: {:?}", report.errors);
    let sidecars = items.iter().filter(|i| i["kind"] == "sidecar" && i["path"].as_str().unwrap().starts_with("library/")).count();
    assert_eq!(report.sidecars as usize, sidecars);

    let n_groups = groups::rebuild(&db, 1).unwrap();
    let kinds: Vec<(String, i64)> = {
        let c = db.read();
        let mut st = c.prepare("SELECT g.kind, count(*) FROM groups g GROUP BY g.kind ORDER BY g.kind").unwrap();
        st.query_map([], |r| Ok((r.get(0).unwrap(), r.get(1).unwrap()))).unwrap().map(Result::unwrap).collect()
    };
    let count = |k: &str| kinds.iter().find(|(n, _)| n == k).map(|(_, c)| *c).unwrap_or(0);
    let live = items.iter().filter(|i| i["group"].as_str().is_some_and(|g| g.starts_with("live:")) && i["kind"] == "video").count() as i64;
    assert_eq!(count("live"), live);
    assert_eq!(count("raw"), 4);
    assert_eq!(count("edit"), 2);
    assert_eq!(count("burst"), 1);
    assert_eq!(n_groups as i64, live + 7);

    let store = ThumbStore::memory().unwrap();
    let ff = roll_media::ffmpeg::Ffmpeg::locate();
    let pool = rayon::ThreadPoolBuilder::new().build().unwrap();
    let started = std::time::Instant::now();
    thumbs::run_pending(&db, &store, ff.as_ref(), &pool, &|_, _| true).unwrap();
    let thumb_time = started.elapsed();

    let c = db.read();
    let failed: Vec<String> = {
        let mut st = c.prepare("SELECT rel_path FROM assets WHERE thumb != 1").unwrap();
        st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    };
    assert!(failed.is_empty(), "thumbnail failures: {failed:?}");
    let broken: Vec<String> = {
        let mut st = c.prepare("SELECT rel_path FROM assets WHERE problems & 16 != 0").unwrap();
        st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(broken, vec!["Dump/IMG_9999.JPG".to_string()]);
    let (w, h): (i64, i64) =
        c.query_row("SELECT width, height FROM assets WHERE rel_path = 'Dump/IMG_7001.MOV'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert!(h > w, "rotated video should be portrait, got {w}x{h}");
    let heic: i64 = c.query_row("SELECT count(*) FROM assets WHERE format = 'heic' AND thumb = 1", [], |r| r.get(0)).unwrap();
    assert!(heic > 30);
    eprintln!("scan {:?}, thumbnails {:?} for {} files", scan_time, thumb_time, media.len());
}
