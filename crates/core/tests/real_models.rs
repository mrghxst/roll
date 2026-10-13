//! Decision runs with real models, through the same Library calls the app
//! uses, on the CC0 set (`tools/cc0/build.mjs`) and optionally the fixture
//! library. Ignored by default: they need a running local server, or an
//! OpenRouter key in the keychain and money. Accuracy is reported, not
//! asserted (PROMPT 9.2); the OpenRouter run asserts the M5 cost points.
//!
//! ```text
//! ROLL_REAL_LOCAL=qwen3-vl:8b cargo test -p roll-core --test real_models local -- --ignored --nocapture
//! ROLL_REAL_OPENROUTER=google/gemma-3-12b-it ROLL_REAL_SET=all ROLL_REAL_BUDGET=0.10 \
//!   cargo test -p roll-core --test real_models openrouter -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use roll_core::library::Library;
use serde_json::Value;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), &t).unwrap();
        }
    }
}

fn wait_idle(lib: &Library, limit: Duration) {
    let start = Instant::now();
    let mut quiet = 0;
    while quiet < 3 {
        let busy = lib.jobs.list().iter().any(|j| matches!(j.state.as_str(), "queued" | "running"));
        quiet = if busy { 0 } else { quiet + 1 };
        assert!(start.elapsed() < limit, "jobs did not finish: {:?}", lib.jobs.list());
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// A library over a copy of the CC0 set (and the fixture library with
/// `ROLL_REAL_SET=all`), with every category cleared so the model sees
/// every item. Returns the truth by relative path.
fn setup(tmp: &Path) -> Option<(Arc<Library>, HashMap<String, String>)> {
    let cc0 = fixtures().join("cc0");
    if !cc0.join("manifest.json").exists() {
        eprintln!("fixtures-out/cc0 is missing; run tools/cc0/build.mjs");
        return None;
    }
    let archive = tmp.join("Archive");
    copy_dir(&cc0, &archive.join("cc0"));
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(cc0.join("manifest.json")).unwrap()).unwrap();
    let mut truth: HashMap<String, String> = manifest["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (format!("cc0/{}", i["file"].as_str().unwrap()), i["category"].as_str().unwrap().to_string()))
        .collect();
    if std::env::var("ROLL_REAL_SET").as_deref() == Ok("all") {
        let lib = fixtures().join("library");
        copy_dir(&lib, &archive.join("library"));
        let m: Value = serde_json::from_str(&std::fs::read_to_string(fixtures().join("manifest.json")).unwrap()).unwrap();
        for i in m["items"].as_array().into_iter().flatten() {
            if let (Some(p), Some(c)) = (i["path"].as_str(), i["category"].as_str()) {
                truth.insert(format!("library/{}", p.trim_start_matches("library/")), c.to_string());
            }
        }
    }
    let lib = Library::open(&tmp.join("data"), Arc::new(|_| {})).unwrap();
    lib.add_root(&archive).unwrap();
    lib.start();
    wait_idle(&lib, Duration::from_secs(900));
    lib.db.write().execute("UPDATE assets SET review = 0, category_id = NULL, category_source = NULL, category_conf = NULL", []).unwrap();
    Some((lib, truth))
}

struct Report {
    items: usize,
    answered: usize,
    correct: usize,
    unsure: usize,
    spent: f64,
    estimate: f64,
    seconds: f64,
}

/// Run the decision model over everything and compare with the truth.
fn run(lib: &Library, truth: &HashMap<String, String>, budget: Option<f64>) -> Report {
    let plan = lib.ai_plan().unwrap();
    println!("plan: {} items, estimate ${:.4}, {}", plan.items, plan.estimate_usd, plan.consent);
    lib.ai_start(budget).unwrap();
    wait_idle(lib, Duration::from_secs(3 * 3600));
    let run = lib.ai_runs().unwrap().into_iter().next().unwrap();
    println!(
        "run: {} · {} of {} · ${:.4} spent · {:.0} s · note {:?}",
        run.state, run.done, run.items, run.spent_usd, run.seconds, run.note
    );
    let c = lib.db.read();
    let mut errs = c.prepare("SELECT error, count(*) FROM ai_items WHERE run_id = ?1 AND state = 3 GROUP BY error").unwrap();
    for e in errs.query_map([run.id], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?))).unwrap().flatten() {
        println!("  failed: {} × {:?}", e.1, e.0);
    }
    let mut st = c
        .prepare(
            "SELECT a.rel_path, json_extract(i.verdict, '$.category') FROM ai_items i JOIN assets a ON a.id = i.asset_id
             WHERE i.run_id = ?1 AND i.state = 2",
        )
        .unwrap();
    let rows: Vec<(String, Option<String>)> =
        st.query_map([run.id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
    let mut per: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut confusion: BTreeMap<(String, String), usize> = BTreeMap::new();
    let (mut correct, mut unsure, mut answered) = (0, 0, 0);
    for (path, got) in &rows {
        let Some(want) = truth.get(path) else { continue };
        answered += 1;
        let e = per.entry(want.clone()).or_default();
        e.0 += 1;
        match got {
            Some(g) if g == want => {
                correct += 1;
                e.1 += 1;
            }
            Some(g) => *confusion.entry((want.clone(), g.clone())).or_default() += 1,
            None => unsure += 1,
        }
    }
    for (cat, (n, ok)) in &per {
        println!("  {cat:<14} {ok:>3} of {n:>3} right");
    }
    for ((want, got), n) in &confusion {
        println!("  {want} taken for {got}: {n}");
    }
    println!("accuracy {:.1}% ({correct} of {answered}), unsure {unsure}", correct as f64 / answered.max(1) as f64 * 100.0);
    Report { items: run.items as usize, answered, correct, unsure, spent: run.spent_usd, estimate: run.estimate_usd, seconds: run.seconds }
}

#[test]
#[ignore = "needs a local AI server and model (ROLL_REAL_LOCAL)"]
fn local() {
    let Ok(model) = std::env::var("ROLL_REAL_LOCAL") else { return };
    let tmp = tempfile::tempdir().unwrap();
    let Some((lib, truth)) = setup(tmp.path()) else { return };
    let mut s = lib.settings();
    s.ai_mode = "local".into();
    s.ai_local_endpoint = std::env::var("ROLL_REAL_ENDPOINT").ok();
    s.ai_decision_model = Some(model);
    lib.save_settings(&s).unwrap();
    let r = run(&lib, &truth, None);
    println!("speed {:.2} items/s", r.answered as f64 / r.seconds.max(0.1));
    assert!(r.answered as f64 >= r.items as f64 * 0.9, "most items answered");
    let _ = (r.correct, r.unsure, r.spent, r.estimate);
}

#[test]
#[ignore = "spends money: needs ROLL_REAL_OPENROUTER and an OpenRouter key in the keychain"]
fn openrouter() {
    let Ok(model) = std::env::var("ROLL_REAL_OPENROUTER") else { return };
    let budget: f64 = std::env::var("ROLL_REAL_BUDGET").ok().and_then(|b| b.parse().ok()).unwrap_or(0.10);
    let tmp = tempfile::tempdir().unwrap();
    let Some((lib, truth)) = setup(tmp.path()) else { return };
    let mut s = lib.settings();
    s.ai_mode = "openrouter".into();
    s.network.ai = true;
    s.ai_budget_usd = budget;
    lib.save_settings(&s).unwrap();
    let offers = lib.ai_offers().unwrap();
    let offer = offers.iter().find(|o| o.id == model).unwrap_or_else(|| panic!("{model} is not offered"));
    println!("offer: {} · {} · ${:.2} per 100k · {}", offer.name, offer.quality, offer.per_100k_usd, offer.data);
    lib.ai_pick(&model, Some(offer)).unwrap();
    let r = run(&lib, &truth, Some(budget));
    assert!(r.spent <= budget, "spent ${} over the ${budget} budget", r.spent);
    if r.answered == r.items {
        let ratio = r.estimate / r.spent.max(1e-9);
        println!("estimate ${:.4} vs measured ${:.4} ({:.0}%)", r.estimate, r.spent, ratio * 100.0);
        assert!((0.75..=1.25).contains(&ratio), "estimate off by more than 25%");
    }
}
