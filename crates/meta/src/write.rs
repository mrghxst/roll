//! Writes Roll's fields into files and `<file>.<ext>.xmp` sidecars, using
//! the mapping in PROMPT.md section 6. Every write first takes a snapshot of
//! the values it is about to replace, so undo can restore them exactly.

use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::exiftool::{ExifTool, ExifToolError, ExifToolPool, arg};
use crate::model::{Gps, MediaKind, Taken, format_offset};
use crate::tags;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MetaEdit {
    pub taken: Option<Taken>,
    /// `Some(None)` clears the location.
    pub gps: Option<Option<Gps>>,
    /// The complete tag set; replaces the lists in the file.
    pub tags: Option<Vec<String>>,
    /// `Some(None)` clears the description.
    pub description: Option<Option<String>>,
    /// 0 clears the rating.
    pub rating: Option<u8>,
    pub orientation: Option<u16>,
}

impl MetaEdit {
    pub fn is_empty(&self) -> bool {
        self == &MetaEdit::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WriteMode {
    #[default]
    FileAndSidecar,
    SidecarOnly,
}

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error(transparent)]
    Tool(#[from] ExifToolError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("The image data changed while writing metadata. The previous values were restored.")]
    ImageDataChanged,
}

/// What a write replaced: enough to put everything back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Tags assigned in the file (family 0 names).
    pub touched: Vec<String>,
    /// Previous values of those tags (family 1 names, raw values).
    pub previous: Vec<(String, Value)>,
    /// Previous sidecar content (base64), or `None` if there was none.
    pub sidecar_before: Option<String>,
    pub sidecar_written: bool,
    pub image_hash: Option<String>,
}

pub fn sidecar_path(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_owned();
    s.push(".xmp");
    PathBuf::from(s)
}

/// Formats where Roll writes into the file itself. Everything else (RAW,
/// GIF, BMP, AVI, MKV…) gets a sidecar only.
pub fn supports_in_file(format: &str) -> bool {
    matches!(format, "jpeg" | "heic" | "avif" | "png" | "tiff" | "webp" | "mov" | "mp4" | "3gp")
}

const EMPTY_XMP: &str = "<?xpacket begin='\u{feff}' id='W5M0MpCehiHzreSzNTczkc9d'?>\n<x:xmpmeta xmlns:x='adobe:ns:meta/'>\n<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n</rdf:RDF>\n</x:xmpmeta>\n<?xpacket end='w'?>\n";

struct Plan {
    assign: Vec<String>,
    touched: Vec<String>,
}

impl Plan {
    fn new() -> Self {
        Self { assign: Vec::new(), touched: Vec::new() }
    }
    fn set(&mut self, tag: &str, value: impl std::fmt::Display) {
        self.assign.push(arg(&format!("-{tag}={value}")));
        self.touch(tag);
    }
    fn set_num(&mut self, tag: &str, value: impl std::fmt::Display) {
        self.assign.push(format!("-{tag}#={value}"));
        self.touch(tag);
    }
    fn clear(&mut self, tag: &str) {
        self.assign.push(format!("-{tag}="));
        self.touch(tag);
    }
    fn list(&mut self, tag: &str, values: &[String]) {
        self.clear(tag);
        for v in values {
            self.assign.push(arg(&format!("-{tag}={v}")));
        }
    }
    fn touch(&mut self, tag: &str) {
        if !self.touched.iter().any(|t| t == tag) {
            self.touched.push(tag.to_string());
        }
    }
}

fn exif_local(t: &Taken) -> String {
    t.local.format("%Y:%m:%d %H:%M:%S").to_string()
}

fn with_offset(t: &Taken) -> String {
    match t.offset_min {
        Some(m) => format!("{}{}", exif_local(t), format_offset(m)),
        None => exif_local(t),
    }
}

fn rating_percent(r: u8) -> u8 {
    match r {
        1 => 1,
        2 => 25,
        3 => 50,
        4 => 75,
        _ => 99,
    }
}

