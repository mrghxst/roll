//! One-click local AI (PROMPT M7): llama.cpp's server with Qwen3-VL 4B,
//! downloaded as the `local-ai` pack (the build for this system) and
//! started by Roll when a decision model is first needed. It listens on
//! 127.0.0.1 only and stops after 10 minutes without requests, or when
//! Roll closes, so it frees the graphics card's memory.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

pub const PACK: &str = "local-ai";
pub const PORT: u16 = 8766;
pub const BASE: &str = "http://127.0.0.1:8766/v1";
/// The name the server gives the model.
pub const MODEL: &str = "qwen3-vl-4b-instruct";
const IDLE: Duration = Duration::from_secs(600);
const LOAD: Duration = Duration::from_secs(300);

#[derive(Default)]
struct State {
    child: Option<Child>,
    last_used: Option<Instant>,
}

/// The server Roll runs. Cheap to clone.
#[derive(Clone, Default)]
pub struct Server {
    state: Arc<Mutex<State>>,
}

/// The engine archive in a pack folder (a `.zip` on Windows, `.tar.gz`
/// elsewhere), unpacked into `engine/`.
pub fn unpack(dir: &Path) -> anyhow::Result<()> {
    let archive = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("engine.")))
        .ok_or_else(|| anyhow::anyhow!("the server archive is missing"))?;
    let out = dir.join("engine");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out)?;
    if archive.extension().and_then(|e| e.to_str()) == Some("zip") {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&archive)?)?;
        zip.extract(&out)?;
    } else {
        // macOS and Linux always have tar.
        let ok = Command::new("tar").arg("-xzf").arg(&archive).arg("-C").arg(&out).status()?.success();
        anyhow::ensure!(ok, "the server archive could not be unpacked");
    }
    let exe = server_exe(dir).ok_or_else(|| anyhow::anyhow!("llama-server is not in the archive"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))?;
    }
    let _ = exe;
    Ok(())
}

/// `llama-server` somewhere under `engine/`.
fn server_exe(dir: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    walkdir::WalkDir::new(dir.join("engine")).into_iter().flatten().find(|e| e.file_name() == name).map(|e| e.into_path())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_millis(800))).build().into()
}

fn healthy() -> bool {
    agent().get(format!("http://127.0.0.1:{PORT}/health")).call().is_ok_and(|r| r.status() == 200)
}

/// Whether the server is answering a request right now (a run sends its
/// requests straight to it).
fn working() -> bool {
    let Ok(mut r) = agent().get(format!("http://127.0.0.1:{PORT}/slots")).call() else { return false };
    let Ok(v) = r.body_mut().read_json::<serde_json::Value>() else { return false };
    v.as_array().is_some_and(|slots| slots.iter().any(|s| s.get("is_processing").and_then(serde_json::Value::as_bool) == Some(true)))
}

impl Server {
    /// Start the server from the pack in `dir` unless it runs; waits until
    /// the model is loaded.
    pub fn ensure(&self, dir: &Path, log: &Path) -> anyhow::Result<()> {
        let mut st = self.state.lock();
        st.last_used = Some(Instant::now());
        let running = st.child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)));
        if running && healthy() {
            return Ok(());
        }
        // One left from an earlier run of Roll answers on the port too.
        if !running && healthy() {
            return Ok(());
        }
        if let Some(mut c) = st.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        let exe = server_exe(dir)
            .ok_or_else(|| anyhow::anyhow!("The local AI is not installed completely. Download it again in Settings › Models and data."))?;
        let model = dir.join("model.gguf");
        let mmproj = dir.join("mmproj.gguf");
        let logfile = std::fs::File::create(log)?;
        let mut cmd = Command::new(&exe);
        cmd.current_dir(exe.parent().unwrap_or(dir))
            .arg("-m")
            .arg(&model)
            .arg("--mmproj")
            .arg(&mmproj)
            .args(["--alias", MODEL, "--host", "127.0.0.1", "--port", &PORT.to_string()])
            // Every layer on the graphics card when there is one; two
            // requests at a time with 8k tokens each.
            .args(["-ngl", "99", "-c", "16384", "-np", "2", "--no-webui"])
            .stdin(Stdio::null())
            .stdout(logfile.try_clone()?)
            .stderr(logfile);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn()?;
        let start = Instant::now();
        loop {
            if healthy() {
                break;
            }
            if let Ok(Some(status)) = child.try_wait() {
                anyhow::bail!("The local AI stopped while starting ({status}). Its log is {}.", log.display());
            }
            if start.elapsed() > LOAD {
                let _ = child.kill();
                anyhow::bail!("The local AI did not start within {} minutes. Its log is {}.", LOAD.as_secs() / 60, log.display());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        tracing::info!("local AI started in {:.1} s", start.elapsed().as_secs_f32());
        st.child = Some(child);
        drop(st);
        self.watch_idle();
        Ok(())
    }

    /// Note a request, so the idle timer starts again.
    pub fn touch(&self) {
        self.state.lock().last_used = Some(Instant::now());
    }

    fn watch_idle(&self) {
        let me = self.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(30));
                if working() {
                    me.touch();
                    continue;
                }
                let mut st = me.state.lock();
                if st.child.is_none() {
                    return;
                }
                if st.last_used.is_some_and(|t| t.elapsed() > IDLE) {
                    if let Some(mut c) = st.child.take() {
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                    tracing::info!("local AI stopped after {} minutes without requests", IDLE.as_secs() / 60);
                    return;
                }
            }
        });
    }

    pub fn running(&self) -> bool {
        self.state.lock().child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    pub fn stop(&self) {
        if let Some(mut c) = self.state.lock().child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn unpacks_the_engine_zip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(tmp.path().join("engine.zip")).unwrap());
        zip.start_file("llama-server.exe", zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut zip, b"MZ").unwrap();
        zip.finish().unwrap();
        unpack(tmp.path()).unwrap();
        assert_eq!(server_exe(tmp.path()), Some(tmp.path().join("engine").join("llama-server.exe")));
    }

    #[test]
    fn one_pack_per_id_on_this_system() {
        let cat = crate::packs::Catalog::bundled();
        let here = crate::packs::this_platform();
        let known = ["windows-x86_64", "linux-x86_64", "macos-aarch64", "macos-x86_64"];
        if known.contains(&here.as_str()) {
            let def = cat.get(PACK).unwrap();
            assert_eq!(def.platform.as_deref(), Some(here.as_str()));
            assert!(def.files.iter().all(|f| f.sha256.is_some()));
        }
        let root = tempfile::tempdir().unwrap();
        let packs = crate::packs::Packs::new(root.path().to_path_buf(), cat, Arc::new(|| {}));
        let ids: Vec<String> = packs.list().into_iter().map(|p| p.id).collect();
        let mut unique = ids.clone();
        unique.dedup();
        assert_eq!(ids, unique);
    }
}
