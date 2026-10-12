//! M6 acceptance for Add photos (4.11): the fixture camera card (clock
//! 9 h 30 min behind, no GPS) is imported next to the Lisbon phone photos
//! already in the library. The smart offset fixes the dates, the phone
//! photos place it, and it lands in its own event folder with dates, places
//! and the event tag written into the copies. The source stays untouched,
//! and undo takes everything back.
//!
//! Skipped when `fixtures-out/` has not been generated.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::NaiveDateTime;
use roll_core::datefix::{Amount, DateFix};
use roll_core::import::ImportOptions;
use roll_core::library::Library;
use roll_core::model::Query;
use serde_json::Value;

fn hashes(root: &Path) -> BTreeMap<String, String> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            let rel = e.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            (rel, blake3::hash(&std::fs::read(e.path()).unwrap()).to_hex().to_string())
        })
        .collect()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), &t).unwrap();
        }
    }
}

/// Wait until no job is queued or running (twice in a row, so a job
/// enqueued by the one finishing is seen).
fn wait_idle(lib: &Library) {
    let start = Instant::now();
    let mut quiet = 0;
    while quiet < 3 {
        let busy = lib.jobs.list().iter().any(|j| matches!(j.state.as_str(), "queued" | "running"));
        quiet = if busy { 0 } else { quiet + 1 };
        assert!(start.elapsed() < Duration::from_secs(300), "jobs did not finish: {:?}", lib.jobs.list());
        std::thread::sleep(Duration::from_millis(150));
    }
    let failed: Vec<_> = lib.jobs.list().into_iter().filter(|j| j.state == "failed").collect();
    assert!(failed.is_empty(), "failed jobs: {failed:?}");
}

/// name, path, local time, offset, lat, lon, missing
type Imported = (String, String, String, Option<i64>, Option<f64>, Option<f64>, i64);

