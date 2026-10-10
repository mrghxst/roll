//! EXIF from images through nom-exif, plus the Apple maker note fields that
//! nom-exif leaves as raw bytes (Live Photo and burst identifiers).

use nom_exif::{EntryValue, Exif, ExifDateTime, ExifTag, IfdIndex, IfdKind};

use crate::model::{DateSource, FileMeta, Gps, Taken, parse_datetime, parse_offset};

const TAG_RATING: u16 = 0x4746;
const TAG_GPS_H_ERROR: u16 = 0x001f;
const TAG_BODY_SERIAL: u16 = 0xa431;

fn text(v: Option<&EntryValue>) -> Option<String> {
    let s = match v? {
        EntryValue::Text(s) => s.clone(),
        EntryValue::Undefined(b) => String::from_utf8_lossy(b).into_owned(),
        other => other.as_str()?.to_string(),
    };
    let s = s.trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string();
    (!s.is_empty()).then_some(s)
}

/// EXIF `UserComment` starts with an 8-byte character code.
fn user_comment(v: Option<&EntryValue>) -> Option<String> {
    let bytes: Vec<u8> = match v? {
        EntryValue::Undefined(b) => b.to_vec(),
        EntryValue::Text(s) => return Some(s.trim_matches('\0').trim().to_string()),
        _ => return None,
    };
    let body = if bytes.len() >= 8 && (bytes.starts_with(b"ASCII") || bytes.starts_with(b"UNICODE") || bytes[..8] == [0; 8]) {
        &bytes[8..]
    } else {
        &bytes[..]
    };
    if bytes.starts_with(b"UNICODE") {
        let u: Vec<u16> = body.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
        return Some(String::from_utf16_lossy(&u).trim_matches('\0').trim().to_string());
    }
    Some(String::from_utf8_lossy(body).trim_matches('\0').trim().to_string())
}

fn exif_taken(exif: &Exif, tag: ExifTag, offset_tag: ExifTag, source: DateSource) -> Option<Taken> {
    let v = exif.get(tag)?;
    let (local, mut offset) = match v.as_datetime() {
        Some(ExifDateTime::Aware(dt)) => (dt.naive_local(), Some(dt.offset().local_minus_utc() / 60)),
        Some(ExifDateTime::Naive(n)) => (n, None),
        None => parse_datetime(v.as_str()?)?,
    };
    if offset.is_none() {
        offset = text(exif.get(offset_tag)).as_deref().and_then(parse_offset);
    }
    (local.and_utc().timestamp() > 0).then(|| Taken::new(local, offset, source))
}

pub fn apply(exif: &Exif, m: &mut FileMeta) {
    m.make = text(exif.get(ExifTag::Make));
    m.model = text(exif.get(ExifTag::Model));
    m.software = text(exif.get(ExifTag::Software));
    m.lens = text(exif.get(ExifTag::LensModel));
    m.serial =
        text(exif.get_by_code_in(IfdIndex::MAIN, IfdKind::Exif, TAG_BODY_SERIAL)).or_else(|| text(exif.get(ExifTag::CameraSerialNumber)));
    m.description = text(exif.get(ExifTag::ImageDescription));
    m.orientation = exif.get(ExifTag::Orientation).and_then(|v| v.try_as_integer()).map(|v| v as u16);
    if m.width.is_none() {
        m.width = exif.get(ExifTag::ExifImageWidth).and_then(|v| v.try_as_integer()).map(|v| v as u32);
        m.height = exif.get(ExifTag::ExifImageHeight).and_then(|v| v.try_as_integer()).map(|v| v as u32);
    }
    m.rating = exif
        .get_by_code_in(IfdIndex::MAIN, IfdKind::Tiff, TAG_RATING)
        .and_then(|v| v.try_as_integer())
        .filter(|r| (0..=5).contains(r))
        .map(|r| r as u8);
    m.taken = exif_taken(exif, ExifTag::DateTimeOriginal, ExifTag::OffsetTimeOriginal, DateSource::ExifOriginal)
        .or_else(|| exif_taken(exif, ExifTag::CreateDate, ExifTag::OffsetTimeDigitized, DateSource::ExifCreate));
    if let Some(g) = exif.gps_info() {
        let accuracy_m =
            exif.get_by_code_in(IfdIndex::MAIN, IfdKind::Gps, TAG_GPS_H_ERROR).and_then(|v| v.as_urational()).and_then(|r| r.to_f64());
        if let (Some(lat), Some(lon)) = (g.latitude_decimal(), g.longitude_decimal()) {
            let gps = Gps { lat, lon, alt: g.altitude_meters(), accuracy_m };
            if gps.is_valid() {
                m.gps = Some(gps);
            }
        }
    }
    if user_comment(exif.get(ExifTag::UserComment)).is_some_and(|c| c.eq_ignore_ascii_case("screenshot")) {
        m.screenshot_marker = true;
    }
    if let Some(EntryValue::Undefined(note)) = exif.get(ExifTag::MakerNote) {
        m.has_maker_notes = true;
        if note.starts_with(b"Apple iOS") {
            let apple = apple_maker_note(note);
            m.content_id = apple.content_id;
            m.burst_id = apple.burst_id;
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct AppleNote {
    pub content_id: Option<String>,
    pub burst_id: Option<String>,
}

/// Apple maker note: `Apple iOS\0` + version + `MM`, then a big-endian IFD
/// whose offsets are relative to the start of the note.
pub fn apple_maker_note(note: &[u8]) -> AppleNote {
    let mut out = AppleNote::default();
    if note.len() < 16 || &note[12..14] != b"MM" {
        return out;
    }
    let be16 = |o: usize| note.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let be32 = |o: usize| note.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let Some(count) = be16(14) else { return out };
    for i in 0..count as usize {
        let e = 16 + i * 12;
        let (Some(tag), Some(typ), Some(n)) = (be16(e), be16(e + 2), be32(e + 4)) else { break };
        if typ != 2 {
            continue;
        }
        let n = n as usize;
        let raw = if n <= 4 { note.get(e + 8..e + 8 + n) } else { be32(e + 8).and_then(|o| note.get(o as usize..o as usize + n)) };
        let Some(raw) = raw else { continue };
        let s = String::from_utf8_lossy(raw).trim_end_matches('\0').trim().to_string();
        if s.is_empty() {
            continue;
        }
        match tag {
            0x0011 => out.content_id = Some(s),
            0x000b => out.burst_id = Some(s),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apple_note() {
        let id = b"7E1B7A2C-0D4B-4F62-9E26-A1C3B5D7E9F0\0";
        let mut note = b"Apple iOS\0\0\x01MM".to_vec();
        note.extend_from_slice(&1u16.to_be_bytes());
        note.extend_from_slice(&0x0011u16.to_be_bytes());
        note.extend_from_slice(&2u16.to_be_bytes());
        note.extend_from_slice(&(id.len() as u32).to_be_bytes());
        note.extend_from_slice(&32u32.to_be_bytes());
        note.extend_from_slice(&[0; 4]);
        note.extend_from_slice(id);
        let a = apple_maker_note(&note);
        assert_eq!(a.content_id.as_deref(), Some("7E1B7A2C-0D4B-4F62-9E26-A1C3B5D7E9F0"));
    }

    #[test]
    fn user_comment_prefix() {
        let mut b = b"ASCII\0\0\0".to_vec();
        b.extend_from_slice(b"Screenshot");
        assert_eq!(user_comment(Some(&EntryValue::Undefined(b))).as_deref(), Some("Screenshot"));
    }
}
