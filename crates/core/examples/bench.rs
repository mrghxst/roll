//! Measures the performance budgets of PROMPT.md section 7.5 on a folder,
//! usually the stress library:
//!
//! ```text
//! cargo run --release -p roll-core --example bench -- fixtures-out/stress
//! ```
//!
//! Uses a temporary database; the folder is only read.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use roll_core::dates::SystemTz;
use roll_core::db::Db;
use roll_core::model::Query;
use roll_core::thumbs::{self, ThumbStore};
use roll_core::{groups, query, scan};

fn rss_mb() -> f64 {
    memory_stats::memory_stats().map(|m| m.physical_mem as f64 / 1e6).unwrap_or(0.0)
}

fn row(what: &str, took: Duration, budget: &str) {
    println!("| {what} | {:.2} s | {budget} | {:.0} MB |", took.as_secs_f64(), rss_mb());
}

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "fixtures-out/stress".into()));
    let root = std::path::absolute(root)?;
    let tmp = tempfile::tempdir()?;
    let db = Db::open(&tmp.path().join("library.db"))?;
    db.write().execute("INSERT INTO roots (id, path, added_at) VALUES (1, ?1, 0)", [root.to_string_lossy()])?;
    println!("| Step | Took | Budget | RSS |");
    println!("|---|---|---|---|");

    let start = Instant::now();
    let first = Mutex::new(None::<Duration>);
    let progress = |phase: &str, _: usize, _: usize| {
        if phase == scan::SAVED {
            first.lock().unwrap().get_or_insert(start.elapsed());
        }
    };
    let (report, _) = scan::scan_root(&db, 1, &root, &SystemTz, false, &progress)?;
    let index = start.elapsed();
    if let Some(f) = *first.lock().unwrap() {
        row("First items in the database", f, "thumbnails visible < 10 s");
    }
    row(&format!("Metadata index ({} files)", report.added), index, "< 10 min");
    let t = Instant::now();
    groups::rebuild(&db, 1)?;
    row("Groups", t.elapsed(), "");

    let q = Query::default();
    for label in ["Timeline payload (cold)", "Timeline payload (warm)"] {
        let t = Instant::now();
        let bytes = query::timeline(&db.read(), &q)?;
        row(&format!("{label}, {} KB", bytes.len() / 1024), t.elapsed(), "scrubber jump < 100 ms");
    }
    let t = Instant::now();
    let indexed = db.tx(|tx| roll_core::search::reindex(tx, None, &|_, _| None))?;
    row(&format!("Search index ({indexed} items)"), t.elapsed(), "background job");
    let today = chrono::Local::now().date_naive();
    for text in ["img", "videos summer 2019", "dsc 2021 without screenshots"] {
        let t = Instant::now();
        let q = roll_core::search::expand(&db.read(), &Query { text: Some(text.into()), ..Default::default() }, None, today);
        let bytes = query::timeline(&db.read(), &q)?;
        let hits = u32::from_le_bytes(bytes[8..12].try_into()?);
        row(&format!("Search “{text}” to timeline ({hits} hits)"), t.elapsed(), "< 300 ms");
    }
    let t = Instant::now();
    let n = query::count(&db.read(), &Query { cameras: vec!["NIKON D750".into()], location: Some("none".into()), ..Default::default() })?;
    row(&format!("Filtered count ({n})"), t.elapsed(), "< 300 ms");

    if std::env::args().any(|a| a == "--no-thumbs") {
        return Ok(());
    }
    let store = ThumbStore::open(&tmp.path().join("thumbs.db"))?;
    let ff = roll_media::ffmpeg::Ffmpeg::locate();
    let pool = rayon::ThreadPoolBuilder::new().build()?;
    let t = Instant::now();
    let made = thumbs::run_pending(&db, &store, ff.as_ref(), &pool, &|_, _| true)?;
    row(&format!("Thumbnails ({made})"), t.elapsed(), "< 30 min");
    let (small, _) = store.bytes()?;
    println!("\nThumbnail store: {:.1} MB, {:.1} KB per item", small as f64 / 1e6, small as f64 / 1024.0 / made.max(1) as f64);
    Ok(())
}
