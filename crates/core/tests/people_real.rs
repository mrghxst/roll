//! People with the real faces pack on CC0 family photos: faces found,
//! grouped across photos, named, written into the files as MWG regions,
//! searchable by name, and the name undone.
//!
//! ```text
//! ROLL_PACKS=<abs path>/fixtures-out/packs cargo test --release -p roll-core --test people_real -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use roll_core::library::Library;
use roll_core::model::Query;
use roll_core::planner::PlanOptions;

fn wait_idle(lib: &Library) {
    let start = Instant::now();
    let mut quiet = 0;
    while quiet < 3 {
        let busy = lib.jobs.list().iter().any(|j| matches!(j.state.as_str(), "queued" | "running"));
        quiet = if busy { 0 } else { quiet + 1 };
        assert!(start.elapsed() < Duration::from_secs(600));
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
#[ignore = "needs the faces pack (ROLL_PACKS) and the CC0 set"]
fn finds_groups_names_and_writes_people() {
    let Some(packs) = std::env::var_os("ROLL_PACKS") else { return };
    assert!(PathBuf::from(&packs).join("models/faces/pack.json").exists(), "faces pack not in ROLL_PACKS");
    let set = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out/cc0/photos");
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Archive");
    std::fs::create_dir_all(&dir).unwrap();
    // Two photos of one family at Chinde Point, a mirrored third, and
    // other people.
    for n in ["010", "011", "012", "013", "014", "035"] {
        std::fs::copy(set.join(format!("photos-{n}.jpg")), dir.join(format!("photos-{n}.jpg"))).unwrap();
    }
    image::open(set.join("photos-012.jpg")).unwrap().fliph().save(dir.join("mirrored-012.jpg")).unwrap();

    let lib = Library::open(&tmp.path().join("data"), Arc::new(|_| {})).unwrap();
    lib.add_root(&dir).unwrap();
    lib.start();
    wait_idle(&lib);
    let faces: i64 = lib.db.read().query_row("SELECT count(*) FROM faces", [], |r| r.get(0)).unwrap();
    let people = lib.people().unwrap();
    println!("{faces} faces; people: {:?}", people.iter().map(|p| (p.id, p.faces, p.items)).collect::<Vec<_>>());
    assert!(faces >= 15, "faces in the family photos: {faces}");
    assert!(people.len() >= 2, "the Chinde Point family shows up as people");
    assert!(people.iter().all(|p| p.items == p.faces), "one face per person per photo");

    // Name the first person; their photos get the name in the file.
    let anna = &people[0];
    lib.name_person(anna.id as i64, Some("Anna Example")).unwrap();
    let named = lib.ids(&Query { people: vec![anna.id], ..Default::default() }).unwrap();
    assert_eq!(named.len(), anna.items as usize);
    let found = lib.ids(&Query { text: Some("anna".into()), ..Default::default() }).unwrap();
    assert!(named.iter().all(|i| found.contains(i)), "search finds her by name");

    let plan = lib.plan(&PlanOptions { write_metadata: true, ..Default::default() }).unwrap();
    assert!(plan.writes as usize >= named.len(), "{plan:?}");
    lib.apply(plan.batch_id as i64).unwrap();
    wait_idle(&lib);
    let pool = roll_meta::exiftool::ExifToolPool::locate().unwrap();
    for id in &named {
        let rel: String = lib.db.read().query_row("SELECT rel_path FROM assets WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        let file = dir.join(&rel);
        let v = pool.with(|t| t.read_json(&[file.as_path()], &["-XMP-mwg-rs:RegionInfo"])).unwrap().remove(0);
        let info = v.as_object().unwrap().iter().find(|(k, _)| k.ends_with(":RegionInfo")).map(|(_, v)| v.clone()).unwrap();
        let names: Vec<&str> = info["RegionList"].as_array().unwrap().iter().filter_map(|r| r["Name"].as_str()).collect();
        println!("{rel}: {names:?}");
        assert_eq!(names, vec!["Anna Example"]);
        let area = &info["RegionList"][0]["Area"];
        let x = area["X"].as_f64().unwrap();
        assert!((0.0..=1.0).contains(&x) && area["Unit"] == "normalized");
    }

    // Undo the name: she is unnamed again and the next write takes it out.
    let batch: i64 = lib.db.read().query_row("SELECT id FROM batches WHERE label = 'Named Anna Example'", [], |r| r.get(0)).unwrap();
    lib.undo(batch).unwrap();
    assert!(lib.people().unwrap().iter().all(|p| p.name.is_none()));
    let plan = lib.plan(&PlanOptions { write_metadata: true, ..Default::default() }).unwrap();
    lib.apply(plan.batch_id as i64).unwrap();
    wait_idle(&lib);
    let rel: String = lib.db.read().query_row("SELECT rel_path FROM assets WHERE id = ?1", [named[0]], |r| r.get(0)).unwrap();
    let v = pool.with(|t| t.read_json(&[dir.join(&rel).as_path()], &["-XMP-mwg-rs:RegionInfo"])).unwrap().remove(0);
    assert!(v.as_object().unwrap().keys().all(|k| !k.ends_with(":RegionInfo")), "the name left the file: {v}");
}
