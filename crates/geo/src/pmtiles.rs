//! Reading PMTiles v3 archives (the format of the Protomaps builds): the
//! header, directories and tile ids, enough to copy an area out of a
//! remote archive with range requests. Spec:
//! <https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md>

use std::io::Read;

pub const HEADER_LEN: usize = 127;

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub root_offset: u64,
    pub root_length: u64,
    pub leaf_offset: u64,
    pub leaf_length: u64,
    pub data_offset: u64,
    pub data_length: u64,
    /// 1 none, 2 gzip, 3 brotli, 4 zstd.
    pub internal_compression: u8,
    pub tile_compression: u8,
    /// 1 vector (MVT).
    pub tile_type: u8,
    pub min_zoom: u8,
    pub max_zoom: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry {
    pub tile_id: u64,
    pub offset: u64,
    pub length: u32,
    /// 0 for a leaf directory; otherwise how many tile ids share the data.
    pub run_length: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a PMTiles v3 archive")]
    NotPmtiles,
    #[error("the archive uses a compression Roll cannot read ({0})")]
    Compression(u8),
    #[error("a directory is damaged")]
    Damaged,
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

pub fn parse_header(b: &[u8]) -> Result<Header, Error> {
    if b.len() < HEADER_LEN || &b[0..7] != b"PMTiles" || b[7] != 3 {
        return Err(Error::NotPmtiles);
    }
    Ok(Header {
        root_offset: u64_at(b, 8),
        root_length: u64_at(b, 16),
        leaf_offset: u64_at(b, 40),
        leaf_length: u64_at(b, 48),
        data_offset: u64_at(b, 56),
        data_length: u64_at(b, 64),
        internal_compression: b[97],
        tile_compression: b[98],
        tile_type: b[99],
        min_zoom: b[100],
        max_zoom: b[101],
    })
}

/// Undo the archive's compression (none or gzip).
pub fn decompress(b: &[u8], compression: u8) -> Result<Vec<u8>, Error> {
    match compression {
        0 | 1 => Ok(b.to_vec()),
        2 => {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(b).read_to_end(&mut out).map_err(|_| Error::Damaged)?;
            Ok(out)
        }
        other => Err(Error::Compression(other)),
    }
}

fn varint(b: &[u8], pos: &mut usize) -> Result<u64, Error> {
    let mut v = 0u64;
    let mut shift = 0;
    loop {
        let byte = *b.get(*pos).ok_or(Error::Damaged)?;
        *pos += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(v);
        }
        shift += 7;
        if shift > 63 {
            return Err(Error::Damaged);
        }
    }
}

/// A directory's entries, from its (decompressed) bytes.
pub fn parse_directory(b: &[u8]) -> Result<Vec<Entry>, Error> {
    let mut pos = 0;
    let n = varint(b, &mut pos)? as usize;
    if n > b.len() {
        return Err(Error::Damaged);
    }
    let mut entries = vec![Entry { tile_id: 0, offset: 0, length: 0, run_length: 0 }; n];
    let mut last = 0u64;
    for e in entries.iter_mut() {
        last += varint(b, &mut pos)?;
        e.tile_id = last;
    }
    for e in entries.iter_mut() {
        e.run_length = varint(b, &mut pos)? as u32;
    }
    for e in entries.iter_mut() {
        e.length = varint(b, &mut pos)? as u32;
    }
    for i in 0..n {
        let v = varint(b, &mut pos)?;
        entries[i].offset = if v == 0 && i > 0 { entries[i - 1].offset + u64::from(entries[i - 1].length) } else { v.saturating_sub(1) };
    }
    Ok(entries)
}

/// The entry that holds `tile_id`: the tile's data, or the leaf directory
/// to look in next.
pub fn find(entries: &[Entry], tile_id: u64) -> Option<Entry> {
    let i = entries.partition_point(|e| e.tile_id <= tile_id);
    let e = *entries.get(i.checked_sub(1)?)?;
    if e.run_length == 0 || tile_id < e.tile_id + u64::from(e.run_length) { Some(e) } else { None }
}

