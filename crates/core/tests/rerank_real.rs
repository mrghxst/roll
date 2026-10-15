//! "Better ranking" with a real local model on the CC0 set: precision of
//! the first 5 best matches before and after the decision model rates them.
//! Relevance comes from the pictures' Openverse titles, so it is rough;
//! reported, not asserted (PROMPT 9.2).
//!
//! ```text
//! ROLL_PACKS=fixtures-out/packs ROLL_REAL_LOCAL=qwen3-vl:8b-instruct \
//!   cargo test --release -p roll-core --test rerank_real -- --ignored --nocapture
//! ```

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
    while quiet < 3 {
        let busy = lib.jobs.list().iter().any(|j| matches!(j.state.as_str(), "queued" | "running"));
        quiet = if busy { 0 } else { quiet + 1 };
        assert!(start.elapsed() < limit);
        std::thread::sleep(Duration::from_millis(300));
    }
}

#[test]
#[ignore = "needs the image model (ROLL_PACKS), a local AI server (ROLL_REAL_LOCAL) and the CC0 set"]
fn better_ranking_on_the_cc0_set() {
    // Without ROLL_REAL_LOCAL only the order before ranking is shown.
    let Some(_) = std::env::var_os("ROLL_PACKS") else { return };
    let model = std::env::var("ROLL_REAL_LOCAL").ok();
    let set = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out/cc0");
    assert!(set.join("manifest.json").exists(), "run tools/cc0/build.mjs first");
    let manifest: Value = serde_json::from_slice(&std::fs::read(set.join("manifest.json")).unwrap()).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let lib_dir = tmp.path().join("Archive");
    std::fs::create_dir_all(&lib_dir).unwrap();
    // Neutral file names, so only the pictures (and text in them) match.
    let mut titles: HashMap<String, String> = HashMap::new();
    for (i, it) in manifest["items"].as_array().unwrap().iter().enumerate() {
        let file = it["file"].as_str().unwrap();
        let ext = file.rsplit('.').next().unwrap();
        let name = format!("item-{i:03}.{ext}");
        std::fs::copy(set.join(file), lib_dir.join(&name)).unwrap();
        titles.insert(name, it["source"]["title"].as_str().unwrap_or("").to_lowercase());
    }
    let lib = Library::open(&tmp.path().join("data"), Arc::new(|_| {})).unwrap();
    assert!(lib.vision().is_some(), "image model not found in ROLL_PACKS");
    lib.add_root(&lib_dir).unwrap();
    lib.start();
    wait_idle(&lib, Duration::from_secs(900));
    let mut s = lib.settings();
    s.ai_mode = "local".into();
    s.ai_local_endpoint = std::env::var("ROLL_REAL_ENDPOINT").ok();
    s.ai_decision_model = model.clone();
    lib.save_settings(&s).unwrap();

    let names: HashMap<i64, String> = {
        let c = lib.db.read();
        let mut st = c.prepare("SELECT id, name FROM assets").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap()
    };
    let searches: [(&str, &[&str]); 8] = [
        ("cat", &["cat"]),
        ("dog", &["dog"]),
        ("food on a plate", &["food", "plate"]),
        ("beach", &["beach"]),
        ("city street", &["city", "street"]),
        ("birthday party", &["birthday", "party"]),
        ("handwritten letter", &["handwritten", "letter"]),
        ("mountains", &["mountain"]),
    ];
    let (mut before_hits, mut after_hits, mut total) = (0, 0, 0);
    for (words, keys) in searches {
        let q = Query { text: Some(words.into()), ..Default::default() };
        // A title word that starts with a key ("mountains", not "location").
        let fits = |id: &i32| {
            let title = &titles[&names[&(*id as i64)]];
            title.split(|c: char| !c.is_alphanumeric()).any(|w| keys.iter().any(|k| w.starts_with(k)))
        };
        let relevant = |ids: &[i32]| ids.iter().take(5).filter(|id| fits(id)).count();
        let top = |ids: &[i32]| {
            ids.iter().take(5).map(|id| titles[&names[&(*id as i64)]].chars().take(24).collect::<String>()).collect::<Vec<_>>()
        };
        let before = lib.best_matches(&q).unwrap();
        let t = Instant::now();
        let by_text: i64 = lib
            .db
            .read()
            .query_row("SELECT count(*) FROM fts WHERE fts MATCH ?1", [roll_core::search::fts_match(words).unwrap()], |r| r.get(0))
            .unwrap();
        let after = if model.is_some() { lib.rerank(&q).unwrap() } else { before.clone() };
        let mut a = after.ids.clone();
        let mut b = before.ids.clone();
        a.sort();
        b.sort();
        assert_eq!(a, b, "the same items, in a new order");
        let (rb, ra) = (relevant(&before.ids), relevant(&after.ids));
        let shown = before.ids.len().min(5);
        before_hits += rb;
        after_hits += ra;
        total += shown;
        println!(
            "{words:?}: {} matches ({by_text} by text), top 5 relevant {rb} -> {ra} ({:.1} s)",
            before.ids.len(),
            t.elapsed().as_secs_f32()
        );
        println!("  before {:?}\n  after  {:?}", top(&before.ids), top(&after.ids));
    }
    println!(
        "precision at 5: {:.0}% -> {:.0}%",
        before_hits as f32 * 100.0 / total.max(1) as f32,
        after_hits as f32 * 100.0 / total.max(1) as f32
    );
}
