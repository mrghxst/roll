//! Metadata for Roll: fast native reading (nom-exif plus small parsers for
//! JPEG markers, QuickTime boxes and XMP), sidecars, and writing through
//! ExifTool with the Immich-compatible mapping.

pub mod detect;
pub mod exif;
pub mod exiftool;
pub mod filename;
pub mod jpeg;
pub mod model;
pub mod quicktime;
pub mod read;
pub mod tags;
pub mod takeout;
pub mod write;
pub mod xmp;

pub use model::{DateSource, FileMeta, Gps, MediaKind, Taken};
pub use read::MetaReader;
