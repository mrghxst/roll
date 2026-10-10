//! RAW previews. Nearly every RAW format embeds one or more full JPEG
//! previews; the largest complete one is used.

/// Byte range of the largest embedded JPEG (SOI to EOI) that has a frame.
pub fn largest_embedded_jpeg(data: &[u8]) -> Option<&[u8]> {
    let mut best: Option<(usize, usize)> = None;
    let mut i = 0;
    while i + 4 < data.len() {
        if data[i] == 0xFF
            && data[i + 1] == 0xD8
            && data[i + 2] == 0xFF
            && let Some(end) = jpeg_end(data, i)
        {
            if best.is_none_or(|(s, e)| end - i > e - s) {
                best = Some((i, end));
            }
            i = end;
            continue;
        }
        i += 1;
    }
    best.filter(|(s, e)| e - s > 4096).map(|(s, e)| &data[s..e])
}

/// Walk the markers from SOI; after SOS, scan entropy data for EOI.
fn jpeg_end(d: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 2;
    let mut has_frame = false;
    loop {
        if i + 4 > d.len() || d[i] != 0xFF {
            return None;
        }
        let m = d[i + 1];
        if m == 0xD9 {
            return has_frame.then_some(i + 2);
        }
        let len = u16::from_be_bytes([d[i + 2], d[i + 3]]) as usize;
        if matches!(m, 0xC0..=0xC3) {
            has_frame = true;
        }
        if m == 0xDA {
            let mut j = i + 2 + len;
            while j + 1 < d.len() {
                if d[j] == 0xFF && d[j + 1] != 0 && !(0xD0..=0xD7).contains(&d[j + 1]) {
                    if d[j + 1] == 0xD9 {
                        return has_frame.then_some(j + 2);
                    }
                    // Another marker after scan data (progressive): continue walking.
                    i = j;
                    break;
                }
                j += 1;
            }
            if j + 1 >= d.len() {
                return None;
            }
            continue;
        }
        i += 2 + len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_largest_preview() {
        let small = crate::testutil::jpeg_bytes(64, 48);
        let big = crate::testutil::jpeg_bytes(320, 240);
        let mut raw = b"II*\0junkjunk".to_vec();
        raw.extend_from_slice(&small);
        raw.extend_from_slice(&[0u8; 100]);
        raw.extend_from_slice(&big);
        raw.extend_from_slice(b"trailer");
        let found = largest_embedded_jpeg(&raw).unwrap();
        assert_eq!(found, &big[..]);
    }
}
