//! Reads the generated fixture library and checks the native reader against
//! the manifest. Skipped when `fixtures-out/` has not been generated
//! (`cargo run -p roll-fixtures`).

use std::path::PathBuf;

use roll_meta::detect::{classify_name, FileClass};
use roll_meta::MetaReader;
use serde_json::Value;

fn fixtures() -> Option<(PathBuf, Value)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out");
    let manifest = std::fs::read_to_string(root.join("manifest.json")).ok()?;
    Some((root, serde_json::from_str(&manifest).ok()?))
}

#[test]
fn reader_matches_manifest() {
    let Some((root, manifest)) = fixtures() else {
        eprintln!("fixtures-out not found; skipping");
        return;
    };
    let mut reader = MetaReader::default();
    let mut checked = 0;
    let mut failures = Vec::new();
    for item in manifest["items"].as_array().unwrap() {
        let rel = item["path"].as_str().unwrap();
        let name = rel.rsplit('/').next().unwrap();
        if !matches!(classify_name(name), FileClass::Media(_)) {
            continue;
        }
        let flags: Vec<&str> = item["flags"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        let m = match reader.read(&root.join(rel)) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("{rel}: {e}"));
                continue;
            }
        };
        checked += 1;
        if flags.contains(&"broken") || flags.contains(&"no_metadata") || rel.ends_with(".gif") {
            continue;
        }
        // The date the file claims.
        let expect_local = item["file_local"].as_str().or(item["true_local"].as_str());
        let is_video = item["kind"] == "video";
        if let (Some(expect), false) = (expect_local, flags.contains(&"missing_date") || rel.ends_with(".NEF")) {
            match &m.taken {
                Some(t) => {
                    let got = t.local.format("%Y-%m-%dT%H:%M:%S").to_string();
                    // Videos without the Apple key store UTC; compare instants.
                    let ok = if is_video && t.offset_min.is_none() {
                        let off = item["file_offset_min"].as_i64().or(item["true_offset_min"].as_i64()).unwrap_or(0);
                        let exp = chrono::NaiveDateTime::parse_from_str(expect, "%Y-%m-%dT%H:%M:%S").unwrap() - chrono::Duration::minutes(off);
                        t.local == exp
                    } else {
                        got == expect
                    };
                    if !ok {
                        failures.push(format!("{rel}: date {got}, expected {expect}"));
                    }
                }
                None if item["category"] == "photos" || item["category"] == "screenshots" && !rel.ends_with(".png") => {
                    failures.push(format!("{rel}: no date, expected {expect}"))
                }
                None => {}
            }
        }
        if flags.contains(&"missing_date") && m.taken.is_some() {
            failures.push(format!("{rel}: has a date, expected none"));
        }
        if item["has_gps"] == true {
            let g = item["true_gps"].as_array().unwrap();
            let (lat, lon) = (g[0].as_f64().unwrap(), g[1].as_f64().unwrap());
            match m.gps {
                Some(p) if (p.lat - lat).abs() < 1e-4 && (p.lon - lon).abs() < 1e-4 => {}
                other => failures.push(format!("{rel}: gps {other:?}, expected {lat},{lon}")),
            }
        } else if m.gps.is_some() {
            failures.push(format!("{rel}: unexpected gps"));
        }
        let screenshot = item["category"] == "screenshots" && !rel.contains("Screenshot_") && !rel.contains("Recording") && !rel.contains("RPReplay");
        if screenshot != m.screenshot_marker {
            failures.push(format!("{rel}: screenshot marker {}", m.screenshot_marker));
        }
        if let Some(group) = item["group"].as_str() {
            if let Some(id) = group.strip_prefix("live:") {
                if m.content_id.as_deref() != Some(id) {
                    failures.push(format!("{rel}: content id {:?}, expected {id}", m.content_id));
                }
            }
            if let Some(id) = group.strip_prefix("burst:") {
                if m.burst_id.as_deref() != Some(id) {
                    failures.push(format!("{rel}: burst id {:?}", m.burst_id));
                }
            }
        }
        if flags.contains(&"rotated") && m.rotation != Some(90) {
            failures.push(format!("{rel}: rotation {:?}", m.rotation));
        }
        if m.width.is_none() || m.height.is_none() {
            failures.push(format!("{rel}: no size"));
        }
    }
    assert!(checked > 800, "only {checked} fixture files read");
    assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
}
