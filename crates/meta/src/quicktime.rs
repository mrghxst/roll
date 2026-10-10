//! Minimal ISO-BMFF / QuickTime reader. Reads only the `moov` box, so large
//! videos cost a few seeks, wherever `moov` sits in the file.

use std::io::{Read, Seek, SeekFrom};

use chrono::{DateTime, Utc};

use crate::model::{parse_datetime, DateSource, Gps, Taken};

const MAX_MOOV: u64 = 64 * 1024 * 1024;
/// Seconds between 1904-01-01 and 1970-01-01.
const MAC_EPOCH_OFFSET: i64 = 2_082_844_800;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QtInfo {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub fps: Option<f32>,
    pub codec: Option<String>,
    pub rotation: Option<i32>,
    pub has_audio: bool,
    /// `mvhd` creation time. UTC by spec; some cameras write local time.
    pub create_utc: Option<DateTime<Utc>>,
    /// `com.apple.quicktime.creationdate`: local time with offset.
    pub creation_date: Option<String>,
    pub content_id: Option<String>,
    pub iso6709: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub software: Option<String>,
}

impl QtInfo {
    /// The capture time. The Apple key carries the zone; otherwise the
    /// `mvhd` instant is reported with no offset (its `local` holds UTC).
    pub fn taken(&self) -> Option<Taken> {
        if let Some((local, off)) = self.creation_date.as_deref().and_then(parse_qt_date) {
            return Some(Taken::new(local, off, DateSource::QuickTime));
        }
        self.create_utc.map(|u| Taken::from_utc(u, None, DateSource::QuickTime))
    }

    pub fn gps(&self) -> Option<Gps> {
        parse_iso6709(self.iso6709.as_deref()?)
    }
}

/// Apple writes `2019-08-12T14:03:22+0200`, without the colon in the offset.
fn parse_qt_date(s: &str) -> Option<(chrono::NaiveDateTime, Option<i32>)> {
    let s = s.trim();
    if s.len() >= 24 {
        let (head, tail) = s.split_at(s.len() - 5);
        if (tail.starts_with('+') || tail.starts_with('-')) && !tail.contains(':') {
            let fixed = format!("{head}{}:{}", &tail[..3], &tail[3..]);
            return parse_datetime(&fixed);
        }
    }
    parse_datetime(s)
}

pub fn parse_iso6709(s: &str) -> Option<Gps> {
    // e.g. "+45.6416+010.6875+123.000/" or "+45.6416+010.6875/"
    let s = s.trim().trim_end_matches('/');
    let mut nums = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if (c == '+' || c == '-') && !cur.is_empty() {
            nums.push(cur.clone());
            cur.clear();
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        nums.push(cur);
    }
    let lat: f64 = nums.first()?.parse().ok()?;
    let lon: f64 = nums.get(1)?.parse().ok()?;
    let alt = nums.get(2).and_then(|a| a.trim_end_matches("CRSWGS_84").parse().ok());
    let g = Gps { lat, lon, alt, accuracy_m: None };
    g.is_valid().then_some(g)
}

pub fn read<R: Read + Seek>(r: &mut R) -> std::io::Result<Option<QtInfo>> {
    let file_len = r.seek(SeekFrom::End(0))?;
    let mut pos = 0u64;
    while pos + 8 <= file_len {
        r.seek(SeekFrom::Start(pos))?;
        let mut hdr = [0u8; 16];
        r.read_exact(&mut hdr[..8])?;
        let mut size = u32::from_be_bytes(hdr[0..4].try_into().unwrap()) as u64;
        let kind = [hdr[4], hdr[5], hdr[6], hdr[7]];
        let mut header = 8u64;
        if size == 1 {
            r.read_exact(&mut hdr[8..16])?;
            size = u64::from_be_bytes(hdr[8..16].try_into().unwrap());
            header = 16;
        } else if size == 0 {
            size = file_len - pos;
        }
        if size < header {
            return Ok(None);
        }
        if &kind == b"moov" {
            let body = size - header;
            if body > MAX_MOOV {
                return Ok(None);
            }
            let mut buf = vec![0u8; body as usize];
            r.read_exact(&mut buf)?;
            return Ok(Some(parse_moov(&buf)));
        }
        pos += size;
    }
    Ok(None)
}

