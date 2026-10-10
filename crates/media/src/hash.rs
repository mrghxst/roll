//! File identity: a quick hash (size, first and last 64 KB) to notice moved
//! files cheaply, and a full BLAKE3 hash to confirm identity and find exact
//! duplicates.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const EDGE: u64 = 64 * 1024;

pub fn quick_hash(path: &Path) -> std::io::Result<String> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut h = blake3::Hasher::new();
    h.update(&len.to_le_bytes());
    let mut buf = vec![0u8; EDGE as usize];
    let n = read_full(&mut f, &mut buf)?;
    h.update(&buf[..n]);
    if len > EDGE * 2 {
        f.seek(SeekFrom::End(-(EDGE as i64)))?;
        let n = read_full(&mut f, &mut buf)?;
        h.update(&buf[..n]);
    } else if len > EDGE {
        let n = read_full(&mut f, &mut buf)?;
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex()[..32].to_string())
}

fn read_full(f: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

pub fn blake3_file(path: &Path) -> std::io::Result<String> {
    let mut h = blake3::Hasher::new();
    h.update_mmap_rayon(path)?;
    Ok(h.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_differ_by_content_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::write(&a, vec![1u8; 300_000]).unwrap();
        let mut other = vec![1u8; 300_000];
        other[299_999] = 2;
        std::fs::write(&b, &other).unwrap();
        assert_ne!(quick_hash(&a).unwrap(), quick_hash(&b).unwrap());
        assert_ne!(blake3_file(&a).unwrap(), blake3_file(&b).unwrap());
        std::fs::write(&b, vec![1u8; 300_000]).unwrap();
        assert_eq!(quick_hash(&a).unwrap(), quick_hash(&b).unwrap());
        assert_eq!(blake3_file(&a).unwrap(), blake3_file(&b).unwrap());
    }
}
