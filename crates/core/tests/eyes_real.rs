//! Open or closed eyes with the real faces and eyes packs, on CC0 and
//! public domain portraits labelled by eye (`tools/cc0/eyes.json`, fetched
//! with `node tools/cc0/eyes.mjs` and the extra queries listed in
//! docs/benchmarks.md), then a blink pair ranked as best shots.
//!
//! ```text
//! ROLL_PACKS=<abs path>/fixtures-out/packs cargo test --release -p roll-core --test eyes_real -- --ignored --nocapture
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use roll_core::library::Library;

#[test]
#[ignore = "needs the faces and eyes packs (ROLL_PACKS) and the eye candidates"]
fn tells_closed_eyes_and_ranks_a_blink_last() {
    let Some(packs) = std::env::var_os("ROLL_PACKS").map(PathBuf::from) else { return };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let set = root.join("fixtures-out/eyes/candidates");
    let labels: HashMap<String, String> = serde_json::from_slice(&std::fs::read(root.join("tools/cc0/eyes.json")).unwrap()).unwrap();
    let candidates: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(set.join("candidates.json")).unwrap()).unwrap();
    let file_of: HashMap<&str, &str> = candidates.iter().map(|c| (c["url"].as_str().unwrap(), c["file"].as_str().unwrap())).collect();

    let faces = roll_ai::faces::Faces::load(&packs.join("models/faces"), true).unwrap();
    let eyes = roll_ai::eyes::Eyes::load(&packs.join("models/eyes"), true).unwrap();
    let (mut found, mut closed, mut wrong, mut open, mut checked) = (0, 0, 0, 0, 0);
    let t = Instant::now();
    for (url, marks) in &labels {
        let Some(file) = file_of.get(url.as_str()) else { continue };
        // As the best shots see it: the 1024 px preview.
        let img = image::open(set.join(file)).unwrap().resize(1024, 1024, image::imageops::FilterType::Triangle).to_rgb8();
        let seen: Vec<_> = faces.detect(&img).unwrap().into_iter().filter(|f| f.rect[2] >= 24.0).collect();
        assert_eq!(seen.len(), marks.len(), "{file}: faces found");
        for (f, mark) in seen.iter().zip(marks.chars()) {
            if mark == 'x' {
                continue;
            }
            let s = eyes.state(&img, f).unwrap();
            checked += 1;
            match mark {
                'c' => {
                    closed += 1;
                    found += usize::from(s.closed());
                }
                _ => {
                    open += 1;
                    wrong += usize::from(s.closed());
                }
            }
        }
    }
    println!(
        "{found} of {closed} closed found, {wrong} of {open} open called closed; {checked} faces in {:.1} s",
        t.elapsed().as_secs_f32()
    );
    assert!(closed >= 40 && open >= 40, "the labelled set is there");
    assert!(found as f32 >= 0.75 * closed as f32, "finds most closed eyes");
    assert!(wrong as f32 <= 0.03 * open as f32, "rarely calls open eyes closed");

    // A blink pair: the same child, eyes open, then closed while smiling.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Archive");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, url) in [
        ("open.jpg", "https://live.staticflickr.com/5010/5229537756_9e3e6d1003_b.jpg"),
        ("blink.jpg", "https://live.staticflickr.com/5287/5229537558_f299daba5c_b.jpg"),
    ] {
        std::fs::copy(set.join(file_of[url]), dir.join(name)).unwrap();
    }
    let lib = Library::open(&tmp.path().join("data"), Arc::new(|_| {})).unwrap();
    lib.add_root(&dir).unwrap();
    lib.start();
    let start = Instant::now();
    let ids = loop {
        let ids: Vec<i64> = lib
            .db
            .read()
            .prepare("SELECT id FROM assets ORDER BY rel_path")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .flatten()
            .collect();
        let previews = ids.iter().filter(|&&id| lib.preview(id).ok().flatten().is_some()).count();
        if ids.len() == 2 && previews == 2 {
            break ids;
        }
        assert!(start.elapsed() < Duration::from_secs(300), "previews made");
        std::thread::sleep(Duration::from_millis(300));
    };
    let rel = |id: i64| -> String { lib.db.read().query_row("SELECT rel_path FROM assets WHERE id = ?1", [id], |r| r.get(0)).unwrap() };
    let ranked = lib.best_shots(&ids).unwrap();
    for s in &ranked {
        println!("{} {:.2} {:?}", rel(i64::from(s.id)), s.score, s.notes);
    }
    assert_eq!(rel(i64::from(ranked[0].id)), "open.jpg");
    assert!(ranked[1].notes.contains(&"Eyes closed".to_string()));
    assert!(!ranked[0].notes.iter().any(|n| n.starts_with("Eyes")));
}
