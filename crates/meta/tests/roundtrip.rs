//! Metadata round trip on copies of fixture files: write with ExifTool,
//! read back with ExifTool and the native reader, then undo. Skipped when
//! the fixtures or ExifTool are missing.

use std::path::{Path, PathBuf};

use roll_meta::MetaReader;
use roll_meta::exiftool::ExifToolPool;
use roll_meta::model::{DateSource, Gps, MediaKind, Taken, parse_datetime};
use roll_meta::write::{self, MetaEdit, WriteMode, sidecar_path};
use serde_json::Value;

fn fixtures() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out/library/Dump");
    p.is_dir().then_some(p)
}

fn edit() -> MetaEdit {
    let (local, _) = parse_datetime("2019:08:12 14:03:22").unwrap();
    MetaEdit {
        taken: Some(Taken::new(local, Some(120), DateSource::ExifOriginal)),
        gps: Some(Some(Gps { lat: 45.6416, lon: 10.6875, alt: Some(70.0), accuracy_m: Some(5000.0) })),
        tags: Some(vec!["Category/Memes".into(), "Event/2019-08 Lake Garda".into(), "Zürich".into()]),
        description: Some(Some("Line one\nLine two".into())),
        rating: Some(4),
        orientation: None,
    }
}

fn read_tags(pool: &ExifToolPool, file: &Path) -> serde_json::Map<String, Value> {
    pool.with(|t| {
        t.read_json(
            &[file],
            &[
                "-DateTimeOriginal",
                "-OffsetTimeOriginal",
                "-GPSLatitude",
                "-GPSLongitude",
                "-Subject",
                "-TagsList",
                "-HierarchicalSubject",
                "-Keywords",
                "-Rating",
                "-ImageDescription",
                "-Description",
                "-Keys:CreationDate",
                "-QuickTime:CreateDate",
                "-ImageDataHash",
            ],
        )
    })
    .unwrap()
    .remove(0)
    .as_object()
    .cloned()
    .unwrap()
}

fn get<'a>(m: &'a serde_json::Map<String, Value>, suffix: &str) -> Option<&'a Value> {
    m.iter().find(|(k, _)| k.ends_with(&format!(":{suffix}"))).map(|(_, v)| v)
}

fn round_trip(pool: &ExifToolPool, src: &Path, kind: MediaKind, format: &str) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(src.file_name().unwrap());
    std::fs::copy(src, &file).unwrap();
    let before = read_tags(pool, &file);
    let bytes_before = std::fs::read(&file).unwrap();

    let snap = write::apply(pool, &file, kind, format, &edit(), WriteMode::FileAndSidecar).unwrap();
    let after = read_tags(pool, &file);
    let name = src.file_name().unwrap().to_string_lossy();
    assert_eq!(get(&before, "ImageDataHash"), get(&after, "ImageDataHash"), "{name}: image data changed");
    let tags: Vec<String> =
        get(&after, "TagsList").and_then(Value::as_array).unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert_eq!(tags, vec!["Category/Memes", "Event/2019-08 Lake Garda", "Zürich"], "{name}");
    assert_eq!(get(&after, "Rating").and_then(Value::as_i64), Some(4), "{name}");
    if kind == MediaKind::Photo {
        assert_eq!(get(&after, "DateTimeOriginal").and_then(Value::as_str), Some("2019:08:12 14:03:22"), "{name}");
        assert_eq!(get(&after, "OffsetTimeOriginal").and_then(Value::as_str), Some("+02:00"), "{name}");
        let lat = get(&after, "GPSLatitude").and_then(Value::as_f64).unwrap();
        assert!((lat - 45.6416).abs() < 1e-4, "{name}: {lat}");
    } else {
        assert_eq!(get(&after, "CreateDate").and_then(Value::as_str), Some("2019:08:12 12:03:22"), "{name}: QuickTime UTC");
    }

    // The native reader sees the new values, sidecar applied on top.
    let mut m = MetaReader::default().read(&file).unwrap();
    let side = sidecar_path(&file);
    let x = roll_meta::xmp::parse(&std::fs::read_to_string(&side).unwrap());
    roll_meta::read::apply_sidecar(&mut m, &x);
    let t = m.taken.unwrap();
    assert_eq!((t.local.to_string().as_str(), t.offset_min), ("2019-08-12 14:03:22", Some(120)), "{name}");
    assert!((m.gps.unwrap().lat - 45.6416).abs() < 1e-4, "{name}");
    assert_eq!(m.gps.unwrap().accuracy_m, Some(5000.0), "{name}: accuracy from sidecar");
    assert_eq!(m.rating, Some(4), "{name}");
    assert_eq!(m.description.as_deref(), Some("Line one\nLine two"), "{name}");
    assert!(m.tags.contains(&"Event/2019-08 Lake Garda".to_string()), "{name}");

    // Undo restores the previous values and removes the new sidecar.
    write::restore(pool, &file, &snap).unwrap();
    let restored = read_tags(pool, &file);
    for key in [
        "DateTimeOriginal",
        "OffsetTimeOriginal",
        "GPSLatitude",
        "TagsList",
        "Subject",
        "Rating",
        "ImageDescription",
        "CreateDate",
        "ImageDataHash",
    ] {
        let (a, b) = (get(&before, key), get(&restored, key));
        // Coordinates come back re-encoded as rationals: equal within 1e-7°.
        let same = match (a.and_then(Value::as_f64), b.and_then(Value::as_f64)) {
            (Some(x), Some(y)) => (x - y).abs() < 1e-7,
            _ => a == b,
        };
        assert!(same, "{name}: {key} after undo: {a:?} vs {b:?}");
    }
    assert!(!side.exists(), "{name}: sidecar should be removed");
    if format == "jpeg" {
        // Restored values, though ExifTool may lay the file out differently.
        assert!(std::fs::read(&file).unwrap().len().abs_diff(bytes_before.len()) < 4096);
    }
}

