//! Relative paths inside a root use `/` on every platform.

use std::path::{Component, Path, PathBuf};

pub fn rel_string(p: &Path) -> String {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub fn rel_dir(rel: &str) -> String {
    rel.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default()
}

pub fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

pub fn join_rel(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in rel.split('/').filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p
}

/// `%LOCALAPPDATA%\Roll` and the platform equivalents, or `ROLL_DATA_DIR`.
pub fn app_data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("ROLL_DATA_DIR") {
        return PathBuf::from(p);
    }
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Roll")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_paths() {
        let root = Path::new("lib");
        let p = join_rel(root, "2019/Photos/a.jpg");
        assert_eq!(rel_string(p.strip_prefix(root).unwrap()), "2019/Photos/a.jpg");
        assert_eq!(rel_dir("2019/Photos/a.jpg"), "2019/Photos");
        assert_eq!(rel_dir("a.jpg"), "");
        assert_eq!(file_name("2019/a.jpg"), "a.jpg");
    }
}