fn file_plan(edit: &MetaEdit, kind: MediaKind, format: &str) -> Plan {
    let mut p = Plan::new();
    let video = kind == MediaKind::Video;
    if let Some(t) = &edit.taken {
        if video {
            let utc = chrono::DateTime::from_timestamp_millis(t.utc_ms()).unwrap_or_default();
            let u = utc.format("%Y:%m:%d %H:%M:%S").to_string();
            p.set("QuickTime:CreateDate", &u);
            p.set("QuickTime:TrackCreateDate", &u);
            p.set("QuickTime:MediaCreateDate", &u);
            p.set("Keys:CreationDate", with_offset(t));
        } else {
            p.set("EXIF:DateTimeOriginal", exif_local(t));
            p.set("EXIF:CreateDate", exif_local(t));
            for tag in ["EXIF:OffsetTimeOriginal", "EXIF:OffsetTimeDigitized", "EXIF:OffsetTime"] {
                match t.offset_min {
                    Some(m) => p.set(tag, format_offset(m)),
                    None => p.clear(tag),
                }
            }
        }
    }
    if let Some(g) = &edit.gps {
        if video {
            for tag in ["Keys:GPSCoordinates", "UserData:GPSCoordinates"] {
                match g {
                    Some(g) => p.set(tag, format!("{}, {}, {}", g.lat, g.lon, g.alt.unwrap_or(0.0))),
                    None => p.clear(tag),
                }
            }
        } else {
            match g {
                Some(g) => {
                    p.set_num("EXIF:GPSLatitude", g.lat.abs());
                    p.set("EXIF:GPSLatitudeRef", if g.lat < 0.0 { "S" } else { "N" });
                    p.set_num("EXIF:GPSLongitude", g.lon.abs());
                    p.set("EXIF:GPSLongitudeRef", if g.lon < 0.0 { "W" } else { "E" });
                    match g.alt {
                        Some(a) => {
                            p.set_num("EXIF:GPSAltitude", a.abs());
                            p.set_num("EXIF:GPSAltitudeRef", u8::from(a < 0.0));
                        }
                        None => {
                            p.clear("EXIF:GPSAltitude");
                            p.clear("EXIF:GPSAltitudeRef");
                        }
                    }
                    match g.accuracy_m {
                        Some(a) => p.set_num("EXIF:GPSHPositioningError", a),
                        None => p.clear("EXIF:GPSHPositioningError"),
                    }
                }
                None => {
                    for tag in [
                        "EXIF:GPSLatitude",
                        "EXIF:GPSLatitudeRef",
                        "EXIF:GPSLongitude",
                        "EXIF:GPSLongitudeRef",
                        "EXIF:GPSAltitude",
                        "EXIF:GPSAltitudeRef",
                        "EXIF:GPSHPositioningError",
                    ] {
                        p.clear(tag);
                    }
                }
            }
        }
    }
    if let Some(list) = &edit.tags {
        let f = tags::serialize(list);
        p.list("XMP-dc:Subject", &f.subjects);
        p.list("XMP-digiKam:TagsList", &f.tags_list);
        p.list("XMP-lr:HierarchicalSubject", &f.hierarchical);
        if matches!(format, "jpeg" | "tiff") {
            p.set("IPTC:CodedCharacterSet", "UTF8");
            p.list("IPTC:Keywords", &f.subjects);
        }
    }
    if let Some(d) = &edit.description {
        let tags: &[&str] = if video { &["XMP-dc:Description"] } else { &["EXIF:ImageDescription", "XMP-dc:Description"] };
        for tag in tags {
            match d {
                Some(d) => p.set(tag, d),
                None => p.clear(tag),
            }
        }
    }
    if let Some(r) = edit.rating {
        if r > 0 {
            p.set_num("XMP-xmp:Rating", r);
            if !video {
                p.set_num("EXIF:Rating", r);
                p.set_num("EXIF:RatingPercent", rating_percent(r));
            }
        } else {
            p.clear("XMP-xmp:Rating");
            if !video {
                p.clear("EXIF:Rating");
                p.clear("EXIF:RatingPercent");
            }
        }
    }
    if let (Some(o), false) = (edit.orientation, video) {
        p.set_num("EXIF:Orientation", o);
    }
    p
}

