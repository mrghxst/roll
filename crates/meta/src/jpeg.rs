//! JPEG marker walk: frame size, ICC profile name and a fingerprint of the
//! quantization tables. The tables identify the encoder (camera firmware,
//! a phone's save path, an app's re-encode), so one decision about a group
//! with the same fingerprint and size can cover thousands of files.

#[derive(Debug, Clone, Default, PartialEq)]
pub struct JpegInfo {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub icc_profile: Option<String>,
    pub qtable: Option<String>,
}

pub fn scan(bytes: &[u8]) -> JpegInfo {
    let mut info = JpegInfo::default();
    let mut icc: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut dqt = Vec::new();
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        if marker == 0xFF || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 1;
            continue;
        }
        if marker == 0xD9 || marker == 0xDA {
            break;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let start = i + 4;
        let end = (i + 2 + len).min(bytes.len());
        if len < 2 || start > end {
            break;
        }
        let seg = &bytes[start..end];
        match marker {
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF if seg.len() >= 5 && info.width.is_none() => {
                info.height = Some(u16::from_be_bytes([seg[1], seg[2]]) as u32);
                info.width = Some(u16::from_be_bytes([seg[3], seg[4]]) as u32);
            }
            0xDB => dqt.extend_from_slice(seg),
            0xE2 if seg.starts_with(b"ICC_PROFILE\0") && seg.len() > 14 => {
                icc.push((seg[12], seg[14..].to_vec()));
            }
            _ => {}
        }
        i = end;
    }
    if !dqt.is_empty() {
        info.qtable = Some(format!("{:016x}", fnv1a(&dqt)));
    }
    if !icc.is_empty() {
        icc.sort_by_key(|(n, _)| *n);
        let profile: Vec<u8> = icc.into_iter().flat_map(|(_, d)| d).collect();
        info.icc_profile = icc_description(&profile);
    }
    info
}

pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Read the `desc` tag of an ICC profile (v2 `desc` or v4 `mluc`).
pub fn icc_description(p: &[u8]) -> Option<String> {
    let be32 = |o: usize| p.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let count = be32(128)?;
    for t in 0..count.min(64) {
        let base = 132 + t * 12;
        if p.get(base..base + 4)? != b"desc" {
            continue;
        }
        let (off, size) = (be32(base + 4)?, be32(base + 8)?);
        let data = p.get(off..off + size)?;
        return match data.get(0..4)? {
            b"desc" => {
                let n = u32::from_be_bytes(data.get(8..12)?.try_into().ok()?) as usize;
                let s = data.get(12..12 + n)?;
                Some(String::from_utf8_lossy(s).trim_end_matches('\0').trim().to_string())
            }
            b"mluc" => {
                let rec_off = 16;
                let len = u32::from_be_bytes(data.get(rec_off + 4..rec_off + 8)?.try_into().ok()?) as usize;
                let soff = u32::from_be_bytes(data.get(rec_off + 8..rec_off + 12)?.try_into().ok()?) as usize;
                let s = data.get(soff..soff + len)?;
                let u: Vec<u16> = s.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
                Some(String::from_utf16_lossy(&u).trim_end_matches('\0').trim().to_string())
            }
            _ => None,
        }
        .filter(|s| !s.is_empty());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(marker: u8, data: &[u8]) -> Vec<u8> {
        let mut v = vec![0xFF, marker];
        v.extend_from_slice(&((data.len() + 2) as u16).to_be_bytes());
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn reads_frame_and_tables() {
        let mut b = vec![0xFF, 0xD8];
        b.extend(seg(0xDB, &[0; 65]));
        b.extend(seg(0xC0, &[8, 0x01, 0xE0, 0x02, 0x80, 3]));
        b.extend([0xFF, 0xDA]);
        let info = scan(&b);
        assert_eq!((info.width, info.height), (Some(640), Some(480)));
        assert!(info.qtable.is_some());
    }

    #[test]
    fn icc_v2_desc() {
        let mut p = vec![0u8; 128];
        p.extend_from_slice(&1u32.to_be_bytes());
        p.extend_from_slice(b"desc");
        p.extend_from_slice(&144u32.to_be_bytes());
        let mut tag = b"desc\0\0\0\0".to_vec();
        tag.extend_from_slice(&9u32.to_be_bytes());
        tag.extend_from_slice(b"sRGB IEC\0");
        p.extend_from_slice(&(tag.len() as u32).to_be_bytes());
        p.extend_from_slice(&tag);
        assert_eq!(icc_description(&p).as_deref(), Some("sRGB IEC"));
    }
}