struct Boxes<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Iterator for Boxes<'a> {
    type Item = (&'a [u8; 4], &'a [u8]);
    fn next(&mut self) -> Option<Self::Item> {
        let d = self.data;
        if self.pos + 8 > d.len() {
            return None;
        }
        let mut size = u32::from_be_bytes(d[self.pos..self.pos + 4].try_into().ok()?) as usize;
        let kind: &[u8; 4] = d[self.pos + 4..self.pos + 8].try_into().ok()?;
        let mut header = 8;
        if size == 1 {
            size = u64::from_be_bytes(d.get(self.pos + 8..self.pos + 16)?.try_into().ok()?) as usize;
            header = 16;
        } else if size == 0 {
            size = d.len() - self.pos;
        }
        if size < header || self.pos + size > d.len() {
            return None;
        }
        let body = &d[self.pos + header..self.pos + size];
        self.pos += size;
        Some((kind, body))
    }
}

fn boxes(data: &[u8]) -> Boxes<'_> {
    Boxes { data, pos: 0 }
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data).find(|(k, _)| *k == kind).map(|(_, b)| b)
}

fn be32(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(o..o + 4)?.try_into().ok()?))
}
fn be64(d: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_be_bytes(d.get(o..o + 8)?.try_into().ok()?))
}

/// (creation seconds since 1904, timescale, duration) of `mvhd`/`mdhd`.
fn header_times(b: &[u8]) -> Option<(u64, u32, u64)> {
    if b.first()? == &1 {
        Some((be64(b, 4)?, be32(b, 20)?, be64(b, 24)?))
    } else {
        Some((be32(b, 4)? as u64, be32(b, 12)?, be32(b, 16)? as u64))
    }
}

fn parse_moov(moov: &[u8]) -> QtInfo {
    let mut q = QtInfo::default();
    if let Some((ctime, ts, dur)) = child(moov, b"mvhd").and_then(header_times) {
        if ts > 0 {
            q.duration_ms = Some(dur * 1000 / ts as u64);
        }
        if ctime > 0 {
            q.create_utc = DateTime::from_timestamp(ctime as i64 - MAC_EPOCH_OFFSET, 0);
        }
    }
    for (kind, trak) in boxes(moov) {
        if kind != b"trak" {
            continue;
        }
        let Some(mdia) = child(trak, b"mdia") else { continue };
        let handler = child(mdia, b"hdlr").and_then(|h| h.get(8..12)).unwrap_or_default();
        match handler {
            b"soun" => q.has_audio = true,
            b"vide" if q.codec.is_none() => read_video_trak(trak, mdia, &mut q),
            _ => {}
        }
    }
    if let Some(meta) = child(moov, b"meta") {
        read_mdta(meta, &mut q);
    }
    if let Some(udta) = child(moov, b"udta") {
        read_udta(udta, &mut q);
    }
    q
}

fn read_video_trak(trak: &[u8], mdia: &[u8], q: &mut QtInfo) {
    if let Some(tkhd) = child(trak, b"tkhd") {
        let base = if tkhd.first() == Some(&1) { 4 + 8 + 8 + 4 + 4 + 8 } else { 4 + 4 + 4 + 4 + 4 + 4 };
        let m = base + 8 + 2 + 2 + 2 + 2;
        let fixed = |o: usize| be32(tkhd, o).map(|v| v as i32 as f64 / 65536.0);
        if let (Some(a), Some(b)) = (fixed(m), fixed(m + 4)) {
            let deg = b.atan2(a).to_degrees().round() as i32;
            q.rotation = Some(deg.rem_euclid(360));
        }
        if let (Some(w), Some(h)) = (be32(tkhd, m + 36), be32(tkhd, m + 40)) {
            let (w, h) = (w >> 16, h >> 16);
            if w > 0 && h > 0 {
                q.width = Some(w);
                q.height = Some(h);
            }
        }
    }
    let stbl = child(mdia, b"minf").and_then(|m| child(m, b"stbl"));
    if let Some(stsd) = stbl.and_then(|s| child(s, b"stsd")) {
        if let Some(fourcc) = stsd.get(12..16) {
            q.codec = Some(codec_name(fourcc));
        }
        // Visual sample entry: coded width and height.
        if q.width.is_none() {
            if let (Some(w), Some(h)) = (stsd.get(40..42), stsd.get(42..44)) {
                q.width = Some(u16::from_be_bytes([w[0], w[1]]) as u32);
                q.height = Some(u16::from_be_bytes([h[0], h[1]]) as u32);
            }
        }
    }
    let samples: Option<u64> = stbl.and_then(|s| child(s, b"stts")).and_then(|stts| {
        let n = be32(stts, 4)? as usize;
        (0..n).map(|i| be32(stts, 8 + i * 8).map(|c| c as u64)).sum()
    });
    if let (Some(samples), Some((_, ts, dur))) = (samples, child(mdia, b"mdhd").and_then(header_times)) {
        if ts > 0 && dur > 0 {
            q.fps = Some((samples as f64 * ts as f64 / dur as f64) as f32);
        }
    }
}

