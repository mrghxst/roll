//! Write a date, tags, a rating and a location into copies of fixture files,
//! for checking what Windows Explorer and the Photos app show.
//!
//! `cargo run -p roll-meta --example write_check -- <out dir>`

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use roll_meta::exiftool::ExifToolPool;
use roll_meta::model::{DateSource, Gps, MediaKind, Taken};
use roll_meta::write::{self, MetaEdit, WriteMode};

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("output folder"));
    std::fs::create_dir_all(&out).unwrap();
    let lib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out/library");
    let pool = ExifToolPool::locate().expect("ExifTool");
    let files: [(&str, &str, MediaKind); 4] = [
        ("Dump/IMG_1001.JPG", "jpeg", MediaKind::Photo),
        ("Dump/IMG_1002.HEIC", "heic", MediaKind::Photo),
        ("Dump/IMG_0640.PNG", "png", MediaKind::Photo),
        ("Dump/20230618_190500.mp4", "mp4", MediaKind::Video),
    ];
    let edit = MetaEdit {
        taken: Some(Taken::new(
            NaiveDate::from_ymd_opt(2019, 8, 12).unwrap().and_hms_opt(11, 0, 0).unwrap(),
            Some(120),
            DateSource::ExifOriginal,
        )),
        gps: Some(Some(Gps { lat: 45.6789, lon: 10.6543, alt: None, accuracy_m: Some(10.0) })),
        tags: Some(vec!["Trips/Lake Garda".into(), "Category/Photos".into()]),
        description: Some(Some("Written by Roll".into())),
        rating: Some(4),
        orientation: None,
    };
    for (rel, format, kind) in files {
        let src = lib.join(rel);
        if !src.exists() {
            eprintln!("missing {rel}; skipped");
            continue;
        }
        let dst = out.join(Path::new(rel).file_name().unwrap());
        std::fs::copy(&src, &dst).unwrap();
        match write::apply(&pool, &dst, kind, format, &edit, WriteMode::FileAndSidecar) {
            Ok(_) => println!("{}", dst.display()),
            Err(e) => eprintln!("{rel}: {e}"),
        }
    }
}
