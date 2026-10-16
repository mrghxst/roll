//! Enlarging a vector tile (Mapbox Vector Tile 2.1) to a deeper zoom: the
//! part of the tile a child covers, with its coordinates scaled to fill
//! the child. Offline map areas that stop at zoom 13 answer zoom 14 and
//! 15 this way, next to areas that have those zooms, so one map source
//! serves both. Features outside the child are left out; the rest keep
//! their coordinates past the child's edges, which the map clips.

/// A tile `dz` levels deeper than `tile`, at `(dx, dy)` among the
/// `2^dz × 2^dz` children (0-based, y down). `tile` is uncompressed.
pub fn overzoom(tile: &[u8], dz: u8, dx: u32, dy: u32) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(dz <= 8 && dx < 1 << dz && dy < 1 << dz, "Not a child of this tile");
    let mut out = Vec::with_capacity(tile.len() / 2);
    for f in fields(tile) {
        let f = f?;
        match (f.number, f.value) {
            (3, Value::Bytes(layer)) => {
                let l = zoom_layer(layer, dz, dx, dy)?;
                if !l.is_empty() {
                    put_bytes(&mut out, 3, &l);
                }
            }
            _ => out.extend_from_slice(f.raw),
        }
    }
    Ok(out)
}

/// Each layer's name and number of features.
pub fn layers(tile: &[u8]) -> anyhow::Result<Vec<(String, usize)>> {
    let mut out = Vec::new();
    for f in fields(tile) {
        let Field { number: 3, value: Value::Bytes(layer), .. } = f? else { continue };
        let (mut name, mut n) = (String::new(), 0);
        for g in fields(layer) {
            match g? {
                Field { number: 1, value: Value::Bytes(b), .. } => name = String::from_utf8_lossy(b).into_owned(),
                Field { number: 2, .. } => n += 1,
                _ => {}
            }
        }
        out.push((name, n));
    }
    Ok(out)
}

fn zoom_layer(layer: &[u8], dz: u8, dx: u32, dy: u32) -> anyhow::Result<Vec<u8>> {
    let mut extent = 4096i64;
    for f in fields(layer) {
        let f = f?;
        if let (5, Value::Int(e)) = (f.number, f.value) {
            extent = e as i64;
        }
    }
    let scale = 1i64 << dz;
    let (ox, oy) = (dx as i64 * extent, dy as i64 * extent);
    // A little of the neighbours stays, so lines and labels meet at edges.
    let buffer = extent / 16;
    let mut out = Vec::with_capacity(layer.len() / 2);
    let mut features = 0;
    for f in fields(layer) {
        let f = f?;
        match (f.number, f.value) {
            (2, Value::Bytes(feature)) => {
                if let Some(feat) = zoom_feature(feature, scale, ox, oy, extent, buffer)? {
                    put_bytes(&mut out, 2, &feat);
                    features += 1;
                }
            }
            _ => out.extend_from_slice(f.raw),
        }
    }
    Ok(if features == 0 { Vec::new() } else { out })
}