fn codec_name(f: &[u8]) -> String {
    match f {
        b"avc1" | b"avc3" => "h264".into(),
        b"hvc1" | b"hev1" => "hevc".into(),
        b"av01" => "av1".into(),
        b"vp09" => "vp9".into(),
        b"vp08" => "vp8".into(),
        b"mp4v" => "mpeg4".into(),
        b"jpeg" | b"mjpa" => "mjpeg".into(),
        b"apcn" | b"apch" | b"apcs" | b"apco" | b"ap4h" | b"ap4x" => "prores".into(),
        other => String::from_utf8_lossy(other).trim().to_string(),
    }
}

/// Apple `mdta` metadata: a `keys` list and an `ilst` indexed by key number.
fn read_mdta(meta: &[u8], q: &mut QtInfo) {
    // QuickTime `meta` has no version/flags; ISO `meta` does. Try both.
    let body = if child(meta, b"hdlr").is_some() { meta } else { meta.get(4..).unwrap_or_default() };
    let (Some(keys), Some(ilst)) = (child(body, b"keys"), child(body, b"ilst")) else { return };
    let mut names = Vec::new();
    let mut o = 8;
    let count = be32(keys, 4).unwrap_or(0);
    for _ in 0..count {
        let Some(size) = be32(keys, o) else { break };
        let size = size as usize;
        if size < 8 || o + size > keys.len() {
            break;
        }
        names.push(String::from_utf8_lossy(&keys[o + 8..o + size]).into_owned());
        o += size;
    }
    for (kind, item) in boxes(ilst) {
        let idx = u32::from_be_bytes(*kind) as usize;
        let Some(name) = idx.checked_sub(1).and_then(|i| names.get(i)) else { continue };
        let Some(data) = child(item, b"data") else { continue };
        let Some(value) = data.get(8..).map(|v| String::from_utf8_lossy(v).trim_end_matches('\0').to_string()) else {
            continue;
        };
        match name.as_str() {
            "com.apple.quicktime.content.identifier" => q.content_id = Some(value),
            "com.apple.quicktime.creationdate" => q.creation_date = Some(value),
            "com.apple.quicktime.location.ISO6709" => q.iso6709 = Some(value),
            "com.apple.quicktime.make" => q.make = Some(value),
            "com.apple.quicktime.model" => q.model = Some(value),
            "com.apple.quicktime.software" => q.software = Some(value),
            _ => {}
        }
    }
}

