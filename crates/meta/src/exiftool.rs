//! ExifTool in `-stay_open` mode. Arguments go through stdin as an argument
//! file, one per line, in UTF-8. On Windows `-charset filename=utf8` is
//! required or non-ASCII paths break.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum ExifToolError {
    #[error("ExifTool was not found")]
    NotFound,
    #[error("ExifTool could not start: {0}")]
    Spawn(std::io::Error),
    #[error("ExifTool stopped responding")]
    Timeout,
    #[error("ExifTool exited unexpectedly")]
    Exited,
    #[error("ExifTool: {0}")]
    Failed(String),
    #[error("ExifTool returned unreadable output: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, ExifToolError>;

/// Where Homebrew and Linux packages put command-line tools.
pub const SYSTEM_DIRS: &[&str] = if cfg!(windows) { &[] } else { &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] };

/// Finds ExifTool: `ROLL_EXIFTOOL`, next to the app, on `PATH`, the
/// usual install folders (apps started from the Dock or a menu do not get
/// Homebrew's `PATH`), or the per-user Windows install location.
pub fn locate() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ROLL_EXIFTOOL").map(PathBuf::from)
        && p.is_file()
    {
        return Some(p);
    }
    let exe_name = if cfg!(windows) { "exiftool.exe" } else { "exiftool" };
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for cand in [dir.join(exe_name), dir.join("exiftool").join(exe_name), dir.join("resources").join("exiftool").join(exe_name)] {
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths).chain(SYSTEM_DIRS.iter().map(PathBuf::from)) {
            let cand = dir.join(exe_name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    if cfg!(windows)
        && let Some(local) = std::env::var_os("LOCALAPPDATA")
    {
        let cand = PathBuf::from(local).join("Programs").join("ExifTool").join("ExifTool.exe");
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

pub struct ExifTool {
    child: Child,
    stdin: ChildStdin,
    out: Receiver<String>,
    err: Receiver<String>,
    seq: u32,
}

const TIMEOUT: Duration = Duration::from_secs(120);

impl ExifTool {
    pub fn spawn(exe: &Path) -> Result<Self> {
        let mut cmd = Command::new(exe);
        cmd.args(["-stay_open", "True", "-@", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(ExifToolError::Spawn)?;
        let stdin = child.stdin.take().ok_or(ExifToolError::Exited)?;
        let out = pump(child.stdout.take().ok_or(ExifToolError::Exited)?);
        let err = pump(child.stderr.take().ok_or(ExifToolError::Exited)?);
        Ok(Self { child, stdin, out, err, seq: 0 })
    }

    /// Run one command. Arguments must not contain line breaks unless they
    /// are passed through [`arg`], which escapes them.
    pub fn execute(&mut self, args: &[String]) -> Result<Output> {
        self.seq += 1;
        let marker = format!("{{ready{}}}", self.seq);
        let mut script = String::new();
        if cfg!(windows) {
            script.push_str("-charset\nfilename=utf8\n");
        }
        for a in args {
            script.push_str(a);
            script.push('\n');
        }
        script.push_str(&format!("-echo4\n{marker}\n-execute{}\n", self.seq));
        self.stdin.write_all(script.as_bytes()).map_err(|_| ExifToolError::Exited)?;
        self.stdin.flush().map_err(|_| ExifToolError::Exited)?;
        let stdout = collect(&self.out, &marker)?;
        let stderr = collect(&self.err, &marker)?;
        Ok(Output { stdout, stderr })
    }

    /// Read tags as JSON (`-j -n -G1`), one object per file.
    pub fn read_json(&mut self, files: &[&Path], extra: &[&str]) -> Result<Vec<Value>> {
        let mut args: Vec<String> = vec!["-j".into(), "-n".into(), "-G1".into(), "-struct".into()];
        args.extend(extra.iter().map(|s| s.to_string()));
        args.extend(files.iter().map(|f| f.to_string_lossy().into_owned()));
        let out = self.execute(&args)?;
        if out.stdout.trim().is_empty() {
            return Err(ExifToolError::Failed(first_error(&out.stderr).unwrap_or("no output").to_string()));
        }
        serde_json::from_str(&out.stdout).map_err(|e| ExifToolError::Parse(e.to_string()))
    }

    /// Write tags. Fails when ExifTool reports an error or wrote nothing.
    pub fn write(&mut self, file: &Path, assignments: &[String]) -> Result<()> {
        let mut args: Vec<String> = vec!["-overwrite_original_in_place".into(), "-P".into()];
        args.extend(assignments.iter().cloned());
        args.push(file.to_string_lossy().into_owned());
        let out = self.execute(&args)?;
        if let Some(e) = first_error(&out.stderr) {
            return Err(ExifToolError::Failed(e.to_string()));
        }
        if !out.stdout.contains("1 image files updated") && !out.stdout.contains("1 image files unchanged") {
            return Err(ExifToolError::Failed(out.stdout.trim().to_string()));
        }
        Ok(())
    }

    pub fn version(&mut self) -> Result<String> {
        Ok(self.execute(&["-ver".into()])?.stdout.trim().to_string())
    }
}

impl Drop for ExifTool {
    fn drop(&mut self) {
        let _ = self.stdin.write_all(b"-stay_open\nFalse\n");
        let _ = self.stdin.flush();
        for _ in 0..20 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.child.kill();
    }
}

fn pump(stream: impl std::io::Read + Send + 'static) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

fn collect(rx: &Receiver<String>, marker: &str) -> Result<String> {
    let mut buf = String::new();
    loop {
        match rx.recv_timeout(TIMEOUT) {
            Ok(line) if line.trim_end() == marker => return Ok(buf),
            Ok(line) => {
                buf.push_str(&line);
                buf.push('\n');
            }
            Err(mpsc::RecvTimeoutError::Timeout) => return Err(ExifToolError::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(ExifToolError::Exited),
        }
    }
}

fn first_error(stderr: &str) -> Option<&str> {
    stderr.lines().map(str::trim).find(|l| l.starts_with("Error"))
}

/// Escape one argument for the argument file. Values with line breaks use
/// ExifTool's `#[CSTR]` C-string syntax.
pub fn arg(s: &str) -> String {
    if !s.contains(['\n', '\r']) {
        return s.to_string();
    }
    let mut out = String::from("#[CSTR]");
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
    out
}

/// A small pool of stay-open processes, created on demand.
pub struct ExifToolPool {
    exe: PathBuf,
    idle: Mutex<Vec<ExifTool>>,
}

impl ExifToolPool {
    pub fn new(exe: PathBuf) -> Self {
        Self { exe, idle: Mutex::new(Vec::new()) }
    }

    pub fn locate() -> Result<Self> {
        locate().map(Self::new).ok_or(ExifToolError::NotFound)
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    pub fn with<T>(&self, f: impl FnOnce(&mut ExifTool) -> Result<T>) -> Result<T> {
        let tool = self.idle.lock().pop();
        let mut tool = match tool {
            Some(t) => t,
            None => ExifTool::spawn(&self.exe)?,
        };
        let res = f(&mut tool);
        // A process that timed out or died is dropped, not reused.
        if !matches!(res, Err(ExifToolError::Timeout | ExifToolError::Exited)) {
            self.idle.lock().push(tool);
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_line_breaks() {
        assert_eq!(arg("plain"), "plain");
        assert_eq!(arg("-Description=a\nb"), "#[CSTR]-Description=a\\nb");
    }
}