/// The truth for manifest items under `prefix`, by file name.
fn truth_under(manifest: &Value, prefix: &str) -> HashMap<String, Value> {
    manifest["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["path"].as_str().unwrap().starts_with(prefix) && i["kind"] == "photo")
        .map(|i| (i["path"].as_str().unwrap().rsplit('/').next().unwrap().to_string(), i.clone()))
        .collect()
}

/// (name, local time, offset, has GPS) of the staged items of an import.
fn staged(lib: &Library, root: i32) -> Vec<(String, String, Option<i64>, bool, i64)> {
    let c = lib.db.read();
    let mut st = c
        .prepare("SELECT name, taken_local, tz_offset, lat IS NOT NULL, id FROM assets WHERE root_id = ?1 AND missing = 2 ORDER BY name")
        .unwrap();
    st.query_map([root], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).unwrap().collect::<Result<_, _>>().unwrap()
}

fn local(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap()
}

#[test]
fn exports_bring_their_dates() {
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out");
    let Ok(manifest) = std::fs::read_to_string(fx.join("manifest.json")) else {
        eprintln!("fixtures-out not found; skipping");
        return;
    };
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    if !fx.join("whatsapp-android").exists() {
        eprintln!("fixtures-out predates the export fixtures; skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let (data, lib_dir) = (tmp.path().join("data"), tmp.path().join("Archive"));
    std::fs::create_dir_all(&lib_dir).unwrap();
    for d in ["whatsapp-android", "whatsapp-iphone", "takeout-export"] {
        copy_dir(&fx.join(d), &tmp.path().join(d));
    }
    let lib = Library::open(&data, Arc::new(|_| {})).unwrap();
    lib.add_root(&lib_dir).unwrap();
    lib.start();
    wait_idle(&lib);
    let opts = ImportOptions { as_event: false, ..Default::default() };
    let tags_of = |id: i64| roll_core::tags::of_asset(&lib.db.read(), id).unwrap();

    // WhatsApp, Android and iPhone: the chat gives the times and senders.
    for (dir, chat, n) in [("whatsapp-android", "WhatsApp chat with Alice", 6), ("whatsapp-iphone", "WhatsApp chat with Bob", 4)] {
        let truth = truth_under(&manifest, dir);
        let id = lib.import_start(&tmp.path().join(dir)).unwrap();
        wait_idle(&lib);
        let info = lib.import_info(id).unwrap();
        assert_eq!((info.kind.as_str(), info.count), ("whatsapp", n), "{info:?}");
        assert!(info.facts.iter().any(|f| f.starts_with(chat) && f.contains(&format!("{n} of {n}"))), "{:?}", info.facts);
        for (name, l, _, _, asset) in staged(&lib, info.root_id) {
            let t = &truth[&name];
            assert_eq!(l, t["true_local"].as_str().unwrap(), "{name}");
            let tags = tags_of(asset);
            for want in t["tags"].as_array().unwrap() {
                assert!(tags.contains(&want.as_str().unwrap().to_string()), "{name}: {tags:?}");
            }
        }
        lib.import_mark(id, Some(true), Some(true)).unwrap();
        let plan = lib.import_plan(id, &opts).unwrap();
        assert_eq!(plan.files, n);
        assert!(plan.folders.iter().all(|f| f.path.starts_with("Imported/")), "{:?}", plan.folders);
        lib.import_apply(id, &opts).unwrap();
        wait_idle(&lib);
        if lib.exiftool.is_some() {
            let (name, t) = truth.iter().next().unwrap();
            let file = walkdir::WalkDir::new(&lib_dir).into_iter().flatten().find(|e| e.file_name().to_string_lossy() == *name).unwrap();
            let m = roll_meta::MetaReader::default().read(file.path()).unwrap();
            assert_eq!(m.taken.unwrap().local, local(t["true_local"].as_str().unwrap()), "{name}");
            assert!(m.tags.iter().any(|x| x.starts_with("From/")), "{:?}", m.tags);
        }
    }

    // Takeout: JSON dates in the place's time zone, album repeats added once.
    let truth = truth_under(&manifest, "takeout-export");
    let id = lib.import_start(&tmp.path().join("takeout-export")).unwrap();
    wait_idle(&lib);
    let info = lib.import_info(id).unwrap();
    assert_eq!((info.kind.as_str(), info.count, info.twins.len()), ("takeout", 8, 3), "{info:?}");
    assert!(info.facts.iter().any(|f| f.contains("Rome trip (3)")), "{:?}", info.facts);
    for (name, l, off, gps, asset) in staged(&lib, info.root_id) {
        let t = &truth[&name];
        assert_eq!((l.as_str(), off, gps), (t["true_local"].as_str().unwrap(), Some(120), true), "{name}");
        let album = tags_of(asset).contains(&"Album/Rome trip".to_string());
        assert_eq!(album, !t["tags"].as_array().unwrap().is_empty(), "{name}");
    }
    lib.import_mark(id, Some(true), Some(true)).unwrap();
    let plan = lib.import_plan(id, &opts).unwrap();
    assert_eq!((plan.files, plan.skipped), (5, 3), "{plan:?}");
    lib.import_apply(id, &opts).unwrap();
    wait_idle(&lib);
    let copied: Vec<String> = walkdir::WalkDir::new(&lib_dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file() && e.file_name().to_string_lossy().starts_with("PXL_"))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(copied.iter().filter(|n| n.ends_with(".jpg")).count(), 5, "{copied:?}");
    if lib.exiftool.is_some() {
        assert!(!copied.iter().any(|n| n.ends_with(".json")), "the JSON stays behind once written: {copied:?}");
        let file =
            walkdir::WalkDir::new(&lib_dir).into_iter().flatten().find(|e| e.file_name().to_string_lossy().ends_with(".jpg")).unwrap();
        let name = file.file_name().to_string_lossy().into_owned();
        let m = roll_meta::MetaReader::default().read(file.path()).unwrap();
        let t = m.taken.unwrap();
        assert_eq!((t.local, t.offset_min), (local(truth[&name]["true_local"].as_str().unwrap()), Some(120)), "{name}");
        assert!(m.gps.is_some(), "{name}");
    }
}

#[test]
fn camera_card_becomes_an_event() {
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out");
    let Ok(manifest) = std::fs::read_to_string(fx.join("manifest.json")) else {
        eprintln!("fixtures-out not found; skipping");
        return;
    };
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    let truth: HashMap<String, (String, [f64; 2])> = manifest["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["path"].as_str().unwrap().starts_with("camera-card/"))
        .map(|i| {
            let name = i["path"].as_str().unwrap().rsplit('/').next().unwrap().to_string();
            let gps = i["true_gps"].as_array().unwrap();
            (name, (i["true_local"].as_str().unwrap().to_string(), [gps[0].as_f64().unwrap(), gps[1].as_f64().unwrap()]))
        })
        .collect();
    assert_eq!(truth.len(), 40);

    // The library: the Lisbon weekend's phone photos. The card: a copy.
    let tmp = tempfile::tempdir().unwrap();
    let (data, lib_dir, card) = (tmp.path().join("data"), tmp.path().join("Archive"), tmp.path().join("camera-card"));
    std::fs::create_dir_all(lib_dir.join("Dump")).unwrap();
    for k in 9101..=9116 {
        let n = format!("IMG_{k}.HEIC");
        std::fs::copy(fx.join("library/Dump").join(&n), lib_dir.join("Dump").join(&n)).unwrap();
    }
    copy_dir(&fx.join("camera-card"), &card);
    let before = hashes(&card);

    let lib = Library::open(&data, Arc::new(|_| {})).unwrap();
    lib.install_places(&fx.join("geonames")).unwrap();
    lib.add_root(&lib_dir).unwrap();
    lib.start();
    wait_idle(&lib);
    let archive = lib.ids(&Query::default()).unwrap().len();
    assert_eq!(archive, 16);

    // 1. Pick the source: indexed in place, outside the archive.
    let id = lib.import_start(&card).unwrap();
    wait_idle(&lib);
    assert_eq!(lib.ids(&Query::default()).unwrap().len(), 16, "staged items stay out of the archive");
    let info = lib.import_info(id).unwrap();
    assert_eq!((info.count, info.photos, info.pending), (40, 40, 0), "{info:?}");
    assert_eq!(info.cameras[0].label, "Canon EOS R6");
    assert!(info.exact.is_empty() && info.similar.is_empty(), "{:?} {:?}", info.exact, info.similar);
    assert_eq!(lib.ids(&Query { staged: Some(info.root_id), ..Default::default() }).unwrap().len(), 40);

    // 2. Dates: phone photos of the same moments give the offset.
    let smart = info.smart.clone().expect("a smart offset");
    let want = 9 * 3600 + 30 * 60;
    assert!(smart.matched >= 24, "{smart:?}");
    assert!((smart.seconds - want).abs() <= 240, "{smart:?}");
    assert_eq!(smart.offset_min, Some(60));
    assert!(!smart.pairs.is_empty());
    let fix = DateFix {
        ids: info.ids.iter().map(|i| *i as i64).collect(),
        mode: "smart".into(),
        reference: None,
        second: None,
        offset_min: smart.offset_min,
        amount: Some(Amount { seconds: smart.seconds as i32, ..Default::default() }),
        exact: None,
        camera_from: None,
        camera_to: None,
    };
    assert_eq!(lib.date_apply(&fix).unwrap(), 40);
    lib.import_mark(id, Some(true), None).unwrap();
    // Not before the places are checked.
    assert!(lib.import_apply(id, &ImportOptions::default()).unwrap_err().to_string().contains("places"));

    // 3. Places: from the phone photos taken at the same time.
    let groups = lib.import_places(id).unwrap();
    assert!(!groups.is_empty());
    assert!(groups.iter().all(|g| g.key.starts_with("import")));
    for g in &groups {
        lib.accept_places(&g.key).unwrap();
    }
    lib.import_mark(id, None, Some(true)).unwrap();
    let info = lib.import_info(id).unwrap();
    assert!(info.no_place <= 4, "{} without a place", info.no_place);
    assert!(info.name.starts_with("May 2022"), "{}", info.name);

    // 5. Preview: one event folder, nothing skipped.
    let plan = lib.import_plan(id, &ImportOptions::default()).unwrap();
    assert_eq!((plan.files, plan.skipped), (40, 0), "{plan:?}");
    assert_eq!(plan.folders.len(), 1);
    let dir = plan.folders[0].path.clone();
    assert_eq!(dir, format!("2022/2022-05 {}", info.name));

    // 6. Apply.
    let batch = lib.import_apply(id, &ImportOptions::default()).unwrap();
    wait_idle(&lib);
    assert_eq!(hashes(&card), before, "the source is untouched");
    assert_eq!(lib.ids(&Query::default()).unwrap().len(), 56);
    let c = lib.db.read();
    let mut st = c
        .prepare(
            "SELECT a.name, a.rel_path, a.taken_local, a.tz_offset, a.lat, a.lon, a.missing
             FROM assets a JOIN roots r ON r.id = a.root_id WHERE r.kind = 'library' AND a.make = 'Canon'",
        )
        .unwrap();
    let rows: Vec<Imported> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows.len(), 40);
    let mut placed = 0;
    for (name, rel, l, off, lat, lon, missing) in &rows {
        let (true_local, gps) = &truth[name];
        assert_eq!(*rel, format!("{dir}/{name}"));
        assert!(lib_dir.join(rel).exists());
        assert_eq!(*missing, 0);
        let err = (local(l) - local(true_local)).num_seconds().abs();
        assert!(err <= 240, "{name}: {l} vs {true_local}");
        assert_eq!(*off, Some(60));
        if let (Some(la), Some(lo)) = (lat, lon) {
            placed += 1;
            assert!(roll_geo::distance_m(*la, *lo, gps[0], gps[1]) < 3000.0, "{name} placed {la},{lo}, true {gps:?}");
        }
    }
    eprintln!("placed {placed} of 40");
    assert!(placed >= 36, "{placed} placed");
    let tag = format!("Event/{}", info.name);
    let tagged: i64 =
        c.query_row("SELECT count(*) FROM asset_tags at JOIN tags t ON t.id = at.tag_id WHERE t.path = ?1", [&tag], |r| r.get(0)).unwrap();
    assert_eq!(tagged, 40);
    let (event_state, n, immich): (String, i64, String) = c
        .query_row(
            "SELECT e.state, (SELECT count(*) FROM event_assets ea WHERE ea.event_id = e.id), e.immich FROM events e WHERE e.name = ?1",
            [&info.name],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((event_state.as_str(), n, immich.as_str()), ("confirmed", 40, "default"));
    // The next Immich sync makes it an album.
    let plan = roll_core::immich::plan(&c).unwrap();
    assert_eq!(plan.new_albums, 1);
    drop(st);
    drop(c);
    // The metadata is in the copies.
    if lib.exiftool.is_some() {
        let m = roll_meta::MetaReader::default().read(&lib_dir.join(&dir).join("IMG_0001.JPG")).unwrap();
        let t = m.taken.unwrap();
        assert!((t.local - local(&truth["IMG_0001.JPG"].0)).num_seconds().abs() <= 240, "{t:?}");
        assert_eq!(t.offset_min, Some(60));
        assert!(m.gps.is_some());
        assert!(m.tags.contains(&tag), "{:?}", m.tags);
    }

    // Undo: the copies go, the items are staged again, the event is gone.
    lib.undo(batch).unwrap();
    wait_idle(&lib);
    assert!(!lib_dir.join(&dir).exists());
    assert_eq!(lib.ids(&Query::default()).unwrap().len(), 16);
    assert_eq!(hashes(&card), before);
    let info = lib.import_info(id).unwrap();
    assert_eq!((info.state.as_str(), info.count), ("checking", 40));
    let events: i64 = lib.db.read().query_row("SELECT count(*) FROM events WHERE state = 'confirmed'", [], |r| r.get(0)).unwrap();
    assert_eq!(events, 0);

    // Closing forgets the staging set; the card keeps its files.
    lib.import_close(id).unwrap();
    assert!(lib.imports().unwrap().is_empty());
    let staged: i64 = lib.db.read().query_row("SELECT count(*) FROM assets WHERE missing = 2", [], |r| r.get(0)).unwrap();
    assert_eq!(staged, 0);
    assert_eq!(hashes(&card), before);
}