fn read_udta(udta: &[u8], q: &mut QtInfo) {
    for (kind, body) in boxes(udta) {
        // International text: u16 length, u16 language, then the string.
        let text = || {
            let n = u16::from_be_bytes([*body.first()?, *body.get(1)?]) as usize;
            body.get(4..4 + n).map(|s| String::from_utf8_lossy(s).trim_end_matches('\0').to_string())
        };
        match kind {
            b"\xa9xyz" if q.iso6709.is_none() => q.iso6709 = text(),
            b"\xa9mak" if q.make.is_none() => q.make = text(),
            b"\xa9mod" if q.model.is_none() => q.model = text(),
            b"\xa9swr" if q.software.is_none() => q.software = text(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(body);
        v
    }

    #[test]
    fn iso6709() {
        let g = parse_iso6709("+45.6416+010.6875+123.000/").unwrap();
        assert!((g.lat - 45.6416).abs() < 1e-9 && (g.lon - 10.6875).abs() < 1e-9);
        assert_eq!(g.alt, Some(123.0));
        let g = parse_iso6709("-33.8688+151.2093/").unwrap();
        assert!(g.lat < 0.0);
    }

    #[test]
    fn apple_date() {
        let (d, o) = parse_qt_date("2019-08-12T14:03:22+0200").unwrap();
        assert_eq!(d.to_string(), "2019-08-12 14:03:22");
        assert_eq!(o, Some(120));
    }

    #[test]
    fn reads_mvhd_keys_and_rotation() {
        let mut mvhd = vec![0u8; 100];
        mvhd[4..8].copy_from_slice(&(MAC_EPOCH_OFFSET as u32 + 1_565_611_402).to_be_bytes());
        mvhd[12..16].copy_from_slice(&600u32.to_be_bytes());
        mvhd[16..20].copy_from_slice(&3000u32.to_be_bytes());
        let mut tkhd = vec![0u8; 84];
        let m = 24 + 16;
        tkhd[m..m + 4].copy_from_slice(&0u32.to_be_bytes());
        tkhd[m + 4..m + 8].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        tkhd[m + 36..m + 40].copy_from_slice(&(1920u32 << 16).to_be_bytes());
        tkhd[m + 40..m + 44].copy_from_slice(&(1080u32 << 16).to_be_bytes());
        let mut hdlr = vec![0u8; 8];
        hdlr.extend_from_slice(b"vide");
        hdlr.extend_from_slice(&[0; 12]);
        let mut stsd = vec![0u8; 8];
        stsd.extend_from_slice(&[0, 0, 0, 86]);
        stsd.extend_from_slice(b"hvc1");
        stsd.extend_from_slice(&[0; 78]);
        let mut stts = vec![0u8; 4];
        stts.extend_from_slice(&1u32.to_be_bytes());
        stts.extend_from_slice(&150u32.to_be_bytes());
        stts.extend_from_slice(&20u32.to_be_bytes());
        let mut mdhd = vec![0u8; 24];
        mdhd[12..16].copy_from_slice(&600u32.to_be_bytes());
        mdhd[16..20].copy_from_slice(&3000u32.to_be_bytes());
        let stbl = bx(b"stbl", &[bx(b"stsd", &stsd), bx(b"stts", &stts)].concat());
        let mdia = bx(b"mdia", &[bx(b"mdhd", &mdhd), bx(b"hdlr", &hdlr), bx(b"minf", &stbl)].concat());
        let trak = bx(b"trak", &[bx(b"tkhd", &tkhd), mdia].concat());
        let key = |s: &str| [((s.len() + 8) as u32).to_be_bytes().to_vec(), b"mdta".to_vec(), s.as_bytes().to_vec()].concat();
        let mut keys = vec![0u8; 4];
        keys.extend_from_slice(&1u32.to_be_bytes());
        keys.extend(key("com.apple.quicktime.content.identifier"));
        let data = bx(b"data", &[vec![0, 0, 0, 1, 0, 0, 0, 0], b"ABC-123".to_vec()].concat());
        let ilst = bx(b"ilst", &bx(&1u32.to_be_bytes(), &data));
        let meta = bx(b"meta", &[bx(b"hdlr", &[0u8; 24]), bx(b"keys", &keys), ilst].concat());
        let moov = bx(b"moov", &[bx(b"mvhd", &mvhd), trak, meta].concat());
        let file = [bx(b"ftyp", b"qt  \0\0\0\0"), moov].concat();
        let q = read(&mut std::io::Cursor::new(file)).unwrap().unwrap();
        assert_eq!(q.duration_ms, Some(5000));
        assert_eq!(q.rotation, Some(90));
        assert_eq!(q.codec.as_deref(), Some("hevc"));
        assert_eq!((q.width, q.height), (Some(1920), Some(1080)));
        assert_eq!(q.fps, Some(30.0));
        assert_eq!(q.content_id.as_deref(), Some("ABC-123"));
        assert_eq!(q.create_utc.unwrap().timestamp(), 1_565_611_402);
        assert!(!q.has_audio);
    }
}
