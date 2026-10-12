//! Notices camera cards and phones that mount as drives: a volume that
//! appears with a `DCIM` folder is announced to the UI, which offers Add
//! photos. Volumes present when Roll starts are not announced.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

pub fn watch(on_card: impl Fn(PathBuf) + Send + 'static) {
    std::thread::spawn(move || {
        let mut known: HashSet<PathBuf> = volumes().into_iter().collect();
        loop {
            std::thread::sleep(Duration::from_secs(3));
            let now: HashSet<PathBuf> = volumes().into_iter().collect();
            for v in now.difference(&known) {
                if v.join("DCIM").is_dir() {
                    on_card(v.clone());
                }
            }
            known = now;
        }
    });
}

#[cfg(windows)]
fn volumes() -> Vec<PathBuf> {
    // SAFETY: GetLogicalDrives takes no arguments and only returns a bit mask.
    let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
    (0..26u8).filter(|i| mask & (1 << i) != 0).map(|i| PathBuf::from(format!("{}:\\", (b'A' + i) as char))).collect()
}

#[cfg(not(windows))]
fn volumes() -> Vec<PathBuf> {
    let user = std::env::var("USER").unwrap_or_default();
    let parents = if cfg!(target_os = "macos") {
        vec![PathBuf::from("/Volumes")]
    } else {
        vec![PathBuf::from("/media").join(&user), PathBuf::from("/run/media").join(&user)]
    };
    parents.iter().filter_map(|p| std::fs::read_dir(p).ok()).flatten().flatten().map(|e| e.path()).collect()
}
