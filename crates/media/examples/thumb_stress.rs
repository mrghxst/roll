//! Decode and thumbnail every fixture image many times in parallel, to
//! reproduce rare crashes. `cargo run -p roll-media --example thumb_stress
//! -- <rounds> <stage>` with stage `decode`, `resize` or `all`.

use std::path::{Path, PathBuf};

use rayon::prelude::*;
use roll_media::decode::{self, Source};
use roll_media::thumb;

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rounds: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let stage = args.get(2).cloned().unwrap_or_else(|| "all".into());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out/library");
    let mut files = Vec::new();
    walk(&root, &mut files);
    let files: Vec<(PathBuf, &str)> = files
        .into_iter()
        .filter_map(|p| {
            let ext = p.extension()?.to_string_lossy().to_lowercase();
            let f = match ext.as_str() {
                "jpg" | "jpeg" => "jpeg",
                "png" => "png",
                "gif" => "gif",
                "webp" => "webp",
                "tif" | "tiff" => "tiff",
                "bmp" => "bmp",
                "nef" | "cr2" | "arw" | "dng" => "raw",
                _ => return None,
            };
            Some((p, f))
        })
        .collect();
    eprintln!("{} files, {rounds} rounds, stage {stage}", files.len());
    for r in 0..rounds {
        files.par_iter().for_each(|(p, f)| {
            let src = Source { path: p, format: f, video: false, orientation: None, duration_ms: None };
            let Ok(img) = decode::decode(src, None, Some(1024)) else { return };
            match stage.as_str() {
                "decode" => {}
                "resize" => {
                    let (w, h) = thumb::thumb_size(img.width(), img.height());
                    let _ = thumb::resize(&img, w, h);
                }
                _ => {
                    let _ = thumb::make(&img);
                }
            }
        });
        eprintln!("round {r} ok");
    }
}
