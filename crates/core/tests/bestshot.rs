//! Best shots through the library: a sharp, well exposed picture and its
//! blurred, dark and blown-out versions as a series; the original ranks
//! first (sharpness and exposure only, without the image model).

use std::sync::Arc;
use std::time::{Duration, Instant};

use image::{Rgb, RgbImage};
use roll_core::library::Library;

fn wait_idle(lib: &Library) {
    let start = Instant::now();
    let mut quiet = 0;
    while quiet < 3 {
        let busy = lib.jobs.list().iter().any(|j| matches!(j.state.as_str(), "queued" | "running"));
        quiet = if busy { 0 } else { quiet + 1 };
        assert!(start.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// A detailed scene: gradients, stripes and squares at several scales.
fn scene() -> RgbImage {
    RgbImage::from_fn(1200, 900, |x, y| {
        let stripes = if (x / 6 + y / 9) % 2 == 0 { 60 } else { 0 };
        let square = if (x / 75 + y / 75) % 3 == 0 { 70 } else { 0 };
        let r = (x * 140 / 1200 + stripes) as u8;
        let g = (y * 140 / 900 + square) as u8;
        let b = ((x + y) % 97 + 40) as u8;
        Rgb([r.saturating_add(30), g.saturating_add(30), b])
    })
}

fn adjust(img: &RgbImage, f: impl Fn(u8) -> u8) -> RgbImage {
    let mut out = img.clone();
    out.pixels_mut().for_each(|p| p.0 = p.0.map(&f));
    out
}

#[test]
fn the_sharp_well_exposed_frame_ranks_first() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Burst");
    std::fs::create_dir_all(&dir).unwrap();
    let base = scene();
    let frames = [
        ("a_blurred.jpg", image::imageops::blur(&base, 4.0)),
        ("b_original.jpg", base.clone()),
        ("c_dark.jpg", adjust(&base, |v| v / 5)),
        ("d_blown.jpg", adjust(&base, |v| v.saturating_add(170))),
        ("e_soft.jpg", image::imageops::blur(&base, 1.5)),
    ];
    for (name, img) in &frames {
        img.save(dir.join(name)).unwrap();
    }
    let lib = Library::open(&tmp.path().join("data"), Arc::new(|_| {})).unwrap();
    lib.add_root(&dir).unwrap();
    lib.start();
    wait_idle(&lib);
    let id = |name: &str| -> i64 { lib.db.read().query_row("SELECT id FROM assets WHERE name = ?1", [name], |r| r.get(0)).unwrap() };
    let ids: Vec<i64> = frames.iter().map(|f| id(f.0)).collect();
    let ranked = lib.best_shots(&ids).unwrap();
    let names: Vec<&str> = ranked.iter().map(|r| frames.iter().find(|f| id(f.0) == r.id as i64).unwrap().0).collect();
    println!("{names:?} {:?}", ranked.iter().map(|r| r.score).collect::<Vec<_>>());
    assert_eq!(names[0], "b_original.jpg");
    assert!(ranked[0].keep && ranked[0].notes.contains(&"Sharpest".to_string()), "{:?}", ranked[0]);
    let pos = |n: &str| names.iter().position(|x| *x == n).unwrap();
    assert!(pos("a_blurred.jpg") > pos("e_soft.jpg"), "more blur ranks lower");
    assert!(pos("c_dark.jpg") >= 2 && pos("d_blown.jpg") >= 2);
}
