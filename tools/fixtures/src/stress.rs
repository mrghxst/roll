//! The stress library: N files from a few dozen templates, with varied
//! metadata spliced in per file. Pixels are cheap; metadata and sizes follow
//! the shares measured in the owner's real library (PROMPT.md section 2).

use std::path::Path;

use anyhow::Result;
use chrono::{Duration, NaiveDate};

use crate::exifblock::{self, ExifSpec};
use crate::pixels::{self, Rng};

const CAMERAS: &[(&str, &str)] = &[
    ("Apple", "iPhone XS"),
    ("Apple", "iPhone 12"),
    ("Apple", "iPhone 14 Pro"),
    ("NIKON CORPORATION", "NIKON D750"),
    ("Canon", "Canon EOS 80D"),
    ("FUJIFILM", "X-T3"),
    ("samsung", "SM-G991B"),
];

pub fn generate(out: &Path, n: usize) -> Result<()> {
    let photo_t: Vec<Vec<u8>> = (0..48).map(|k| pixels::jpeg(&pixels::scene(256, 192, 100 + k, 0), 75)).collect();
    let portrait_t: Vec<Vec<u8>> = (0..16).map(|k| pixels::jpeg(&pixels::scene(192, 256, 200 + k, 0), 75)).collect();
    let screen_t: Vec<Vec<u8>> = (0..12).map(|k| pixels::jpeg(&pixels::screenshot(118, 256, 300 + k, k % 2 == 0), 80)).collect();
    let saved_t: Vec<Vec<u8>> = (0..24).map(|k| pixels::jpeg(&pixels::social(138, 256, 400 + k), 75)).collect();
    let mut rng = Rng::new(7);
    let base = NaiveDate::from_ymd_opt(2010, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap();
    for i in 0..n {
        let local = base + Duration::seconds(rng.range(0, 16 * 365 * 86400));
        let roll = rng.f();
        let (bytes, spec) = if roll < 0.32 {
            let (make, model) = *rng.pick(CAMERAS);
            let gps = (rng.f() < 0.19).then(|| (35.0 + rng.f() * 20.0, -5.0 + rng.f() * 30.0, 100.0));
            let t = if rng.f() < 0.5 { rng.pick(&photo_t) } else { rng.pick(&portrait_t) };
            (
                t,
                ExifSpec {
                    make: Some(make.into()),
                    model: Some(model.into()),
                    date: Some(local.format("%Y:%m:%d %H:%M:%S").to_string()),
                    offset: Some("+01:00".into()),
                    gps,
                    ..Default::default()
                },
            )
        } else if roll < 0.43 {
            (
                rng.pick(&screen_t),
                ExifSpec {
                    date: Some(local.format("%Y:%m:%d %H:%M:%S").to_string()),
                    offset: Some("+01:00".into()),
                    user_comment: Some("Screenshot".into()),
                    width: Some(828),
                    height: Some(1792),
                    ..Default::default()
                },
            )
        } else {
            let (w, h) = *rng.pick(&[(854u32, 1590u32), (1024, 1024), (1152, 2048), (896, 1152), (750, 1334), (1080, 1350)]);
            (
                rng.pick(&saved_t),
                ExifSpec {
                    date: Some(local.format("%Y:%m:%d %H:%M:%S").to_string()),
                    offset: Some("+01:00".into()),
                    width: Some(w),
                    height: Some(h),
                    ..Default::default()
                },
            )
        };
        let dir = out.join(local.format("%Y").to_string());
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join(format!("IMG_stress_{} - {i}.jpeg", local.format("%Y"))),
            exifblock::insert_into_jpeg(bytes, &exifblock::tiff(&spec)),
        )?;
        if i % 10_000 == 0 && i > 0 {
            eprintln!("  {i} files");
        }
    }
    Ok(())
}
