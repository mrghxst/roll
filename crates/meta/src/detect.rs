use crate::model::MediaKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileClass {
    Media(MediaKind),
    Sidecar,
    Ignored,
}

const PHOTO_EXT: &[&str] =
    &["jpg", "jpeg", "jpe", "jfif", "heic", "heif", "hif", "avif", "png", "gif", "webp", "tif", "tiff", "bmp", "jxl"];
const RAW_EXT: &[&str] = &[
    "dng", "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef", "srw", "x3f", "3fr", "erf", "kdc", "mrw",
    "iiq", "raw",
];
const VIDEO_EXT: &[&str] = &["mov", "mp4", "m4v", "3gp", "3g2", "avi", "mkv", "webm", "mts", "m2ts", "wmv", "mpg", "mpeg"];
const SIDECAR_EXT: &[&str] = &["xmp", "aae", "json"];

/// Names that are never indexed: OS metadata files and partial downloads.
pub fn is_ignored_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "thumbs.db"
        || lower == "desktop.ini"
        || lower == ".ds_store"
        || lower.starts_with("._")
        || lower.starts_with("~$")
        || lower.ends_with(".tmp")
        || lower.ends_with(".part")
        || lower.ends_with(".crdownload")
}

pub fn extension(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

pub fn classify_name(name: &str) -> FileClass {
    if is_ignored_name(name) {
        return FileClass::Ignored;
    }
    let ext = extension(name);
    let e = ext.as_str();
    if PHOTO_EXT.contains(&e) || RAW_EXT.contains(&e) {
        FileClass::Media(MediaKind::Photo)
    } else if VIDEO_EXT.contains(&e) {
        FileClass::Media(MediaKind::Video)
    } else if SIDECAR_EXT.contains(&e) {
        FileClass::Sidecar
    } else {
        FileClass::Ignored
    }
}

pub fn is_raw_ext(ext: &str) -> bool {
    RAW_EXT.contains(&ext)
}

/// Normalized format name from the extension, refined by magic bytes when
/// the extension lies (a PNG saved as `.jpg` is common with saved images).
pub fn format_of(ext: &str, head: &[u8]) -> &'static str {
    if let Some(f) = sniff(head) {
        return f;
    }
    match ext {
        "jpg" | "jpeg" | "jpe" | "jfif" => "jpeg",
        "heic" | "heif" | "hif" => "heic",
        "tif" | "tiff" => "tiff",
        "mpg" | "mpeg" => "mpeg",
        "m2ts" | "mts" => "mts",
        e if is_raw_ext(e) => "raw",
        "avif" => "avif",
        "png" => "png",
        "gif" => "gif",
        "webp" => "webp",
        "bmp" => "bmp",
        "jxl" => "jxl",
        "mov" => "mov",
        "mp4" | "m4v" => "mp4",
        "3gp" | "3g2" => "3gp",
        "avi" => "avi",
        "mkv" => "mkv",
        "webm" => "webm",
        "wmv" => "wmv",
        _ => "unknown",
    }
}

fn sniff(h: &[u8]) -> Option<&'static str> {
    if h.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpeg");
    }
    if h.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if h.starts_with(b"GIF87a") || h.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if h.len() >= 12 && &h[0..4] == b"RIFF" && &h[8..12] == b"WEBP" {
        return Some("webp");
    }
    if h.len() >= 12 && &h[4..8] == b"ftyp" {
        let brand = &h[8..12];
        return Some(match brand {
            b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"mif1" | b"msf1" => "heic",
            b"avif" | b"avis" => "avif",
            b"qt  " => "mov",
            b"crx " => "raw",
            _ => "mp4",
        });
    }
    if h.starts_with(&[0xFF, 0x0A]) || h.starts_with(&[0, 0, 0, 0x0C, b'J', b'X', b'L', b' ']) {
        return Some("jxl");
    }
    None
}

pub fn mime_of(format: &str) -> &'static str {
    match format {
        "jpeg" => "image/jpeg",
        "heic" => "image/heic",
        "avif" => "image/avif",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "tiff" => "image/tiff",
        "bmp" => "image/bmp",
        "jxl" => "image/jxl",
        "raw" => "image/x-raw",
        "mov" => "video/quicktime",
        "mp4" => "video/mp4",
        "3gp" => "video/3gpp",
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "wmv" => "video/x-ms-wmv",
        "mpeg" => "video/mpeg",
        "mts" => "video/mp2t",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies() {
        assert_eq!(classify_name("IMG_1234.HEIC"), FileClass::Media(MediaKind::Photo));
        assert_eq!(classify_name("clip.MOV"), FileClass::Media(MediaKind::Video));
        assert_eq!(classify_name("IMG_1234.HEIC.xmp"), FileClass::Sidecar);
        assert_eq!(classify_name("Thumbs.db"), FileClass::Ignored);
        assert_eq!(classify_name("._IMG_1.JPG"), FileClass::Ignored);
        assert_eq!(classify_name("notes.txt"), FileClass::Ignored);
    }

    #[test]
    fn sniffs_lying_extension() {
        assert_eq!(format_of("jpg", b"\x89PNG\r\n\x1a\n...."), "png");
        assert_eq!(format_of("mov", b"\0\0\0\x18ftypqt  "), "mov");
        assert_eq!(format_of("heic", b"\0\0\0\x18ftypheic"), "heic");
        assert_eq!(format_of("nef", b"II*\0"), "raw");
    }
}