fn sidecar_plan(edit: &MetaEdit) -> Plan {
    let mut p = Plan::new();
    if let Some(list) = &edit.tags {
        let f = tags::serialize(list);
        p.list("XMP-digiKam:TagsList", &f.tags_list);
        p.list("XMP-lr:HierarchicalSubject", &f.hierarchical);
        p.list("XMP-dc:Subject", &f.subjects);
    }
    if let Some(t) = &edit.taken {
        let v = with_offset(t);
        p.set("XMP-exif:DateTimeOriginal", &v);
        p.set("XMP-photoshop:DateCreated", &v);
        p.set("XMP-xmp:CreateDate", &v);
    }
    if let Some(g) = &edit.gps {
        match g {
            Some(g) => {
                p.set_num("XMP-exif:GPSLatitude", g.lat);
                p.set_num("XMP-exif:GPSLongitude", g.lon);
                match g.alt {
                    Some(a) => {
                        p.set_num("XMP-exif:GPSAltitude", a.abs());
                        p.set_num("XMP-exif:GPSAltitudeRef", u8::from(a < 0.0));
                    }
                    None => {
                        p.clear("XMP-exif:GPSAltitude");
                        p.clear("XMP-exif:GPSAltitudeRef");
                    }
                }
                match g.accuracy_m {
                    Some(a) => p.set_num("XMP-exif:GPSHPositioningError", a),
                    None => p.clear("XMP-exif:GPSHPositioningError"),
                }
            }
            None => {
                for tag in [
                    "XMP-exif:GPSLatitude",
                    "XMP-exif:GPSLongitude",
                    "XMP-exif:GPSAltitude",
                    "XMP-exif:GPSAltitudeRef",
                    "XMP-exif:GPSHPositioningError",
                ] {
                    p.clear(tag);
                }
            }
        }
    }
    if let Some(d) = &edit.description {
        match d {
            Some(d) => p.set("XMP-dc:Description", d),
            None => p.clear("XMP-dc:Description"),
        }
    }
    if let Some(r) = edit.rating {
        if r > 0 {
            p.set_num("XMP-xmp:Rating", r);
        } else {
            p.clear("XMP-xmp:Rating");
        }
    }
    p
}

/// Previous tag values and the image data hash.
type Values = (Vec<(String, Value)>, Option<String>);

fn read_values(tool: &mut ExifTool, file: &Path, touched: &[String]) -> Result<Values, ExifToolError> {
    let mut extra: Vec<String> = touched.iter().map(|t| format!("-{t}")).collect();
    extra.push("-ImageDataHash".into());
    let refs: Vec<&str> = extra.iter().map(String::as_str).collect();
    let json = tool.read_json(&[file], &refs)?;
    let mut previous = Vec::new();
    let mut hash = None;
    if let Some(Value::Object(obj)) = json.into_iter().next() {
        for (k, v) in obj {
            if k == "SourceFile" {
                continue;
            }
            if k.ends_with(":ImageDataHash") || k == "ImageDataHash" {
                hash = v.as_str().map(String::from);
                continue;
            }
            previous.push((k, v));
        }
    }
    Ok((previous, hash))
}

/// Write an edit. Returns the snapshot needed to undo it.
/// Read what an edit is about to replace. Store this before calling
/// [`perform`], so an interrupted write can still be undone.
pub fn snapshot(
    pool: &ExifToolPool,
    file: &Path,
    kind: MediaKind,
    format: &str,
    edit: &MetaEdit,
    mode: WriteMode,
) -> Result<Snapshot, WriteError> {
    let mut snap = Snapshot::default();
    if edit.is_empty() {
        return Ok(snap);
    }
    if mode == WriteMode::FileAndSidecar && supports_in_file(format) {
        let plan = file_plan(edit, kind, format);
        let (previous, hash) = pool.with(|tool| read_values(tool, file, &plan.touched))?;
        snap.touched = plan.touched;
        snap.previous = previous;
        snap.image_hash = hash;
    }
    let before = std::fs::read(sidecar_path(file)).ok();
    snap.sidecar_before = before.map(|b| base64::engine::general_purpose::STANDARD.encode(b));
    snap.sidecar_written = true;
    Ok(snap)
}

/// Write an edit whose snapshot was taken. Writing the same edit twice is
/// harmless, which makes an interrupted write safe to repeat.
pub fn perform(
    pool: &ExifToolPool,
    file: &Path,
    kind: MediaKind,
    format: &str,
    edit: &MetaEdit,
    snap: &Snapshot,
) -> Result<(), WriteError> {
    if edit.is_empty() {
        return Ok(());
    }
    let image_changed = pool.with(|tool| -> Result<bool, ExifToolError> {
        if !snap.touched.is_empty() {
            let mut assign = file_plan(edit, kind, format).assign;
            if edit.tags.is_some() && matches!(format, "jpeg" | "tiff") {
                assign.splice(0..0, ["-charset".to_string(), "iptc=UTF8".to_string()]);
            }
            tool.write(file, &assign)?;
            let after = read_values(tool, file, &[])?.1;
            if snap.image_hash.is_some() && after != snap.image_hash {
                restore_in_file(tool, file, snap)?;
                return Ok(true);
            }
        }
        let side = sidecar_path(file);
        let existed = side.exists();
        if !existed {
            std::fs::write(&side, EMPTY_XMP).map_err(|e| ExifToolError::Failed(e.to_string()))?;
        }
        if let Err(e) = tool.write(&side, &sidecar_plan(edit).assign) {
            if !existed {
                let _ = std::fs::remove_file(&side);
            }
            return Err(e);
        }
        Ok(false)
    })?;
    if image_changed {
        return Err(WriteError::ImageDataChanged);
    }
    Ok(())
}

