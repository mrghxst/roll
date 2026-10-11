//! Generates Roll's fixture library and its ground-truth manifest.
//!
//! ```text
//! roll-fixtures [--out DIR] [--stress N] [--stress-only]
//! ```
//!
//! Needs FFmpeg with libx264 and libx265 (`ROLL_FFMPEG` or `PATH`) and
//! ExifTool (`ROLL_EXIFTOOL`, `PATH`, or the Windows per-user install).

mod exifblock;
mod geonames;
mod heif;
mod library;
mod manifest;
mod pixels;
mod stress;
mod tracks;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

fn find_ffmpeg() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ROLL_FFMPEG").map(PathBuf::from) {
        return Some(p);
    }
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(name)).find(|p| p.is_file())
}

fn main() -> Result<()> {
    let mut out = PathBuf::from("fixtures-out");
    let mut stress_n = 0usize;
    let mut library = true;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out = PathBuf::from(args.next().context("--out needs a folder")?),
            "--stress" => stress_n = args.next().context("--stress needs a count")?.parse()?,
            "--stress-only" => library = false,
            "-h" | "--help" => {
                println!("roll-fixtures [--out DIR] [--stress N] [--stress-only]");
                return Ok(());
            }
            other => bail!("unknown argument {other}"),
        }
    }
    std::fs::create_dir_all(&out)?;
    let out = std::path::absolute(&out)?;

    if library {
        let ffmpeg = find_ffmpeg().context("FFmpeg was not found. Set ROLL_FFMPEG or add it to PATH.")?;
        let exiftool = roll_meta::exiftool::locate().context("ExifTool was not found. Set ROLL_EXIFTOOL or add it to PATH.")?;
        for sub in ["library", "location-history", "camera-card"] {
            let p = out.join(sub);
            if p.exists() {
                clear_readonly(&p);
                std::fs::remove_dir_all(&p).with_context(|| format!("removing old {}", p.display()))?;
            }
        }
        let started = std::time::Instant::now();
        let mut g = library::Gen::new(out.clone(), ffmpeg, &exiftool)?;
        library::generate(&mut g)?;
        let manifest = g.finish()?;
        geonames::write(&out.join("geonames"))?;
        std::fs::write(out.join("manifest.json"), serde_json::to_string_pretty(&manifest)?)?;
        println!("Wrote {} files to {} in {:.1} s", manifest.items.len(), out.display(), started.elapsed().as_secs_f32());
    }
    if stress_n > 0 {
        let dir = out.join("stress");
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        let started = std::time::Instant::now();
        stress::generate(&dir, stress_n)?;
        println!("Wrote {stress_n} stress files to {} in {:.1} s", dir.display(), started.elapsed().as_secs_f32());
    }
    Ok(())
}

/// Read-only fixtures must be made writable before the folder is replaced.
fn clear_readonly(dir: &std::path::Path) {
    for entry in walk(dir) {
        if let Ok(meta) = std::fs::metadata(&entry) {
            let mut perm = meta.permissions();
            if perm.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perm.set_readonly(false);
                let _ = std::fs::set_permissions(&entry, perm);
            }
        }
    }
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}
