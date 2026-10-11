//! M3 acceptance on the fixture library:
//! - the NIKON D750 set with a clock 1 year 2 hours ahead is found and fixed
//!   with one reference, every photo landing on its true time;
//! - the 500 Sony photos of the Dolomites hike get their places from one
//!   GPX file in one accepted suggestion, close to the true positions;
//! - the Canon on home time is flagged against phone photos' zone.
//!
//! Skipped when `fixtures-out/` has not been generated.

use std::collections::HashMap;
use std::path::PathBuf;

use roll_core::datefix::{self, DateFix, DateRef};
use roll_core::dates::FixedTz;
use roll_core::db::Db;
use roll_core::{groups, places, scan};
use serde_json::Value;

fn noop(_: &str, _: usize, _: usize) {}

#[test]
fn dates_and_places_on_fixtures() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out");
    let Ok(manifest) = std::fs::read_to_string(root.join("manifest.json")) else {
        eprintln!("fixtures-out not found; skipping");
        return;
    };
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    let items: HashMap<String, &Value> = manifest["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| Some((i["path"].as_str()?.strip_prefix("library/")?.to_string(), i)))
        .collect();
    let lib = root.join("library");
    let db = Db::memory().unwrap();
    db.write().execute("INSERT INTO roots (id, path, kind, added_at) VALUES (1, ?1, 'library', 0)", [lib.to_string_lossy()]).unwrap();
    // The owner's computer is in Switzerland: guesses use UTC+2 in summer.
    scan::scan_root(&db, 1, &lib, &FixedTz(120), false, &noop).unwrap();
    groups::rebuild(&db, 1).unwrap();

    let id_of = |rel: &str| -> i64 { db.read().query_row("SELECT id FROM assets WHERE rel_path = ?1", [rel], |r| r.get(0)).unwrap() };
    let local_of = |id: i64| -> String { db.read().query_row("SELECT taken_local FROM assets WHERE id = ?1", [id], |r| r.get(0)).unwrap() };

    // The wrong clock is found, with a reason.
    let sets = datefix::sets(&db.read(), i64::MAX).unwrap();
    let nikon = sets.iter().find(|s| s.label.contains("D750")).expect("the NIKON set is listed");
    assert!(nikon.reasons.iter().any(|r| r.contains("The folder says 2019")), "{:?}", nikon.reasons);
    let wrong: Vec<(&String, &&Value)> = items
        .iter()
        .filter(|(_, v)| v["camera"] == "NIKON D750" && v["flags"].as_array().unwrap().iter().any(|f| f == "wrong_clock"))
        .collect();
    assert!(wrong.len() >= 20, "{} wrong-clock NIKON photos", wrong.len());

    // One reference with its true time fixes the set; the owner's actions
    // are: open Dates (the set is selected), pick the photo, type the time,
    // choose the zone, apply.
    let reference = "Old events/Italy 2019/DSC_0001.JPG";
    let truth = items[reference];
    let fix = DateFix {
        ids: nikon.ids.iter().map(|i| *i as i64).collect(),
        mode: "shift".into(),
        reference: Some(DateRef {
            asset_id: id_of(reference),
            local: truth["true_local"].as_str().unwrap().into(),
            offset_min: truth["true_offset_min"].as_i64().map(|o| o as i32),
        }),
        second: None,
        offset_min: None,
        amount: None,
        exact: None,
        camera_from: Some(nikon.from_local.clone()),
        camera_to: Some(nikon.to_local.clone()),
    };
    let preview = datefix::preview(&db.read(), &fix, &FixedTz(120)).unwrap();
    assert!(preview.label.starts_with("−1 y 2 h"), "{}", preview.label);
    let (_, n) = datefix::apply(&db.write(), &fix, &FixedTz(120)).unwrap();
    assert!(n >= wrong.len(), "fixed {n} of {}", wrong.len());
    let mut off = Vec::new();
    for (rel, v) in &wrong {
        let got = local_of(id_of(rel));
        if got != v["true_local"].as_str().unwrap() {
            off.push(format!("{rel}: {got} vs {}", v["true_local"]));
        }
    }
    assert!(off.is_empty(), "{}", off.join("\n"));

    // The Canon on home time is flagged against the phones' zone.
    if let Some(canon) = datefix::sets(&db.read(), i64::MAX).unwrap().iter().find(|s| s.label.contains("80D")) {
        assert!(canon.reasons.iter().any(|r| r.contains("UTC−4")), "{:?}", canon.reasons);
    }

    // 500 photos from one GPX file, in one accepted suggestion.
    places::import_track(&db.write(), &root.join("location-history/dolomites-hike-2022-07-16.gpx")).unwrap();
    let (groups, by_key) = places::suggest(&db.read(), None, 30).unwrap();
    let hike = groups.iter().find(|g| g.source == "track" && g.evidence.contains("dolomites")).expect("a group from the hike");
    assert!(hike.count >= 500, "{} items in the hike group", hike.count);
    places::accept(&db.write(), &by_key[&hike.key], "Set the location of 500 items from location history").unwrap();
    let mut far = Vec::new();
    let covered: Vec<(&String, &&Value)> =
        items.iter().filter(|(_, v)| v["flags"].as_array().unwrap().iter().any(|f| f == "gpx_covered")).collect();
    assert_eq!(covered.len(), 500);
    for (rel, v) in &covered {
        // In-memory databases read through the one writer lock: look the id
        // up before taking it again.
        let id = id_of(rel);
        let (lat, lon): (Option<f64>, Option<f64>) =
            db.read().query_row("SELECT lat, lon FROM assets WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        let (Some(lat), Some(lon)) = (lat, lon) else {
            far.push(format!("{rel}: no place"));
            continue;
        };
        let t = v["true_gps"].as_array().unwrap();
        let d = roll_geo::distance_m(lat, lon, t[0].as_f64().unwrap(), t[1].as_f64().unwrap());
        if d > 150.0 {
            far.push(format!("{rel}: {d:.0} m off"));
        }
    }
    assert!(far.is_empty(), "{} of 500 misplaced:\n{}", far.len(), far.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));

    // The Sony files state their zone, so no time zone follow-up is due
    // (unit tests cover items without one).
    let ids: Vec<i64> = covered.iter().map(|(rel, _)| id_of(rel)).collect();
    assert!(places::zone_proposals(&db.read(), &ids).unwrap().is_empty());
}