fn zoom_feature(feature: &[u8], scale: i64, ox: i64, oy: i64, extent: i64, buffer: i64) -> anyhow::Result<Option<Vec<u8>>> {
    let mut out = Vec::with_capacity(feature.len());
    let mut geometry = None;
    for f in fields(feature) {
        let f = f?;
        match (f.number, f.value) {
            (4, Value::Bytes(g)) => geometry = Some(g),
            _ => out.extend_from_slice(f.raw),
        }
    }
    let Some(g) = geometry else { return Ok(Some(out)) };
    let cmds = packed(g)?;
    // Decode, scale and shift, and find the bounds.
    let (mut x, mut y) = (0i64, 0i64);
    let (mut nx, mut ny) = (0i64, 0i64);
    let (mut min, mut max) = ((i64::MAX, i64::MAX), (i64::MIN, i64::MIN));
    let mut enc = Vec::with_capacity(cmds.len());
    let mut i = 0;
    while i < cmds.len() {
        let (id, count) = (cmds[i] & 7, (cmds[i] >> 3) as usize);
        enc.push(cmds[i]);
        i += 1;
        if id == 7 {
            continue;
        }
        anyhow::ensure!((id == 1 || id == 2) && i + 2 * count <= cmds.len(), "Broken tile geometry");
        for _ in 0..count {
            x += unzig(cmds[i]);
            y += unzig(cmds[i + 1]);
            i += 2;
            let (sx, sy) = (x * scale - ox, y * scale - oy);
            min = (min.0.min(sx), min.1.min(sy));
            max = (max.0.max(sx), max.1.max(sy));
            enc.push(zig(sx - nx)?);
            enc.push(zig(sy - ny)?);
            (nx, ny) = (sx, sy);
        }
    }
    if max.0 < -buffer || max.1 < -buffer || min.0 > extent + buffer || min.1 > extent + buffer {
        return Ok(None);
    }
    let mut packed_geom = Vec::with_capacity(enc.len() * 2);
    for v in enc {
        put_varint(&mut packed_geom, u64::from(v));
    }
    put_bytes(&mut out, 4, &packed_geom);
    Ok(Some(out))
}

enum Value<'a> {
    Int(u64),
    Bytes(&'a [u8]),
    Other,
}

struct Field<'a> {
    number: u64,
    value: Value<'a>,
    /// The field as it was, key included, to copy unchanged.
    raw: &'a [u8],
}

fn fields(b: &[u8]) -> impl Iterator<Item = anyhow::Result<Field<'_>>> {
    let mut i = 0;
    std::iter::from_fn(move || {
        if i >= b.len() {
            return None;
        }
        let start = i;
        let r = (|| {
            let key = varint(b, &mut i)?;
            let value = match key & 7 {
                0 => Value::Int(varint(b, &mut i)?),
                2 => {
                    let n = varint(b, &mut i)? as usize;
                    anyhow::ensure!(i + n <= b.len(), "Tile cut short");
                    i += n;
                    Value::Bytes(&b[i - n..i])
                }
                5 => {
                    i += 4;
                    Value::Other
                }
                1 => {
                    i += 8;
                    Value::Other
                }
                t => anyhow::bail!("Unknown wire type {t} in tile"),
            };
            anyhow::ensure!(i <= b.len(), "Tile cut short");
            Ok(Field { number: key >> 3, value, raw: &b[start..i] })
        })();
        if r.is_err() {
            i = b.len();
        }
        Some(r)
    })
}

fn varint(b: &[u8], i: &mut usize) -> anyhow::Result<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let c = *b.get(*i).ok_or_else(|| anyhow::anyhow!("Tile cut short"))?;
        *i += 1;
        v |= u64::from(c & 0x7f) << shift;
        if c < 0x80 {
            return Ok(v);
        }
    }
    anyhow::bail!("Varint too long in tile")
}

fn packed(b: &[u8]) -> anyhow::Result<Vec<u32>> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        out.push(varint(b, &mut i)? as u32);
    }
    Ok(out)
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn put_bytes(out: &mut Vec<u8>, number: u64, b: &[u8]) {
    put_varint(out, (number << 3) | 2);
    put_varint(out, b.len() as u64);
    out.extend_from_slice(b);
}

fn unzig(v: u32) -> i64 {
    i64::from((v >> 1) as i32 ^ -((v & 1) as i32))
}

