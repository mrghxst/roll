//! M2 acceptance on a copy of the fixture library:
//!
//! - rule-decidable fixtures are classified and land in the right folders;
//! - killing the process during apply, then recovering, gives a consistent
//!   state with every file accounted for;
//! - undo of the moves restores a byte-identical tree;
//! - metadata writes read back, and undo restores the previous values with
//!   an unchanged ImageDataHash.
//!
//! Skipped when `fixtures-out/` has not been generated. The apply runs in a
//! child process (this test binary again) that the test kills mid-way.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use roll_core::actions::{self, EditAction};
use roll_core::dates::FixedTz;
use roll_core::db::Db;
use roll_core::edits::{self, Change};
use roll_core::journal::{self, Tools};
use roll_core::planner::{self, PlanOptions};
use roll_core::thumbs::{self, ThumbStore};
use roll_core::{classify, groups, rules, scan};
use roll_meta::exiftool::ExifToolPool;
use roll_meta::write::WriteMode;
use serde_json::{Value, json};

const CHILD_ENV: &str = "ROLL_ACCEPT_APPLY";
const TEST_NAME: &str = "organize_fixture_library";

fn noop(_: &str, _: usize, _: usize) {}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&p, &t);
        } else {
            std::fs::copy(&p, &t).unwrap();
        }
    }
}

/// Every file with its BLAKE3 hash, and every folder, relative to `root`.
#[derive(Debug, PartialEq)]
struct Tree {
    files: BTreeMap<String, String>,
    dirs: BTreeSet<String>,
}

fn tree(root: &Path) -> Tree {
    fn walk(root: &Path, dir: &Path, t: &mut Tree) {
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if e.file_type().unwrap().is_dir() {
                t.dirs.insert(rel);
                walk(root, &p, t);
            } else {
                t.files.insert(rel, blake3::hash(&std::fs::read(&p).unwrap()).to_hex().to_string());
            }
        }
    }
    let mut t = Tree { files: BTreeMap::new(), dirs: BTreeSet::new() };
    walk(root, root, &mut t);
    t
}

fn tools(pool: Option<&ExifToolPool>) -> Tools<'_> {
    Tools { exiftool: pool, write_mode: WriteMode::FileAndSidecar }
}

/// The child: apply the batch slowly, so the parent can kill it mid-way.
fn child_apply(data: &Path, batch: i64) {
    let db = Db::open(&data.join("library.db")).unwrap();
    journal::apply(&db, &tools(None), batch, &|_, _| {
        std::thread::sleep(Duration::from_millis(4));
        true
    })
    .unwrap();
}

#[cfg(windows)]
unsafe extern "system" fn on_fault(info: *mut windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS) -> i32 {
    let code = unsafe { (*(*info).ExceptionRecord).ExceptionCode };
    if code == windows_sys::Win32::Foundation::EXCEPTION_ACCESS_VIOLATION {
        let addr = unsafe { (*(*info).ExceptionRecord).ExceptionAddress };
        eprintln!(
            "ACCESS VIOLATION at {addr:?} on {:?}
{}",
            std::thread::current().name(),
            std::backtrace::Backtrace::force_capture()
        );
    }
    0
}