/// The Hilbert-curve tile id of z/x/y.
pub fn tile_id(z: u8, x: u32, y: u32) -> u64 {
    let mut acc: u64 = 0;
    for t in 0..z {
        acc += 1u64 << (2 * t);
    }
    let n: u64 = 1 << z;
    let (mut x, mut y) = (u64::from(x), u64::from(y));
    let mut d = 0u64;
    let mut s = n / 2;
    while s > 0 {
        let rx = u64::from(x & s > 0);
        let ry = u64::from(y & s > 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                x = n - 1 - x;
                y = n - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        s /= 2;
    }
    acc + d
}

/// The tiles covering [west, south, east, north] at zoom `z`, as x and y
/// ranges (inclusive).
pub fn tile_range(bbox: [f64; 4], z: u8) -> ((u32, u32), (u32, u32)) {
    let n = f64::from(1u32 << z);
    let max = (1u32 << z) - 1;
    let x = |lon: f64| (((lon + 180.0) / 360.0 * n).floor().max(0.0) as u32).min(max);
    let y = |lat: f64| {
        let r = lat.clamp(-85.0511, 85.0511).to_radians();
        (((1.0 - (r.tan() + 1.0 / r.cos()).ln() / std::f64::consts::PI) / 2.0 * n).floor().max(0.0) as u32).min(max)
    };
    ((x(bbox[0]), x(bbox[2])), (y(bbox[3]), y(bbox[1])))
}

/// How many tiles an area needs from zoom 0 to `max_zoom`.
pub fn tile_count(bbox: [f64; 4], max_zoom: u8) -> u64 {
    (0..=max_zoom)
        .map(|z| {
            let ((x0, x1), (y0, y1)) = tile_range(bbox, z);
            u64::from(x1 - x0 + 1) * u64::from(y1 - y0 + 1)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_ids_follow_the_spec() {
        assert_eq!(tile_id(0, 0, 0), 0);
        assert_eq!(tile_id(1, 0, 0), 1);
        assert_eq!(tile_id(1, 0, 1), 2);
        assert_eq!(tile_id(1, 1, 1), 3);
        assert_eq!(tile_id(1, 1, 0), 4);
        assert_eq!(tile_id(2, 0, 0), 5);
        // The last tile of zoom 12 is just before the first of zoom 13.
        assert_eq!(tile_id(12, 4095, 0) + 1, tile_id(13, 0, 0));
    }

    #[test]
    fn directories_round_trip() {
        // Three entries: two consecutive tiles (offset 0 = right after the
        // previous one) and a leaf directory.
        let mut b = vec![3u8];
        b.extend([5, 1, 10]); // tile id deltas: 5, 6, 16
        b.extend([1, 1, 0]); // run lengths
        b.extend([100, 50, 0x80, 0x01]); // lengths 100, 50, 128
        b.extend([1, 0, 201, 1]); // offsets: 0, after, 200
        let e = parse_directory(&b).unwrap();
        assert_eq!(e[0], Entry { tile_id: 5, offset: 0, length: 100, run_length: 1 });
        assert_eq!(e[1], Entry { tile_id: 6, offset: 100, length: 50, run_length: 1 });
        assert_eq!(e[2], Entry { tile_id: 16, offset: 200, length: 128, run_length: 0 });
        assert_eq!(find(&e, 6).unwrap().offset, 100);
        assert!(find(&e, 7).is_none(), "between tiles");
        assert_eq!(find(&e, 40).unwrap().run_length, 0, "a leaf covers the rest");
        assert!(find(&e, 2).is_none());
    }

    #[test]
    fn tiles_of_an_area() {
        // Switzerland, roughly.
        let ch = [5.9, 45.8, 10.5, 47.8];
        assert_eq!(tile_range(ch, 0), ((0, 0), (0, 0)));
        let ((x0, x1), (y0, y1)) = tile_range(ch, 8);
        assert!(x0 < x1 && y0 < y1 && x1 - x0 < 5, "{x0}..{x1} {y0}..{y1}");
        let n = tile_count(ch, 13);
        assert!((8_000..12_000).contains(&n), "{n}");
    }
}
