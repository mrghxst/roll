//! File operations that never overwrite. `std::fs::rename` replaces an
//! existing target on every platform, so moves use the platform's
//! no-replace rename. Moves across volumes copy, verify and then remove the
//! source.

use std::io;
use std::path::{Path, PathBuf};

/// Suffix of a copy in progress. Leftovers are removed on recovery.
pub const PARTIAL: &str = ".roll-partial";

#[cfg(windows)]
fn rename_noreplace(src: &Path, dst: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let wide = |p: &Path| -> Vec<u16> {
        // Verbatim paths lift the 260-character limit.
        let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
        let s = abs.as_os_str();
        let mut v: Vec<u16> = if s.to_string_lossy().starts_with(r"\\?\") {
            s.encode_wide().collect()
        } else {
            r"\\?\".encode_utf16().chain(s.encode_wide()).collect()
        };
        v.push(0);
        v
    };
    let (s, d) = (wide(src), wide(dst));
    // Flags 0: fail if the target exists, same volume only.
    if unsafe { MoveFileExW(s.as_ptr(), d.as_ptr(), 0) } != 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

#[cfg(target_os = "linux")]
fn rename_noreplace(src: &Path, dst: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let s = CString::new(src.as_os_str().as_bytes())?;
    let d = CString::new(dst.as_os_str().as_bytes())?;
    let r = unsafe { libc::renameat2(libc::AT_FDCWD, s.as_ptr(), libc::AT_FDCWD, d.as_ptr(), libc::RENAME_NOREPLACE) };
    if r == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

#[cfg(target_os = "macos")]
fn rename_noreplace(src: &Path, dst: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let s = CString::new(src.as_os_str().as_bytes())?;
    let d = CString::new(dst.as_os_str().as_bytes())?;
    let r = unsafe { libc::renamex_np(s.as_ptr(), d.as_ptr(), libc::RENAME_EXCL) };
    if r == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

fn same_file_ignoring_case(src: &Path, dst: &Path) -> bool {
    src.parent() == dst.parent()
        && src.file_name().map(|n| n.to_string_lossy().to_lowercase()) == dst.file_name().map(|n| n.to_string_lossy().to_lowercase())
}

fn cross_device(e: &io::Error) -> bool {
    // EXDEV on Unix, ERROR_NOT_SAME_DEVICE on Windows.
    matches!(e.raw_os_error(), Some(18) if cfg!(unix)) || matches!(e.raw_os_error(), Some(17) if cfg!(windows))
}

/// Move a file without ever replacing an existing one. Creates missing
/// parent folders. Falls back to copy, verify and delete across volumes.
pub fn move_file(src: &Path, dst: &Path) -> io::Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A case-only rename targets the same file on case-insensitive systems.
    if dst.exists() && !same_file_ignoring_case(src, dst) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} already exists", dst.display())));
    }
    match rename_noreplace(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if cross_device(&e) => {
            copy_verified(src, dst)?;
            std::fs::remove_file(src)
        }
        Err(e) => Err(e),
    }
}

/// Copy to a temporary name, compare BLAKE3 hashes, then rename into place.
pub fn copy_verified(src: &Path, dst: &Path) -> io::Result<String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if dst.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} already exists", dst.display())));
    }
    let tmp = partial_path(dst);
    std::fs::copy(src, &tmp)?;
    let (a, b) = (roll_media::hash::blake3_file(src)?, roll_media::hash::blake3_file(&tmp)?);
    if a != b {
        let _ = std::fs::remove_file(&tmp);
        return Err(io::Error::other("The copy does not match the original."));
    }
    if let Err(e) = rename_noreplace(&tmp, dst) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(a)
}

pub fn partial_path(dst: &Path) -> PathBuf {
    let mut s = dst.as_os_str().to_owned();
    s.push(PARTIAL);
    PathBuf::from(s)
}

/// Remove a folder if it is empty (and not a root).
pub fn remove_dir_if_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).map(|mut r| r.next().is_none()).unwrap_or(false) && std::fs::remove_dir(dir).is_ok()
}

/// Free bytes on the volume holding `path`.
#[cfg(windows)]
pub fn free_space(path: &Path) -> io::Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut w: Vec<u16> = path.as_os_str().encode_wide().collect();
    w.push(0);
    let mut free = 0u64;
    if unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) } != 0 {
        Ok(free)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
pub fn free_space(path: &Path) -> io::Result<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes())?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0 {
        #[allow(clippy::unnecessary_cast)]
        Ok(s.f_bavail as u64 * s.f_frsize as u64)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_overwrites() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a.jpg"), d.path().join("sub/b.jpg"));
        std::fs::write(&a, b"one").unwrap();
        move_file(&a, &b).unwrap();
        assert!(!a.exists() && b.exists());
        std::fs::write(&a, b"two").unwrap();
        let e = move_file(&a, &b).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&b).unwrap(), b"one");
        assert_eq!(std::fs::read(&a).unwrap(), b"two");
    }

    #[test]
    fn case_only_rename() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("img_1.jpg"), d.path().join("IMG_1.jpg"));
        std::fs::write(&a, b"x").unwrap();
        move_file(&a, &b).unwrap();
        let names: Vec<String> =
            std::fs::read_dir(d.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["IMG_1.jpg"]);
    }

    #[test]
    fn verified_copy() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a.bin"), d.path().join("x/b.bin"));
        std::fs::write(&a, vec![5u8; 100_000]).unwrap();
        let h = copy_verified(&a, &b).unwrap();
        assert_eq!(h, roll_media::hash::blake3_file(&b).unwrap());
        assert!(!partial_path(&b).exists());
        assert!(free_space(d.path()).unwrap() > 0);
    }

    #[test]
    fn long_paths() {
        let d = tempfile::tempdir().unwrap();
        let mut deep = d.path().to_path_buf();
        for _ in 0..4 {
            deep.push("a folder name long enough to push the path well past two hundred and sixty characters");
        }
        let a = d.path().join("a.jpg");
        std::fs::write(&a, b"x").unwrap();
        let b = deep.join("IMG_0001.JPG");
        assert!(b.as_os_str().len() > 300);
        move_file(&a, &b).unwrap();
        assert_eq!(std::fs::read(&b).unwrap(), b"x");
        move_file(&b, &a).unwrap();
        assert!(a.exists());
    }
}
