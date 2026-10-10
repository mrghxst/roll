//! A small big-endian TIFF/EXIF encoder: IFD0, the Exif sub-IFD and the GPS
//! sub-IFD with the tags the fixtures need. Used for HEIC Exif items and
//! for the stress library, where calling ExifTool per file is too slow.

#[derive(Debug, Clone, Default)]
pub struct ExifSpec {
    pub make: Option<String>,
    pub model: Option<String>,
    pub software: Option<String>,
    pub serial: Option<String>,
    pub lens: Option<String>,
    pub orientation: Option<u16>,
    /// `YYYY:MM:DD HH:MM:SS`
    pub date: Option<String>,
    /// `+02:00`
    pub offset: Option<String>,
    pub gps: Option<(f64, f64, f64)>,
    pub user_comment: Option<String>,
    pub maker_note: Option<Vec<u8>>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

enum Val {
    Ascii(String),
    Short(u16),
    Long(u32),
    Rational(Vec<(u32, u32)>),
    Byte(Vec<u8>),
    Undefined(Vec<u8>),
}

impl Val {
    fn kind(&self) -> u16 {
        match self {
            Val::Byte(_) => 1,
            Val::Ascii(_) => 2,
            Val::Short(_) => 3,
            Val::Long(_) => 4,
            Val::Rational(_) => 5,
            Val::Undefined(_) => 7,
        }
    }
    fn bytes(&self) -> (u32, Vec<u8>) {
        match self {
            Val::Ascii(s) => {
                let mut b = s.as_bytes().to_vec();
                b.push(0);
                (b.len() as u32, b)
            }
            Val::Short(v) => (1, v.to_be_bytes().to_vec()),
            Val::Long(v) => (1, v.to_be_bytes().to_vec()),
            Val::Rational(r) => (r.len() as u32, r.iter().flat_map(|(a, b)| [a.to_be_bytes(), b.to_be_bytes()].concat()).collect()),
            Val::Byte(b) | Val::Undefined(b) => (b.len() as u32, b.clone()),
        }
    }
}

/// Serialize IFDs. `subs` are (pointer tag, entries) placed after the main IFD.
fn write_ifd(out: &mut Vec<u8>, mut entries: Vec<(u16, Val)>, next_patch: &mut Vec<(usize, u16)>) {
    entries.sort_by_key(|(t, _)| *t);
    let start = out.len();
    let n = entries.len();
    out.extend_from_slice(&(n as u16).to_be_bytes());
    let data_start = start + 2 + n * 12 + 4;
    let mut data = Vec::new();
    for (tag, val) in &entries {
        let (count, bytes) = val.bytes();
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&val.kind().to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
        if *tag == 0x8769 || *tag == 0x8825 {
            next_patch.push((out.len(), *tag));
            out.extend_from_slice(&[0; 4]);
        } else if bytes.len() <= 4 {
            let mut b = bytes.clone();
            b.resize(4, 0);
            out.extend_from_slice(&b);
        } else {
            let off = (data_start + data.len()) as u32;
            out.extend_from_slice(&off.to_be_bytes());
            data.extend_from_slice(&bytes);
            if data.len() % 2 == 1 {
                data.push(0);
            }
        }
    }
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&data);
}

fn rational(v: f64, den: u32) -> (u32, u32) {
    ((v * den as f64).round() as u32, den)
}

fn dms(v: f64) -> Vec<(u32, u32)> {
    let v = v.abs();
    let d = v.floor();
    let m = ((v - d) * 60.0).floor();
    let s = (v - d - m / 60.0) * 3600.0;
    vec![(d as u32, 1), (m as u32, 1), rational(s, 10000)]
}

