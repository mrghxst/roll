//! Downloadable packs: place names and models, listed in `packs.json` with
//! URL, size, SHA-256 and license. A pack downloads only when the owner asks
//! for it and downloads are on in Settings › Network. Files land in
//! `<data dir>/<pack dir>/` as `.part` first, resume after a stop, and are
//! kept only when their size and SHA-256 match. GeoNames files change daily
//! at the source, so they carry no checksum; their size is capped and the
//! place-name build checks their format instead.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    pub packs: Vec<PackDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackDef {
    pub id: String,
    pub title: String,
    pub purpose: String,
    pub license: String,
    pub source: String,
    pub attribution: String,
    /// Relative to the data dir.
    pub dir: String,
    /// A step after the download ("places" builds the gazetteer).
    pub after: Option<String>,
    /// Only for this system (`windows-x86_64`, `macos-aarch64`, …); a pack
    /// can be listed once per system under one id.
    #[serde(default)]
    pub platform: Option<String>,
    pub files: Vec<PackFile>,
}

/// This system as packs name it: OS and CPU, like `windows-x86_64`.
pub fn this_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

impl PackDef {
    fn fits(&self, platform: &str) -> bool {
        self.platform.as_deref().is_none_or(|p| p == platform)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackFile {
    /// Relative to the pack dir.
    pub path: String,
    pub url: String,
    pub size: u64,
    /// `None` for files that change at the source.
    pub sha256: Option<String>,
}

impl PackDef {
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

impl Catalog {
    /// The catalog shipped with the app.
    pub fn bundled() -> Self {
        serde_json::from_str(include_str!("../../../packs.json")).expect("packs.json is valid")
    }

    pub fn get(&self, id: &str) -> Option<&PackDef> {
        let here = this_platform();
        self.packs.iter().find(|p| p.id == id && p.fits(&here))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum PackState {
    Available,
    Downloading,
    /// Building or checking after the download.
    Installing,
    Installed,
    /// Installed, but `packs.json` now lists other files.
    Outdated,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PackStatus {
    pub id: String,
    pub title: String,
    pub purpose: String,
    pub license: String,
    pub source: String,
    pub attribution: String,
    /// Hosts the files come from.
    pub hosts: Vec<String>,
    #[ts(type = "number")]
    pub size: u64,
    /// Bytes on disk so far (also for a stopped download).
    #[ts(type = "number")]
    pub done: u64,
    pub state: PackState,
    pub error: Option<String>,
    /// False when the source has no fixed checksum.
    pub verified: bool,
}

/// Written last; a pack is installed when this exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    id: String,
    files: Vec<PackFile>,
    installed_at: i64,
}

struct Live {
    state: PackState,
    done: u64,
    error: Option<String>,
    cancel: Arc<AtomicBool>,
}

pub type AfterStep = Box<dyn FnOnce(&Path) -> anyhow::Result<()> + Send>;

pub struct Packs {
    root: PathBuf,
    catalog: Catalog,
    live: Arc<Mutex<HashMap<String, Live>>>,
    notify: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Debug, thiserror::Error)]
enum Stop {
    #[error("stopped")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}

impl Packs {
    pub fn new(root: PathBuf, catalog: Catalog, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { root, catalog, live: Arc::default(), notify }
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn dir(&self, id: &str) -> Option<PathBuf> {
        self.catalog.get(id).map(|p| self.root.join(&p.dir))
    }

    /// The pack's folder when it is installed and current.
    pub fn installed(&self, id: &str) -> Option<PathBuf> {
        let st = self.status(id)?;
        let def = self.catalog.get(id)?;
        (st.state == PackState::Installed).then(|| self.root.join(&def.dir))
    }

    pub fn list(&self) -> Vec<PackStatus> {
        let here = this_platform();
        self.catalog.packs.iter().filter(|p| p.fits(&here)).filter_map(|p| self.status(&p.id)).collect()
    }

    pub fn status(&self, id: &str) -> Option<PackStatus> {
        let def = self.catalog.get(id)?;
        let dir = self.root.join(&def.dir);
        let live = self.live.lock();
        let (state, done, error) = match live.get(id) {
            Some(l) if matches!(l.state, PackState::Downloading | PackState::Installing | PackState::Failed) => {
                (l.state, l.done, l.error.clone())
            }
            _ => match read_manifest(&dir) {
                Some(m) if m.id == def.id && same_files(&m.files, &def.files) => (PackState::Installed, def.size(), None),
                Some(_) => (PackState::Outdated, on_disk(&dir, def), None),
                None => (PackState::Available, on_disk(&dir, def), None),
            },
        };
        let mut hosts: Vec<String> = def.files.iter().filter_map(|f| host(&f.url)).collect();
        hosts.dedup();
        Some(PackStatus {
            id: def.id.clone(),
            title: def.title.clone(),
            purpose: def.purpose.clone(),
            license: def.license.clone(),
            source: def.source.clone(),
            attribution: def.attribution.clone(),
            hosts,
            size: def.size(),
            done,
            state,
            error,
            verified: def.files.iter().all(|f| f.sha256.is_some()),
        })
    }

    /// Start downloading `id` on its own thread. `after` runs once every file
    /// is in place, before the pack counts as installed.
    pub fn download(&self, id: &str, after: Option<AfterStep>) -> anyhow::Result<()> {
        let def = self.catalog.get(id).ok_or_else(|| anyhow::anyhow!("Unknown pack {id}"))?.clone();
        let dir = self.root.join(&def.dir);
        fs::create_dir_all(&dir)?;
        let need = def.size().saturating_sub(on_disk(&dir, &def));
        if let Ok(free) = crate::fsops::free_space(&dir)
            && free < need + 200 * 1024 * 1024
        {
            anyhow::bail!("Not enough free space: {} needed, {} free", human(need + 200 * 1024 * 1024), human(free));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut live = self.live.lock();
            if live.get(id).is_some_and(|l| matches!(l.state, PackState::Downloading | PackState::Installing)) {
                return Ok(());
            }
            live.insert(
                id.to_string(),
                Live { state: PackState::Downloading, done: on_disk(&dir, &def), error: None, cancel: cancel.clone() },
            );
        }
        (self.notify)();
        let live = self.live.clone();
        let notify = self.notify.clone();
        std::thread::Builder::new().name(format!("pack {id}")).spawn(move || {
            let result = run(&def, &dir, &cancel, &live, &notify, after);
            let mut l = live.lock();
            match result {
                Ok(()) => {
                    l.remove(&def.id);
                    tracing::info!("pack {} installed", def.id);
                }
                Err(Stop::Cancelled) => {
                    l.remove(&def.id);
                }
                Err(Stop::Failed(e)) => {
                    tracing::warn!("pack {}: {e}", def.id);
                    if let Some(x) = l.get_mut(&def.id) {
                        x.state = PackState::Failed;
                        x.error = Some(e);
                    }
                }
            }
            drop(l);
            notify();
        })?;
        Ok(())
    }

    /// Stop a download; the part on disk stays so the next download
    /// continues from there.
    pub fn cancel(&self, id: &str) {
        let mut live = self.live.lock();
        if let Some(l) = live.get(id) {
            l.cancel.store(true, Ordering::Relaxed);
            if l.state == PackState::Failed {
                live.remove(id);
            }
        }
        drop(live);
        (self.notify)();
    }

    /// Delete the pack's files.
    pub fn remove(&self, id: &str) -> anyhow::Result<()> {
        let def = self.catalog.get(id).ok_or_else(|| anyhow::anyhow!("Unknown pack {id}"))?;
        if self.live.lock().get(id).is_some_and(|l| matches!(l.state, PackState::Downloading | PackState::Installing)) {
            anyhow::bail!("Stop the download first");
        }
        let dir = self.root.join(&def.dir);
        anyhow::ensure!(dir.starts_with(&self.root) && dir != self.root, "pack folder outside the data folder");
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        self.live.lock().remove(id);
        (self.notify)();
        Ok(())
    }
}

fn run(
    def: &PackDef,
    dir: &Path,
    cancel: &AtomicBool,
    live: &Mutex<HashMap<String, Live>>,
    notify: &Arc<dyn Fn() + Send + Sync>,
    after: Option<AfterStep>,
) -> Result<(), Stop> {
    let fail = |e: String| Stop::Failed(e);
    // A stale manifest must not mark a half-replaced pack as installed.
    let _ = fs::remove_file(dir.join("pack.json"));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .user_agent(concat!("Roll/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut finished = 0u64;
    let mut last_emit = Instant::now();
    let mut installed = Vec::new();
    for f in &def.files {
        let dest = dir.join(&f.path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| fail(e.to_string()))?;
        }
        let mut progress = |n: u64| {
            if let Some(l) = live.lock().get_mut(&def.id) {
                l.done = finished + n;
            }
            if last_emit.elapsed() > Duration::from_millis(250) {
                last_emit = Instant::now();
                notify();
            }
        };
        let sha = fetch(&agent, f, &dest, cancel, &mut progress)?;
        finished += fs::metadata(&dest).map(|m| m.len()).unwrap_or(f.size);
        installed.push(PackFile { sha256: Some(sha), size: fs::metadata(&dest).map(|m| m.len()).unwrap_or(f.size), ..f.clone() });
    }
    if let Some(after) = after {
        if let Some(l) = live.lock().get_mut(&def.id) {
            l.state = PackState::Installing;
        }
        notify();
        after(dir).map_err(|e| fail(format!("{e:#}")))?;
    }
    let manifest = Manifest { id: def.id.clone(), files: installed, installed_at: crate::db::now_ms() };
    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| fail(e.to_string()))?;
    fs::write(dir.join("pack.json"), json).map_err(|e| fail(e.to_string()))?;
    Ok(())
}

/// Download one file to `dest` through `dest.part`, continuing a part that
/// is there. Returns the file's SHA-256.
fn fetch(agent: &ureq::Agent, f: &PackFile, dest: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(u64)) -> Result<String, Stop> {
    let fail = |e: String| Stop::Failed(format!("{}: {e}", f.path));
    // Already here from an earlier, interrupted run.
    if let Some(want) = &f.sha256
        && fs::metadata(dest).is_ok_and(|m| m.len() == f.size)
        && hash_file(dest).is_ok_and(|h| &h == want)
    {
        progress(f.size);
        return Ok(want.clone());
    }
    let part = PathBuf::from(format!("{}.part", dest.display()));
    let limit = match f.sha256 {
        Some(_) => f.size,
        None => f.size.saturating_mul(4) + (1 << 20),
    };
    let mut have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    // Files without a checksum may have changed at the source; start over.
    if have > limit || f.sha256.is_none() {
        have = 0;
    }
    let mut hasher = Sha256::new();
    if have > 0 {
        let r = File::open(&part).map_err(|e| fail(e.to_string()))?.take(have);
        feed(&mut hasher, r).map_err(|e| fail(e.to_string()))?;
    }
    progress(have);
    if have < f.size || f.sha256.is_none() {
        let mut req = agent.get(&f.url);
        if have > 0 {
            req = req.header("Range", format!("bytes={have}-"));
        }
        let resp = req.call().map_err(|e| fail(e.to_string()))?;
        let status = resp.status().as_u16();
        let append = match status {
            206 if have > 0 => true,
            200 => {
                have = 0;
                hasher = Sha256::new();
                false
            }
            416 if have > 0 => {
                // The server has nothing past what we have; check it below.
                return finish(f, &part, dest, have, hasher, &fail);
            }
            s => return Err(fail(format!("the server answered {s}"))),
        };
        let mut out =
            OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(&part).map_err(|e| fail(e.to_string()))?;
        let mut body = resp.into_body().into_reader();
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Stop::Cancelled);
            }
            let n = body.read(&mut buf).map_err(|e| fail(e.to_string()))?;
            if n == 0 {
                break;
            }
            have += n as u64;
            if have > limit {
                drop(out);
                let _ = fs::remove_file(&part);
                return Err(fail("larger than expected; deleted".into()));
            }
            out.write_all(&buf[..n]).map_err(|e| fail(e.to_string()))?;
            hasher.update(&buf[..n]);
            progress(have);
        }
        out.flush().map_err(|e| fail(e.to_string()))?;
    }
    finish(f, &part, dest, have, hasher, &fail)
}

fn finish(f: &PackFile, part: &Path, dest: &Path, have: u64, hasher: Sha256, fail: &dyn Fn(String) -> Stop) -> Result<String, Stop> {
    let sha = hex(&hasher.finalize());
    if let Some(want) = &f.sha256 {
        if have != f.size || &sha != want {
            let _ = fs::remove_file(part);
            return Err(fail("the download didn't match its checksum and was deleted".into()));
        }
    } else if have == 0 {
        return Err(fail("the download was empty".into()));
    }
    fs::rename(part, dest).map_err(|e| fail(e.to_string()))?;
    Ok(sha)
}

fn hash_file(p: &Path) -> std::io::Result<String> {
    let mut h = Sha256::new();
    feed(&mut h, File::open(p)?)?;
    Ok(hex(&h.finalize()))
}

fn feed(h: &mut Sha256, mut r: impl Read) -> std::io::Result<()> {
    let mut buf = vec![0u8; 1 << 20];
    loop {
        match r.read(&mut buf)? {
            0 => return Ok(()),
            n => h.update(&buf[..n]),
        }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn read_manifest(dir: &Path) -> Option<Manifest> {
    serde_json::from_slice(&fs::read(dir.join("pack.json")).ok()?).ok()
}

/// Installed files match the catalog: same paths, and the same checksum
/// where the catalog has one.
fn same_files(installed: &[PackFile], listed: &[PackFile]) -> bool {
    installed.len() == listed.len()
        && listed
            .iter()
            .all(|l| installed.iter().any(|i| i.path == l.path && (l.sha256.is_none() || i.sha256 == l.sha256 && i.size == l.size)))
}

/// Bytes of the pack already on disk, finished or partial.
fn on_disk(dir: &Path, def: &PackDef) -> u64 {
    def.files
        .iter()
        .map(|f| {
            let p = dir.join(&f.path);
            fs::metadata(&p).or_else(|_| fs::metadata(format!("{}.part", p.display()))).map(|m| m.len().min(f.size)).unwrap_or(0)
        })
        .sum()
}

fn host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    Some(rest.split(['/', ':']).next()?.to_string())
}

/// Sizes as the UI shows them (1024-based, like Explorer).
pub fn human(bytes: u64) -> String {
    let b = bytes as f64;
    let (k, m, g) = (1024.0, 1024.0 * 1024.0, 1024.0 * 1024.0 * 1024.0);
    if b >= g {
        format!("{:.1} GB", b / g)
    } else if b >= m {
        format!("{:.0} MB", b / m)
    } else {
        format!("{:.0} KB", (b / k).max(1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;

    /// A tiny HTTP server for `files` that honours `Range` and counts
    /// requests. `slow` sleeps between chunks so a test can stop midway.
    fn serve(files: HashMap<String, Vec<u8>>, slow: bool) -> (String, Arc<AtomicUsize>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        std::thread::spawn(move || {
            for stream in l.incoming() {
                let Ok(mut s) = stream else { continue };
                h.fetch_add(1, Ordering::Relaxed);
                let files = files.clone();
                std::thread::spawn(move || {
                    let mut r = BufReader::new(s.try_clone().unwrap());
                    let mut line = String::new();
                    r.read_line(&mut line).unwrap();
                    let path = line.split_whitespace().nth(1).unwrap_or("/").trim_start_matches('/').to_string();
                    let mut from = 0usize;
                    loop {
                        let mut h = String::new();
                        if r.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                            break;
                        }
                        if let Some(v) = h.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            from = v.trim().trim_end_matches('-').parse().unwrap_or(0);
                        }
                    }
                    let Some(body) = files.get(&path) else {
                        let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        return;
                    };
                    if from >= body.len() && from > 0 {
                        let _ = s.write_all(b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        return;
                    }
                    let part = &body[from..];
                    let head = if from > 0 {
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {from}-{}/{}\r\nConnection: close\r\n\r\n",
                            part.len(),
                            body.len() - 1,
                            body.len()
                        )
                    } else {
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", part.len())
                    };
                    let _ = s.write_all(head.as_bytes());
                    for c in part.chunks(64 * 1024) {
                        if s.write_all(c).is_err() {
                            return;
                        }
                        if slow {
                            std::thread::sleep(Duration::from_millis(30));
                        }
                    }
                });
            }
        });
        (base, hits)
    }

    fn sha(b: &[u8]) -> String {
        hex(&Sha256::digest(b))
    }

    fn catalog(base: &str, files: &[(&str, &[u8], bool)]) -> Catalog {
        Catalog {
            packs: vec![PackDef {
                id: "test".into(),
                title: "Test".into(),
                purpose: String::new(),
                license: "MIT".into(),
                source: "here".into(),
                attribution: String::new(),
                dir: "models/test".into(),
                after: None,
                platform: None,
                files: files
                    .iter()
                    .map(|(name, body, pinned)| PackFile {
                        path: format!("sub/{name}"),
                        url: format!("{base}/{name}"),
                        size: body.len() as u64,
                        sha256: pinned.then(|| sha(body)),
                    })
                    .collect(),
            }],
        }
    }

    fn wait(p: &Packs) -> PackStatus {
        let t = Instant::now();
        loop {
            let s = p.status("test").unwrap();
            if !matches!(s.state, PackState::Downloading | PackState::Installing) {
                return s;
            }
            assert!(t.elapsed() < Duration::from_secs(30), "download hangs");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn body(n: usize, seed: u8) -> Vec<u8> {
        (0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
    }

    #[test]
    fn downloads_verifies_and_removes() {
        let a = body(300_000, 1);
        let b = b"hello".to_vec();
        let (base, _) = serve(HashMap::from([("a.onnx".into(), a.clone()), ("b.txt".into(), b.clone())]), false);
        let root = tempfile::tempdir().unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        let p = Packs::new(root.path().into(), catalog(&base, &[("a.onnx", &a, true), ("b.txt", &b, false)]), Arc::new(|| {}));
        assert_eq!(p.status("test").unwrap().state, PackState::Available);
        assert!(!p.status("test").unwrap().verified);
        let r = ran.clone();
        p.download(
            "test",
            Some(Box::new(move |dir| {
                assert!(dir.join("sub/b.txt").exists());
                r.store(true, Ordering::Relaxed);
                Ok(())
            })),
        )
        .unwrap();
        let s = wait(&p);
        assert_eq!(s.state, PackState::Installed, "{:?}", s.error);
        assert!(ran.load(Ordering::Relaxed));
        assert_eq!(fs::read(root.path().join("models/test/sub/a.onnx")).unwrap(), a);
        assert!(p.installed("test").is_some());
        p.remove("test").unwrap();
        assert!(!root.path().join("models/test").exists());
        assert_eq!(p.status("test").unwrap().state, PackState::Available);
    }

    #[test]
    fn rejects_a_wrong_checksum() {
        let a = body(100_000, 2);
        let (base, _) = serve(HashMap::from([("a.onnx".into(), a.clone())]), false);
        let root = tempfile::tempdir().unwrap();
        let mut c = catalog(&base, &[("a.onnx", &a, true)]);
        c.packs[0].files[0].sha256 = Some(sha(b"something else"));
        let p = Packs::new(root.path().into(), c, Arc::new(|| {}));
        p.download("test", None).unwrap();
        let s = wait(&p);
        assert_eq!(s.state, PackState::Failed);
        assert!(s.error.unwrap().contains("checksum"));
        assert!(!root.path().join("models/test/sub/a.onnx").exists());
        assert!(!root.path().join("models/test/sub/a.onnx.part").exists());
        assert!(!root.path().join("models/test/pack.json").exists());
    }

    #[test]
    fn a_failing_after_step_leaves_the_pack_uninstalled() {
        let a = body(1000, 3);
        let (base, _) = serve(HashMap::from([("a.onnx".into(), a.clone())]), false);
        let root = tempfile::tempdir().unwrap();
        let p = Packs::new(root.path().into(), catalog(&base, &[("a.onnx", &a, true)]), Arc::new(|| {}));
        p.download("test", Some(Box::new(|_| anyhow::bail!("not a gazetteer")))).unwrap();
        let s = wait(&p);
        assert_eq!(s.state, PackState::Failed);
        assert_eq!(s.error.as_deref(), Some("not a gazetteer"));
        p.cancel("test");
        assert_eq!(p.status("test").unwrap().state, PackState::Available);
    }

    #[test]
    fn stops_and_resumes_with_range() {
        let a = body(2_000_000, 4);
        let (base, hits) = serve(HashMap::from([("a.onnx".into(), a.clone())]), true);
        let root = tempfile::tempdir().unwrap();
        let p = Packs::new(root.path().into(), catalog(&base, &[("a.onnx", &a, true)]), Arc::new(|| {}));
        p.download("test", None).unwrap();
        let t = Instant::now();
        while p.status("test").unwrap().done < 300_000 {
            assert!(t.elapsed() < Duration::from_secs(20));
            std::thread::sleep(Duration::from_millis(10));
        }
        p.cancel("test");
        let s = wait(&p);
        assert_eq!(s.state, PackState::Available);
        let part = root.path().join("models/test/sub/a.onnx.part");
        let kept = fs::metadata(&part).unwrap().len();
        assert!(kept > 0 && kept < a.len() as u64, "kept {kept}");
        assert_eq!(s.done, kept);
        p.download("test", None).unwrap();
        let s = wait(&p);
        assert_eq!(s.state, PackState::Installed, "{:?}", s.error);
        assert_eq!(fs::read(root.path().join("models/test/sub/a.onnx")).unwrap(), a);
        assert_eq!(hits.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn outdated_when_the_catalog_changes() {
        let a = body(1000, 5);
        let (base, _) = serve(HashMap::from([("a.onnx".into(), a.clone())]), false);
        let root = tempfile::tempdir().unwrap();
        let p = Packs::new(root.path().into(), catalog(&base, &[("a.onnx", &a, true)]), Arc::new(|| {}));
        p.download("test", None).unwrap();
        assert_eq!(wait(&p).state, PackState::Installed);
        let newer = body(1000, 6);
        let p2 = Packs::new(root.path().into(), catalog(&base, &[("a.onnx", &newer, true)]), Arc::new(|| {}));
        assert_eq!(p2.status("test").unwrap().state, PackState::Outdated);
        assert!(p2.installed("test").is_none());
    }

    #[test]
    fn bundled_catalog_is_complete() {
        let c = Catalog::bundled();
        for id in ["places", "image-model", "text-reader"] {
            let p = c.get(id).unwrap_or_else(|| panic!("{id} missing"));
            assert!(p.size() > 0 && !p.license.is_empty() && !p.attribution.is_empty());
            for f in &p.files {
                assert!(f.url.starts_with("https://"), "{}", f.url);
                if let Some(s) = &f.sha256 {
                    assert!(s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()));
                }
            }
        }
        assert!(c.get("image-model").unwrap().files.iter().all(|f| f.sha256.is_some()));
        assert_eq!(human(1_536_574_638), "1.4 GB");
        assert_eq!(host("https://huggingface.co/x/y").as_deref(), Some("huggingface.co"));
    }
}
