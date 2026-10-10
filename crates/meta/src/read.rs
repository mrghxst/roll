//! Reads one media file's headers into a [`FileMeta`]. Fast path only:
//! nothing here spawns a process.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use nom_exif::{Exif, MediaParser, MediaSource, TrackInfoTag};

use crate::detect::{classify_name, extension, format_of, mime_of, FileClass};
use crate::model::{DateSource, FileMeta, MediaKind, Taken};
use crate::{exif, jpeg, quicktime, xmp};

/// Bytes read from the start of each file for sniffing, JPEG markers and XMP.
const HEAD: usize = 256 * 1024;

/// Holds the reusable nom-exif buffer. Use one per thread.
pub struct MetaReader {
    parser: MediaParser,
}

impl Default for MetaReader {
    fn default() -> Self {
        Self { parser: MediaParser::new() }
    }
}

impl MetaReader {
    pub fn read(&mut self, path: &Path) -> std::io::Result<FileMeta> {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = extension(&name);
        let mut file = File::open(path)?;
        let mut head = Vec::with_capacity(HEAD);
        (&mut file).take(HEAD as u64).read_to_end(&mut head)?;
        let format = format_of(&ext, &head);
        let kind = match classify_name(&name) {
            FileClass::Media(k) => k,
            _ => MediaKind::Photo,
        };
        let mut m = FileMeta { kind, format: format.to_string(), mime: mime_of(format).to_string(), ..Default::default() };
        match kind {
            MediaKind::Photo => {
                if format == "jpeg" {
                    m.truncated = jpeg_truncated(&mut file, head.len())?;
                }
                self.read_photo(path, &head, &mut m)
            }
            MediaKind::Video => self.read_video(path, file, &mut m),
        }
        Ok(m)
    }

    fn read_photo(&mut self, path: &Path, head: &[u8], m: &mut FileMeta) {
        if m.format == "jpeg" {
            let j = jpeg::scan(head);
            m.width = j.width;
            m.height = j.height;
            m.icc_profile = j.icc_profile;
            m.qtable = j.qtable;
        }
        if m.width.is_none() {
            if let Ok(s) = imagesize::blob_size(head).or_else(|_| imagesize::size(path)) {
                m.width = Some(s.width as u32);
                m.height = Some(s.height as u32);
            }
        }
        if matches!(m.format.as_str(), "jpeg" | "heic" | "avif" | "png" | "tiff" | "raw" | "webp") {
            match MediaSource::open(path).and_then(|ms| self.parser.parse_exif(ms)) {
                Ok(iter) => {
                    let e: Exif = iter.into();
                    exif::apply(&e, m);
                }
                Err(err) => {
                    let msg = err.to_string();
                    // "No EXIF" is normal for saved images and screenshots.
                    if !msg.to_ascii_lowercase().contains("not found") {
                        m.warnings.push(format!("exif: {msg}"));
                    }
                }
            }
        }
        if let Some(packet) = xmp::find_packet(head) {
            m.has_xmp = true;
            merge_xmp(m, &xmp::parse(packet));
        }
    }

    fn read_video(&mut self, path: &Path, file: File, m: &mut FileMeta) {
        if matches!(m.format.as_str(), "mov" | "mp4" | "3gp") {
            match quicktime::read(&mut BufReader::new(file)) {
                Ok(Some(q)) => {
                    m.width = q.width;
                    m.height = q.height;
                    m.duration_ms = q.duration_ms;
                    m.fps = q.fps;
                    m.codec = q.codec.clone();
                    m.rotation = q.rotation;
                    m.has_audio = Some(q.has_audio);
                    m.taken = q.taken();
                    m.gps = q.gps();
                    m.content_id = q.content_id.clone();
                    m.make = q.make.clone();
                    m.model = q.model.clone();
                    m.software = q.software.clone();
                    return;
                }
                Ok(None) => m.warnings.push("quicktime: no movie header".into()),
                Err(e) => m.warnings.push(format!("quicktime: {e}")),
            }
        }
        match MediaSource::open(path).and_then(|ms| self.parser.parse_track(ms)) {
            Ok(t) => {
                let num = |tag| t.get(tag).and_then(|v: &nom_exif::EntryValue| v.try_as_integer());
                m.width = num(TrackInfoTag::Width).map(|v| v as u32);
                m.height = num(TrackInfoTag::Height).map(|v| v as u32);
                m.duration_ms = num(TrackInfoTag::DurationMs).map(|v| v as u64);
                m.make = t.get(TrackInfoTag::Make).and_then(|v| v.as_str()).map(String::from);
                m.model = t.get(TrackInfoTag::Model).and_then(|v| v.as_str()).map(String::from);
                if let Some(dt) = t.get(TrackInfoTag::CreateDate).and_then(|v| v.as_datetime()) {
                    m.taken = Some(match dt.aware() {
                        Some(a) => Taken::new(a.naive_local(), Some(a.offset().local_minus_utc() / 60), DateSource::QuickTime),
                        None => Taken::new(dt.into_naive(), None, DateSource::QuickTime),
                    });
                }
            }
            Err(e) => m.warnings.push(format!("track: {e}")),
        }
    }
}

/// A JPEG without an end marker near its end was cut off. Some JPEGs carry
/// a video or other data after the image (motion photos), so `ftyp` or a
/// later end marker also count as complete.
fn jpeg_truncated(file: &mut File, head_len: usize) -> std::io::Result<bool> {
    use std::io::{Seek, SeekFrom};
    let len = file.metadata()?.len();
    let tail_len = len.min(64 * 1024);
    file.seek(SeekFrom::Start(len - tail_len))?;
    let mut tail = Vec::with_capacity(tail_len as usize);
    file.take(tail_len).read_to_end(&mut tail)?;
    let found = tail.windows(2).any(|w| w == [0xFF, 0xD9]) || tail.windows(4).any(|w| w == b"ftyp" || w == b"mdat");
    Ok(!found && len as usize > head_len.min(1024))
}

/// XMP fills gaps left by EXIF. Tags only exist in XMP.
pub fn merge_xmp(m: &mut FileMeta, x: &xmp::XmpData) {
    for t in x.tags() {
        if !m.tags.contains(&t) {
            m.tags.push(t);
        }
    }
    if x.is_screenshot() {
        m.screenshot_marker = true;
    }
    if m.rating.is_none() {
        m.rating = x.rating();
    }
    if m.description.is_none() {
        m.description = x.description.clone();
    }
    if m.taken.is_none() {
        m.taken = x.taken();
    }
    if m.gps.is_none() {
        m.gps = x.gps();
    }
    if m.make.is_none() {
        m.make = x.make.clone();
    }
    if m.model.is_none() {
        m.model = x.model.clone();
    }
    if m.software.is_none() {
        m.software = x.creator_tool.clone();
    }
}

/// Apply an `.xmp` sidecar on top of the file's own values. Sidecar values
/// win, as they do in Immich.
pub fn apply_sidecar(m: &mut FileMeta, x: &xmp::XmpData) {
    let tags = x.tags();
    if !tags.is_empty() {
        m.tags = tags;
    }
    if let Some(t) = x.taken() {
        m.taken = Some(Taken { source: DateSource::Sidecar, ..t });
    }
    if let Some(g) = x.gps() {
        m.gps = Some(g);
    }
    if let Some(r) = x.rating() {
        m.rating = Some(r);
    }
    if let Some(d) = &x.description {
        m.description = Some(d.clone());
    }
    m.has_xmp = true;
}