#[test]
fn write_read_undo() {
    let (Some(dir), Ok(pool)) = (fixtures(), ExifToolPool::locate()) else {
        eprintln!("fixtures or ExifTool missing; skipping");
        return;
    };
    round_trip(&pool, &dir.join("IMG_3001.JPG"), MediaKind::Photo, "jpeg");
    round_trip(&pool, &dir.join("IMG_2101.HEIC"), MediaKind::Photo, "heic");
    round_trip(&pool, &dir.join("IMG_5001.PNG"), MediaKind::Photo, "png");
    round_trip(&pool, &dir.join("IMG_1100.MOV"), MediaKind::Video, "mov");
    round_trip(&pool, &dir.join("20230618_190500.mp4"), MediaKind::Video, "mp4");
}

#[test]
fn sidecar_only_leaves_file_untouched() {
    let (Some(dir), Ok(pool)) = (fixtures(), ExifToolPool::locate()) else { return };
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("IMG_3002.JPG");
    std::fs::copy(dir.join("IMG_3002.JPG"), &file).unwrap();
    let before = std::fs::read(&file).unwrap();
    let snap = write::apply(&pool, &file, MediaKind::Photo, "jpeg", &edit(), WriteMode::SidecarOnly).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), before);
    assert!(sidecar_path(&file).exists());
    write::restore(&pool, &file, &snap).unwrap();
    assert!(!sidecar_path(&file).exists());
}

#[test]
fn unicode_and_long_paths() {
    let (Some(dir), Ok(pool)) = (fixtures(), ExifToolPool::locate()) else { return };
    let tmp = tempfile::tempdir().unwrap();
    let mut deep = tmp.path().join("Zürich Ünïcødé 写真");
    for _ in 0..3 {
        deep.push("a folder name long enough to push the whole path past the old limit");
    }
    std::fs::create_dir_all(&deep).unwrap();
    let file = deep.join("Spaß & Café — été.jpg");
    std::fs::copy(dir.join("IMG_3003.JPG"), &file).unwrap();
    assert!(file.as_os_str().len() > 260);
    let e = MetaEdit { rating: Some(5), ..Default::default() };
    let snap = write::apply(&pool, &file, MediaKind::Photo, "jpeg", &e, WriteMode::FileAndSidecar).unwrap();
    assert_eq!(get(&read_tags(&pool, &file), "Rating").and_then(Value::as_i64), Some(5));
    write::restore(&pool, &file, &snap).unwrap();
}
