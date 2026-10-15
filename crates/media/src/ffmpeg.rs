//! FFmpeg and ffprobe as helper processes: frames from videos and from
//! formats the image crate does not read (HEIC, AVIF, JPEG XL), and
//! playback proxies for codecs the WebView cannot play.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use image::RgbImage;

#[derive(Debug, thiserror::Error)]
pub enum FfmpegError {
    #[error("FFmpeg was not found")]
    NotFound,
    #[error("FFmpeg could not read this file: {0}")]
    Failed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn exe_name(base: &str) -> String {
    if cfg!(windows) { format!("{base}.exe") } else { base.to_string() }
}

/// Where an installed Roll keeps the tools it ships: next to the program
/// (Windows), in the app's Resources (macOS), or in `lib/Roll` beside
/// `bin` (Linux packages and the AppImage).
pub fn bundle_dirs() -> Vec<PathBuf> {
    let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) else { return Vec::new() };
    let mut out = vec![dir.clone(), dir.join("resources"), dir.join("../Resources")];
    for name in ["Roll", "roll"] {
        out.push(dir.join("../lib").join(name));
    }
    out
}

/// `ROLL_FFMPEG`, the copy Roll ships, `PATH`, or the usual install folders
/// (apps started from the Dock or a menu do not get Homebrew's `PATH`).
pub fn locate(base: &str) -> Option<PathBuf> {
    let env = format!("ROLL_{}", base.to_uppercase());
    if let Some(p) = std::env::var_os(env).map(PathBuf::from)
        && p.is_file()
    {
        return Some(p);
    }
    let name = exe_name(base);
    for dir in bundle_dirs() {
        for c in [dir.join(&name), dir.join("ffmpeg").join(&name)] {
            if c.is_file() {
                return Some(c);
            }
        }
    }
    let system: &[&str] = if cfg!(windows) { &[] } else { &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] };
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .chain(system.iter().map(PathBuf::from))
        .map(|d| d.join(&name))
        .find(|p| p.is_file())
}

#[derive(Debug, Clone)]
pub struct Ffmpeg {
    pub ffmpeg: PathBuf,
}

fn command(exe: &Path) -> Command {
    let mut cmd = Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.stdin(Stdio::null());
    cmd
}

/// Scale so the long side is at most `max` (keeps even sizes for encoders).
fn scale_filter(max: u32) -> String {
    format!("scale='if(gt(iw,ih),min(iw,{max}),-2)':'if(gt(iw,ih),-2,min(ih,{max}))':flags=bilinear")
}

impl Ffmpeg {
    pub fn locate() -> Option<Self> {
        locate("ffmpeg").map(|ffmpeg| Self { ffmpeg })
    }

    /// One frame as RGB, at `at_secs` for videos (0 for still images).
    /// FFmpeg applies HEIF transforms and video rotation itself.
    pub fn frame(&self, path: &Path, at_secs: f64, max_side: Option<u32>) -> Result<RgbImage, FfmpegError> {
        let mut cmd = command(&self.ffmpeg);
        cmd.args(["-v", "error", "-nostdin"]);
        if at_secs > 0.0 {
            cmd.args(["-ss", &format!("{at_secs:.3}")]);
        }
        cmd.arg("-i").arg(path).args(["-frames:v", "1", "-an"]);
        if let Some(m) = max_side {
            cmd.args(["-vf", &scale_filter(m)]);
        }
        let out = cmd
            .args(["-f", "image2pipe", "-c:v", "bmp", "-pix_fmt", "bgr24", "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()?;
        if out.stdout.is_empty() {
            return Err(FfmpegError::Failed(String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("no frame").to_string()));
        }
        let img =
            image::load_from_memory_with_format(&out.stdout, image::ImageFormat::Bmp).map_err(|e| FfmpegError::Failed(e.to_string()))?;
        Ok(img.to_rgb8())
    }

    /// A poster frame at 10% of the duration, skipping black frames.
    pub fn poster(&self, path: &Path, duration_ms: Option<u64>, max_side: Option<u32>) -> Result<RgbImage, FfmpegError> {
        let dur = duration_ms.unwrap_or(0) as f64 / 1000.0;
        let mut last = None;
        for frac in [0.1, 0.3, 0.5, 0.0] {
            let img = self.frame(path, dur * frac, max_side)?;
            if crate::stats::mean_luma(&img) > 0.06 {
                return Ok(img);
            }
            last = Some(img);
        }
        last.ok_or_else(|| FfmpegError::Failed("no frame".into()))
    }

    /// Three keyframes (25%, 50%, 75%) for AI and hover scrubbing.
    pub fn keyframes(&self, path: &Path, duration_ms: Option<u64>, max_side: u32) -> Result<Vec<RgbImage>, FfmpegError> {
        let dur = duration_ms.unwrap_or(0) as f64 / 1000.0;
        [0.25, 0.5, 0.75].iter().map(|f| self.frame(path, dur * f, Some(max_side))).collect()
    }

    /// Transcode a playback proxy (H.264 through a hardware or system
    /// encoder, else VP9) that the WebView can play. Returns its MIME type.
    pub fn proxy(&self, input: &Path, output: &Path) -> Result<&'static str, FfmpegError> {
        let encoders: &[&[&str]] = &[
            &["-c:v", "h264_nvenc", "-preset", "p4", "-cq", "26"],
            &["-c:v", "h264_qsv", "-global_quality", "26"],
            &["-c:v", "h264_amf", "-quality", "balanced"],
            &["-c:v", "h264_mf", "-b:v", "6M"],
            &["-c:v", "h264_videotoolbox", "-b:v", "6M"],
        ];
        let mp4 = output.with_extension("mp4");
        for enc in encoders {
            let status = command(&self.ffmpeg)
                .args(["-v", "error", "-nostdin", "-y", "-i"])
                .arg(input)
                .args(["-vf", &scale_filter(1920), "-pix_fmt", "yuv420p"])
                .args(*enc)
                .args(["-c:a", "aac", "-b:a", "160k", "-movflags", "+faststart"])
                .arg(&mp4)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if status.success() {
                std::fs::rename(&mp4, output)?;
                return Ok("video/mp4");
            }
        }
        let webm = output.with_extension("webm");
        let status = command(&self.ffmpeg)
            .args(["-v", "error", "-nostdin", "-y", "-i"])
            .arg(input)
            .args([
                "-vf",
                &scale_filter(1920),
                "-c:v",
                "libvpx-vp9",
                "-b:v",
                "0",
                "-crf",
                "33",
                "-row-mt",
                "1",
                "-deadline",
                "realtime",
                "-cpu-used",
                "8",
                "-c:a",
                "libopus",
            ])
            .arg(&webm)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()?;
        if !status.status.success() {
            return Err(FfmpegError::Failed(String::from_utf8_lossy(&status.stderr).lines().next().unwrap_or("encode failed").to_string()));
        }
        std::fs::rename(&webm, output)?;
        Ok("video/webm")
    }
}
