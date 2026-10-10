//! Watching roots for files added, changed or removed outside Roll. Events
//! are debounced per root; a quiet root then gets an incremental scan.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

const QUIET: Duration = Duration::from_secs(3);

pub struct RootWatcher {
    _watcher: RecommendedWatcher,
}

impl RootWatcher {
    pub fn start(roots: &[(i64, PathBuf)], on_change: impl Fn(i64) + Send + 'static) -> notify::Result<Self> {
        let (tx, rx) = mpsc::channel::<PathBuf>();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if matches!(ev.kind, notify::EventKind::Access(_)) {
                    return;
                }
                for p in ev.paths {
                    let _ = tx.send(p);
                }
            }
        })?;
        for (_, path) in roots {
            watcher.watch(path, RecursiveMode::Recursive)?;
        }
        let roots = roots.to_vec();
        std::thread::Builder::new()
            .name("roll-watch".into())
            .spawn(move || {
                let mut pending: HashMap<i64, Instant> = HashMap::new();
                loop {
                    match rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(p) => {
                            if let Some((id, _)) = roots.iter().filter(|(_, r)| p.starts_with(r)).max_by_key(|(_, r)| r.as_os_str().len()) {
                                pending.insert(*id, Instant::now());
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                    let ready: Vec<i64> = pending.iter().filter(|(_, t)| t.elapsed() >= QUIET).map(|(id, _)| *id).collect();
                    for id in ready {
                        pending.remove(&id);
                        on_change(id);
                    }
                }
            })
            .map_err(|e| notify::Error::generic(&e.to_string()))?;
        Ok(Self { _watcher: watcher })
    }
}