/// Snapshot and write in one step. Returns the snapshot needed to undo it.
pub fn apply(
    pool: &ExifToolPool,
    file: &Path,
    kind: MediaKind,
    format: &str,
    edit: &MetaEdit,
    mode: WriteMode,
) -> Result<Snapshot, WriteError> {
    let snap = snapshot(pool, file, kind, format, edit, mode)?;
    perform(pool, file, kind, format, edit, &snap)?;
    Ok(snap)
}

fn restore_in_file(tool: &mut ExifTool, file: &Path, snap: &Snapshot) -> Result<(), ExifToolError> {
    if snap.touched.is_empty() {
        return Ok(());
    }
    let mut assign: Vec<String> = snap.touched.iter().map(|t| format!("-{t}=")).collect();
    for (k, v) in &snap.previous {
        match v {
            Value::Array(items) => {
                for i in items {
                    assign.push(arg(&format!("-{k}#={}", plain(i))));
                }
            }
            v => assign.push(arg(&format!("-{k}#={}", plain(v)))),
        }
    }
    tool.write(file, &assign)
}

fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Put back what [`apply`] replaced.
pub fn restore(pool: &ExifToolPool, file: &Path, snap: &Snapshot) -> Result<(), WriteError> {
    if !snap.touched.is_empty() {
        pool.with(|tool| restore_in_file(tool, file, snap))?;
    }
    if snap.sidecar_written {
        let side = sidecar_path(file);
        match &snap.sidecar_before {
            Some(b64) => {
                let bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| WriteError::Io(std::io::Error::other(e)))?;
                std::fs::write(&side, bytes)?;
            }
            None => {
                if side.exists() {
                    std::fs::remove_file(&side)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DateSource, parse_datetime};

    fn edit() -> MetaEdit {
        let (local, _) = parse_datetime("2019:08:12 14:03:22").unwrap();
        MetaEdit {
            taken: Some(Taken::new(local, Some(120), DateSource::ExifOriginal)),
            gps: Some(Some(Gps { lat: 45.64, lon: -10.68, alt: Some(-3.0), accuracy_m: Some(5000.0) })),
            tags: Some(vec!["Category/Memes".into(), "Favorite".into()]),
            description: Some(Some("Line one\nline two".into())),
            rating: Some(4),
            orientation: None,
        }
    }

    #[test]
    fn jpeg_plan_covers_explorer_fields() {
        let p = file_plan(&edit(), MediaKind::Photo, "jpeg");
        let all = p.assign.join("\n");
        assert!(all.contains("-EXIF:DateTimeOriginal=2019:08:12 14:03:22"));
        assert!(all.contains("-EXIF:OffsetTimeOriginal=+02:00"));
        assert!(all.contains("-EXIF:GPSLongitudeRef=W"));
        assert!(all.contains("-EXIF:GPSAltitudeRef#=1"));
        assert!(all.contains("-IPTC:Keywords=Memes"));
        assert!(all.contains("-XMP-digiKam:TagsList=Category/Memes"));
        assert!(all.contains("-XMP-lr:HierarchicalSubject=Category|Memes"));
        assert!(all.contains("-EXIF:RatingPercent#=75"));
        assert!(all.contains("#[CSTR]-EXIF:ImageDescription=Line one\\nline two"));
    }

    #[test]
    fn video_plan_uses_utc() {
        let p = file_plan(&edit(), MediaKind::Video, "mov");
        let all = p.assign.join("\n");
        assert!(all.contains("-QuickTime:CreateDate=2019:08:12 12:03:22"));
        assert!(all.contains("-Keys:CreationDate=2019:08:12 14:03:22+02:00"));
        assert!(!all.contains("IPTC"));
    }

    #[test]
    fn sidecar_plan_has_offset() {
        let p = sidecar_plan(&edit());
        let all = p.assign.join("\n");
        assert!(all.contains("-XMP-exif:DateTimeOriginal=2019:08:12 14:03:22+02:00"));
        assert!(all.contains("-XMP-exif:GPSLatitude#=45.64"));
    }

    #[test]
    fn sidecar_name() {
        assert_eq!(sidecar_path(Path::new("a/IMG_1.HEIC")), PathBuf::from("a/IMG_1.HEIC.xmp"));
    }
}