#[test]
fn organize_fixture_library() {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::AddVectoredExceptionHandler(1, Some(on_fault));
    }
    if let Ok(v) = std::env::var(CHILD_ENV) {
        let (data, batch) = v.rsplit_once('|').unwrap();
        child_apply(Path::new(data), batch.parse().unwrap());
        return;
    }
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out");
    let Ok(manifest) = std::fs::read_to_string(fixtures.join("manifest.json")) else {
        eprintln!("fixtures-out not found; skipping");
        return;
    };
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("library");
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    copy_dir(&fixtures.join("library"), &lib);
    let original = tree(&lib);

    // Index, group, thumbnail (junk rules need pixel statistics) and classify.
    let db = Db::open(&data.join("library.db")).unwrap();
    rules::seed(&db.write()).unwrap();
    db.write().execute("INSERT INTO roots (id, path, kind, added_at) VALUES (1, ?1, 'library', 0)", [lib.to_string_lossy()]).unwrap();
    eprintln!("checkpoint: scan");
    scan::scan_root(&db, 1, &lib, &FixedTz(60), false, &noop).unwrap();
    eprintln!("checkpoint: groups");
    groups::rebuild(&db, 1).unwrap();
    eprintln!("checkpoint: thumbs");
    let store = ThumbStore::memory().unwrap();
    let pool = rayon::ThreadPoolBuilder::new().build().unwrap();
    thumbs::run_pending(&db, &store, roll_media::ffmpeg::Ffmpeg::locate().as_ref(), &pool, &|_, _| true).unwrap();
    eprintln!("checkpoint: classify");
    classify::run(&db.write(), None).unwrap();
    eprintln!("checkpoint: classified");

    // Rule-decidable items are decided, with the expected category.
    let rows: HashMap<String, (i64, Option<String>, i64, Option<String>)> = {
        let c = db.read();
        let mut st = c
            .prepare("SELECT a.rel_path, a.id, c.key, a.review, a.taken_local FROM assets a LEFT JOIN categories c ON c.id = a.category_id")
            .unwrap();
        st.query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let decidable: Vec<(&str, &str)> = manifest["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["rule_decidable"] == true && i["kind"] != "sidecar")
        .filter_map(|i| Some((i["path"].as_str()?.strip_prefix("library/")?, i["category"].as_str()?)))
        .collect();
    assert!(decidable.len() > 100, "the manifest marks too few rule-decidable items");
    let mut wrong = Vec::new();
    for (rel, want) in &decidable {
        let (_, got, review, _) = rows.get(*rel).unwrap_or_else(|| panic!("{rel} was not indexed"));
        if got.as_deref() != Some(*want) || *review < 2 {
            wrong.push(format!("{rel}: want {want}, got {got:?} (review {review})"));
        }
    }
    assert!(wrong.is_empty(), "{} of {} rule-decidable items are wrong:\n{}", wrong.len(), decidable.len(), wrong.join("\n"));

    // Plan the moves only.
    let summary = planner::plan(&db.write(), &PlanOptions { write_metadata: false, ..Default::default() }).unwrap();
    assert_eq!(summary.writes, 0);
    assert!(summary.moved as usize >= decidable.len() / 2, "moved only {}", summary.moved);
    let batch = summary.batch_id as i64;
    let total_ops: i64 = db.read().query_row("SELECT count(*) FROM ops WHERE batch_id = ?1", [batch], |r| r.get(0)).unwrap();

    // Apply in a child process and kill it part way through.
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([TEST_NAME, "--exact", "--nocapture", "--test-threads", "1"])
        .env(CHILD_ENV, format!("{}|{batch}", data.display()))
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let started = Instant::now();
    let done_ops = || -> i64 {
        db.read().query_row("SELECT count(*) FROM ops WHERE batch_id = ?1 AND state = 'done'", [batch], |r| r.get(0)).unwrap_or(0)
    };
    while done_ops() < total_ops / 3 {
        assert!(started.elapsed() < Duration::from_secs(120), "the child made no progress");
        assert!(child.try_wait().unwrap().is_none(), "the child finished before it could be killed");
        std::thread::sleep(Duration::from_millis(5));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let at_kill = done_ops();
    assert!(at_kill < total_ops, "the apply finished before the kill");
    eprintln!("killed the apply after {at_kill} of {total_ops} operations");

    // Restart: recovery completes the batch.
    let db = Db::open(&data.join("library.db")).unwrap();
    assert_eq!(journal::recover(&db, &tools(None)).unwrap(), 1);
    let state: String = db.read().query_row("SELECT state FROM batches WHERE id = ?1", [batch], |r| r.get(0)).unwrap();
    assert_eq!(state, "applied");
    let not_done: i64 =
        db.read().query_row("SELECT count(*) FROM ops WHERE batch_id = ?1 AND state != 'done'", [batch], |r| r.get(0)).unwrap();
    assert_eq!(not_done, 0);

    // Consistent: every asset and sidecar is where the database says, and
    // no file was lost, duplicated or changed.
    let after = tree(&lib);
    let mut want: Vec<&String> = original.files.values().collect();
    let mut got: Vec<&String> = after.files.values().collect();
    want.sort();
    got.sort();
    assert_eq!(want, got, "file contents changed during apply");
    assert!(after.files.keys().all(|p| !p.contains(".roll-partial")));
    {
        let c = db.read();
        let mut st = c.prepare("SELECT rel_path FROM assets UNION ALL SELECT rel_path FROM sidecars").unwrap();
        let paths: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        for p in &paths {
            assert!(after.files.contains_key(p), "{p} is in the database but not on disk");
        }
    }

    // Rule-decidable items are in {year}/{Category}.
    let names: HashMap<String, String> = {
        let c = db.read();
        let mut st = c.prepare("SELECT key, name FROM categories").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
    };
    let mut misplaced = Vec::new();
    for (rel, cat) in &decidable {
        let (id, _, _, local) = &rows[*rel];
        let now: String = db.read().query_row("SELECT rel_path FROM assets WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        let year = local.as_deref().map(|l| l[..4].to_string()).unwrap_or_default();
        let dir = if *cat == "wallpapers" { names[*cat].clone() } else { format!("{year}/{}", names[*cat]) };
        if now.rsplit_once('/').map(|(d, _)| d) != Some(dir.as_str()) {
            misplaced.push(format!("{rel} -> {now}, expected in {dir}"));
        }
    }
    assert!(misplaced.is_empty(), "misplaced:\n{}", misplaced.join("\n"));

    // Undo restores the exact previous tree, folders included.
    let r = journal::undo(&db, &tools(None), batch, &|_, _| true).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(tree(&lib), original, "undo did not restore the tree");

    metadata_round_trip(&db, &lib);
}

/// Edit rating, tags, date and place on one file per format, apply, read
/// back with ExifTool, undo, and check values and image data.
fn metadata_round_trip(db: &Db, lib: &Path) {
    let Ok(pool) = ExifToolPool::locate() else {
        eprintln!("ExifTool not found; skipping metadata writes");
        return;
    };
    let picks: Vec<(i64, String, String)> = {
        let c = db.read();
        let mut st = c
            .prepare(
                "SELECT id, rel_path, format FROM assets WHERE id IN (
                   SELECT min(id) FROM assets WHERE format IN ('jpeg', 'heic', 'png', 'mov', 'mp4') AND problems & 1 = 0 GROUP BY format)",
            )
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect()
    };
    assert!(picks.len() >= 4, "too few formats to test: {picks:?}");
    let ids: Vec<i64> = picks.iter().map(|p| p.0).collect();
    let read = |rel: &str| -> Value {
        pool.with(|t| {
            t.read_json(
                &[&lib.join(rel)],
                &[
                    "-XMP-xmp:Rating",
                    "-XMP-dc:Subject",
                    "-ExifIFD:DateTimeOriginal",
                    "-XMP-exif:DateTimeOriginal",
                    "-Keys:CreationDate",
                    "-GPSLatitude",
                    "-GPSLongitude",
                    "-ImageDataHash",
                ],
            )
        })
        .unwrap()
        .remove(0)
    };
    let before: Vec<Value> = picks.iter().map(|p| read(&p.1)).collect();

    {
        let c = db.write();
        actions::apply(&c, &ids, &EditAction::Rating { rating: 4 }).unwrap();
        actions::apply(&c, &ids, &EditAction::AddTag { path: "Trips/Lake Garda".into() }).unwrap();
        for id in &ids {
            let set = |field: &'static str, value: Value| {
                edits::set(&c, None, *id, &Change { field, value, source: "manual", source_ref: None, conf: None, why: None }).unwrap();
            };
            set("date", json!({"utc": 1_565_600_400_000_i64, "local": "2019-08-12T11:00:00", "offset": 120, "source": "manual"}));
            set("location", json!({"lat": 45.6789, "lon": 10.6543, "alt": null, "acc": 10.0, "source": "manual"}));
        }
    }
    let summary = planner::plan(&db.write(), &PlanOptions { write_metadata: true, ids: ids.clone(), ..Default::default() }).unwrap();
    // Edits apply to whole groups, so a Live Photo partner is written too.
    // The moves undone above are planned again; files are read by path.
    assert!(summary.writes as usize >= ids.len(), "{} writes", summary.writes);
    let batch = summary.batch_id as i64;
    let r = journal::apply(db, &tools(Some(&pool)), batch, &|_, _| true).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);

    let now: Vec<String> =
        ids.iter().map(|id| db.read().query_row("SELECT rel_path FROM assets WHERE id = ?1", [id], |r| r.get(0)).unwrap()).collect();
    for ((_, _, format), (rel, old)) in picks.iter().zip(now.iter().zip(&before)) {
        let v = read(rel);
        let get = |k: &str| v.as_object().unwrap().iter().find(|(key, _)| key.ends_with(k)).map(|(_, v)| v.clone());
        assert_eq!(get(":Rating"), Some(json!(4)), "{format} rating: {v}");
        let subject = get(":Subject").unwrap_or_default().to_string();
        assert!(subject.contains("Lake Garda"), "{format} tags: {v}");
        let dated = v.as_object().unwrap().iter().any(|(k, x)| {
            (k.ends_with("DateTimeOriginal") || k.ends_with("CreationDate")) && x.to_string().contains("2019:08:12 11:00:00")
        });
        assert!(dated, "{format} date: {v}");
        let lat = get(":GPSLatitude").and_then(|x| x.as_f64()).unwrap_or_default();
        assert!((lat - 45.6789).abs() < 1e-4, "{format} latitude: {v}");
        assert_eq!(
            get(":ImageDataHash"),
            old.as_object().unwrap().iter().find(|(k, _)| k.ends_with(":ImageDataHash")).map(|(_, v)| v.clone())
        );
    }

    let r = journal::undo(db, &tools(Some(&pool)), batch, &|_, _| true).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    for ((rel, format), old) in picks.iter().map(|p| (&p.1, &p.2)).zip(&before) {
        let mut v = read(rel);
        let mut old = old.clone();
        for x in [&mut v, &mut old] {
            x.as_object_mut().unwrap().remove("SourceFile");
        }
        assert!(same(&v, &old), "{format}: undo did not restore the values\nnow:    {v}\nbefore: {old}");
    }
}

/// Equal, with numbers compared to 1e-9: GPS degrees are re-encoded as
/// DMS rationals on restore and can differ in the last digits.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() < 1e-9,
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(v, w)| same(v, w)),
        _ => a == b,
    }
}