fn zig(v: i64) -> anyhow::Result<u32> {
    let v = i32::try_from(v).map_err(|_| anyhow::anyhow!("Tile coordinates too large"))?;
    Ok(((v << 1) ^ (v >> 31)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tile with one layer ("roads", extent 4096) holding one line per
    /// entry of `lines` (absolute points), each with an id.
    fn tile(lines: &[&[(i64, i64)]]) -> Vec<u8> {
        let mut layer = Vec::new();
        put_varint(&mut layer, 15 << 3);
        put_varint(&mut layer, 2);
        put_bytes(&mut layer, 1, b"roads");
        for (n, pts) in lines.iter().enumerate() {
            let mut g = Vec::new();
            let (mut x, mut y) = (0, 0);
            for (k, &(px, py)) in pts.iter().enumerate() {
                if k == 0 {
                    put_varint(&mut g, (1 << 3) | 1);
                } else if k == 1 {
                    put_varint(&mut g, (((pts.len() - 1) as u64) << 3) | 2);
                }
                put_varint(&mut g, u64::from(zig(px - x).unwrap()));
                put_varint(&mut g, u64::from(zig(py - y).unwrap()));
                (x, y) = (px, py);
            }
            let mut f = Vec::new();
            put_varint(&mut f, 1 << 3);
            put_varint(&mut f, n as u64 + 1);
            put_varint(&mut f, 3 << 3);
            put_varint(&mut f, 2);
            put_bytes(&mut f, 4, &g);
            put_bytes(&mut layer, 2, &f);
        }
        put_varint(&mut layer, 5 << 3);
        put_varint(&mut layer, 4096);
        let mut t = Vec::new();
        put_bytes(&mut t, 3, &layer);
        t
    }

    /// Each feature's id and points.
    fn read(t: &[u8]) -> Vec<(u64, Vec<(i64, i64)>)> {
        let mut out = Vec::new();
        for l in fields(t) {
            let Value::Bytes(layer) = l.unwrap().value else { continue };
            for f in fields(layer) {
                let f = f.unwrap();
                let (2, Value::Bytes(feat)) = (f.number, f.value) else { continue };
                let (mut id, mut pts) = (0, Vec::new());
                for g in fields(feat) {
                    let g = g.unwrap();
                    match (g.number, g.value) {
                        (1, Value::Int(v)) => id = v,
                        (4, Value::Bytes(b)) => {
                            let c = packed(b).unwrap();
                            let (mut x, mut y, mut i) = (0, 0, 0);
                            while i < c.len() {
                                let n = (c[i] >> 3) as usize;
                                i += 1;
                                for _ in 0..n {
                                    x += unzig(c[i]);
                                    y += unzig(c[i + 1]);
                                    i += 2;
                                    pts.push((x, y));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                out.push((id, pts));
            }
        }
        out
    }

    #[test]
    fn zigzag_round_trips() {
        for v in [0i64, 1, -1, 4095, -4096, 1 << 20, -(1 << 20)] {
            assert_eq!(unzig(zig(v).unwrap()), v);
        }
    }

    #[test]
    fn a_child_is_the_enlarged_quarter() {
        // One road in the top-left quarter, one in the bottom-right.
        let t = tile(&[&[(100, 200), (1000, 1500)], &[(3000, 3000), (4000, 3500)]]);
        assert_eq!(read(&overzoom(&t, 0, 0, 0).unwrap()), read(&t));
        assert_eq!(layers(&t).unwrap(), vec![("roads".to_string(), 2)]);
        // Top left, one level down: twice the size, the far road gone.
        assert_eq!(read(&overzoom(&t, 1, 0, 0).unwrap()), vec![(1, vec![(200, 400), (2000, 3000)])]);
        // Bottom right: the near road is gone, the far one moved up.
        assert_eq!(read(&overzoom(&t, 1, 1, 1).unwrap()), vec![(2, vec![(1904, 1904), (3904, 2904)])]);
        // Two levels down, a corner with nothing in it: no layer at all.
        assert!(read(&overzoom(&t, 2, 3, 0).unwrap()).is_empty());
        assert!(overzoom(&t, 1, 2, 0).is_err());
    }

    #[test]
    fn a_line_across_the_edge_is_kept_whole() {
        let t = tile(&[&[(1000, 1000), (3000, 1000)]]);
        assert_eq!(read(&overzoom(&t, 1, 1, 0).unwrap()), vec![(1, vec![(-2096, 2000), (1904, 2000)])]);
    }
}
