//! M4 acceptance with the real image model: the CC0 set (tools/cc0) as a
//! library, sorted with rules and the image model only (nothing leaves the
//! machine), and searched by words.
//!
//! ROLL_PACKS=fixtures-out/packs cargo test --release -p roll-core --test vision_real -- --ignored --nocapture
//!
//! Needs the image-model pack in ROLL_PACKS (`cargo run -p roll-core
//! --example packs -- fixtures-out/packs image-model`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use roll_core::library::Library;
use roll_core::model::Query;
use serde_json::Value;

fn wait_idle(lib: &Library, limit: Duration) {
    let start = Instant::now();
    let mut quiet = 0;
    while quiet < 5 {
        let busy = lib.jobs.list().iter().any(|j| matches!(j.state.as_str(), "queued" | "running"));
        quiet = if busy { 0 } else { quiet + 1 };
        assert!(start.elapsed() < limit, "still busy");
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
#[ignore = "needs the image model (ROLL_PACKS) and the CC0 set"]
fn sorts_and_searches_the_cc0_set() {
    let Some(packs) = std::env::var_os("ROLL_PACKS") else { return };
    let set = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out/cc0");
    assert!(set.join("manifest.json").exists(), "run tools/cc0/build.mjs first");
    assert!(PathBuf::from(&packs).join("models/siglip2-b16/visual/model.onnx").exists());
    let manifest: Value = serde_json::from_slice(&std::fs::read(set.join("manifest.json")).unwrap()).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let lib_dir = tmp.path().join("Archive");
    let mut truth: HashMap<String, String> = HashMap::new();
    for it in manifest["items"].as_array().unwrap() {
        let file = it["file"].as_str().unwrap();
        let name = file.replace('/', "_");
        std::fs::create_dir_all(&lib_dir).unwrap();
        std::fs::copy(set.join(file), lib_dir.join(&name)).unwrap();
        truth.insert(name, it["category"].as_str().unwrap().to_string());
    }
    let lib = Library::open(&tmp.path().join("data"), Arc::new(|_| {})).unwrap();
    assert!(lib.vision().is_some(), "image model not found in ROLL_PACKS");
    let t = Instant::now();
    lib.add_root(&lib_dir).unwrap();
    lib.start();
    wait_idle(&lib, Duration::from_secs(900));
    let el = t.elapsed().as_secs_f32();

    let rows: Vec<(String, Option<String>, i64, Option<String>)> = {
        let c = lib.db.read();
        let mut st = c
            .prepare("SELECT a.name, cat.key, a.review, a.category_source FROM assets a LEFT JOIN categories cat ON cat.id = a.category_id")
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().collect::<Result<_, _>>().unwrap()
    };
    let embedded: i64 = lib.db.read().query_row("SELECT count(*) FROM embeddings", [], |r| r.get(0)).unwrap();
    assert_eq!(embedded as usize, truth.len(), "every item gets an embedding");
    let n = rows.len();
    let auto: Vec<_> = rows.iter().filter(|r| r.2 == 2 && r.1.is_some()).collect();
    let right = auto.iter().filter(|r| truth.get(&r.0) == r.1.as_ref()).count();
    let proposed: Vec<_> = rows.iter().filter(|r| r.2 == 1 && r.1.is_some()).collect();
    let proposed_right = proposed.iter().filter(|r| truth.get(&r.0) == r.1.as_ref()).count();
    let mut by_source: HashMap<String, usize> = HashMap::new();
    for r in &auto {
        *by_source.entry(r.3.clone().unwrap_or_default()).or_default() += 1;
    }
    for r in auto.iter().filter(|r| truth.get(&r.0) != r.1.as_ref()) {
        println!("  wrong: {} is {} but sorted as {:?} ({:?})", r.0, truth[&r.0], r.1, r.3);
    }
    let share = auto.len() as f32 / n as f32;
    let precision = right as f32 / auto.len().max(1) as f32;
    println!(
        "{n} items in {el:.0} s: {} sorted on their own ({:.0}%), {:.1}% right; {} proposed, {} of them right; by source {by_source:?}",
        auto.len(),
        share * 100.0,
        precision * 100.0,
        proposed.len(),
        proposed_right
    );
    // Precision by stored confidence, for calibrating `zero_shot_conf`.
    let mut by_conf: Vec<(f64, bool)> = {
        let c = lib.db.read();
        let mut st = c.prepare("SELECT a.name, cat.key, a.category_conf FROM assets a JOIN categories cat ON cat.id = a.category_id WHERE a.category_source = 'image-model'").unwrap();
        st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .map(|(name, key, conf)| (conf, truth.get(&name) == Some(&key)))
            .collect()
    };
    by_conf.sort_by(|a, b| b.0.total_cmp(&a.0));
    for frac in [0.5, 0.55, 0.6, 0.65, 0.7] {
        let k = (by_conf.len() as f32 * frac) as usize;
        let ok = by_conf[..k].iter().filter(|x| x.1).count();
        println!("  top {:.0}%: precision {:.1}% at conf >= {:.3}", frac * 100.0, ok as f32 * 100.0 / k as f32, by_conf[k - 1].0);
    }
    assert!(share >= 0.60, "auto-accepted {:.0}%", share * 100.0);
    assert!(precision >= 0.97, "precision {:.1}%", precision * 100.0);

    // Search by what is in the picture.
    let names: HashMap<i64, String> = {
        let c = lib.db.read();
        let mut st = c.prepare("SELECT id, name FROM assets").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap()
    };
    for (q, want) in
        [("dog", "photos_photos-004.jpg"), ("receipt", "documents_documents-006.jpg"), ("nebula", "wallpapers_wallpapers-009.jpg")]
    {
        let ids = lib.ids(&Query { text: Some(q.into()), ..Default::default() }).unwrap();
        let found: Vec<&str> = ids.iter().filter_map(|i| names.get(i).map(String::as_str)).collect();
        println!("search {q:?}: {} results", found.len());
        assert!(found.contains(&want), "{q}: {found:?}");
        assert!(found.len() < n / 3, "{q}: too broad ({})", found.len());
    }
}