/// The TIFF block (starting with `MM\0*`), as stored after `Exif\0\0`.
pub fn tiff(spec: &ExifSpec) -> Vec<u8> {
    let mut ifd0: Vec<(u16, Val)> = Vec::new();
    let s = |v: &Option<String>| v.clone().map(Val::Ascii);
    if let Some(v) = s(&spec.make) {
        ifd0.push((0x010f, v));
    }
    if let Some(v) = s(&spec.model) {
        ifd0.push((0x0110, v));
    }
    if let Some(v) = s(&spec.software) {
        ifd0.push((0x0131, v));
    }
    if let Some(o) = spec.orientation {
        ifd0.push((0x0112, Val::Short(o)));
    }
    if let Some(v) = s(&spec.date) {
        ifd0.push((0x0132, v));
    }
    let mut exif: Vec<(u16, Val)> = vec![(0x9000, Val::Undefined(b"0232".to_vec()))];
    if let Some(d) = &spec.date {
        exif.push((0x9003, Val::Ascii(d.clone())));
        exif.push((0x9004, Val::Ascii(d.clone())));
    }
    if let Some(o) = &spec.offset {
        exif.push((0x9011, Val::Ascii(o.clone())));
        exif.push((0x9012, Val::Ascii(o.clone())));
        exif.push((0x9010, Val::Ascii(o.clone())));
    }
    if let Some(c) = &spec.user_comment {
        exif.push((0x9286, Val::Undefined([b"ASCII\0\0\0".as_slice(), c.as_bytes()].concat())));
    }
    if let Some(v) = s(&spec.serial) {
        exif.push((0xa431, v));
    }
    if let Some(v) = s(&spec.lens) {
        exif.push((0xa434, v));
    }
    if let (Some(w), Some(h)) = (spec.width, spec.height) {
        exif.push((0xa002, Val::Long(w)));
        exif.push((0xa003, Val::Long(h)));
    }
    if let Some(n) = &spec.maker_note {
        exif.push((0x927c, Val::Undefined(n.clone())));
    }
    ifd0.push((0x8769, Val::Long(0)));
    let gps = spec.gps.map(|(lat, lon, alt)| {
        vec![
            (0x0000, Val::Byte(vec![2, 3, 0, 0])),
            (0x0001, Val::Ascii(if lat < 0.0 { "S" } else { "N" }.into())),
            (0x0002, Val::Rational(dms(lat))),
            (0x0003, Val::Ascii(if lon < 0.0 { "W" } else { "E" }.into())),
            (0x0004, Val::Rational(dms(lon))),
            (0x0005, Val::Byte(vec![u8::from(alt < 0.0)])),
            (0x0006, Val::Rational(vec![rational(alt.abs(), 100)])),
        ]
    });
    if gps.is_some() {
        ifd0.push((0x8825, Val::Long(0)));
    }

    let mut out = b"MM\0\x2a\0\0\0\x08".to_vec();
    let mut patches = Vec::new();
    write_ifd(&mut out, ifd0, &mut patches);
    let mut sub_offsets = Vec::new();
    sub_offsets.push((0x8769u16, out.len() as u32));
    write_ifd(&mut out, exif, &mut Vec::new());
    if let Some(g) = gps {
        sub_offsets.push((0x8825, out.len() as u32));
        write_ifd(&mut out, g, &mut Vec::new());
    }
    for (pos, tag) in patches {
        if let Some((_, off)) = sub_offsets.iter().find(|(t, _)| *t == tag) {
            out[pos..pos + 4].copy_from_slice(&off.to_be_bytes());
        }
    }
    out
}

/// Insert an EXIF APP1 segment right after the JPEG SOI marker.
pub fn insert_into_jpeg(jpeg: &[u8], tiff: &[u8]) -> Vec<u8> {
    let mut app1 = vec![0xFF, 0xE1];
    let len = (tiff.len() + 6 + 2) as u16;
    app1.extend_from_slice(&len.to_be_bytes());
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend_from_slice(tiff);
    [&jpeg[..2], &app1, &jpeg[2..]].concat()
}

/// An Apple maker note with a burst UUID (0x000b) and/or a Live Photo
/// ContentIdentifier (0x0011). Offsets are relative to the note start.
pub fn apple_maker_note(content_id: Option<&str>, burst_id: Option<&str>) -> Vec<u8> {
    let entries: Vec<(u16, Vec<u8>)> = [(0x000bu16, burst_id), (0x0011, content_id)]
        .into_iter()
        .filter_map(|(t, v)| v.map(|v| (t, [v.as_bytes(), b"\0"].concat())))
        .collect();
    let mut note = b"Apple iOS\0\0\x01MM".to_vec();
    note.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    let mut data_off = 16 + entries.len() * 12 + 4;
    let mut data = Vec::new();
    for (tag, bytes) in &entries {
        note.extend_from_slice(&tag.to_be_bytes());
        note.extend_from_slice(&2u16.to_be_bytes());
        note.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        note.extend_from_slice(&(data_off as u32).to_be_bytes());
        data.extend_from_slice(bytes);
        data_off += bytes.len();
    }
    note.extend_from_slice(&[0; 4]);
    note.extend_from_slice(&data);
    note
}
