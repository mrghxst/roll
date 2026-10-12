//! Types shared with the UI. `cargo test -p roll-core` writes their
//! TypeScript versions to `apps/desktop/src/lib/ipc/bindings/`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Problem flags on an asset (bit set in `assets.problems`).
pub mod problem {
    pub const NO_DATE: i64 = 1;
    pub const SUSPICIOUS_DATE: i64 = 2;
    pub const DATE_CHANGED: i64 = 4;
    pub const NO_LOCATION: i64 = 8;
    pub const BROKEN: i64 = 16;
    pub const NO_OFFSET: i64 = 32;
}

/// `assets.missing`: 0 present in the archive, 1 gone from disk, 2 present
/// in a staging root (an import being checked, not part of the archive).
pub mod presence {
    pub const ARCHIVE: i64 = 0;
    pub const GONE: i64 = 1;
    pub const STAGED: i64 = 2;
}

/// Which items a check looks at: the archive, or one import's staging set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Archive,
    Staged(i64),
}

impl Scope {
    /// A WHERE condition on `assets` (with an optional table alias).
    pub fn sql(&self, alias: &str) -> String {
        let a = if alias.is_empty() { String::new() } else { format!("{alias}.") };
        match self {
            Scope::Archive => format!("{a}missing = 0"),
            Scope::Staged(root) => format!("{a}root_id = {root} AND {a}missing = 2"),
        }
    }
}

/// Tile flags packed into the timeline payload.
pub mod tile {
    pub const VIDEO: u16 = 1;
    pub const LIVE: u16 = 2;
    pub const BURST: u16 = 4;
    pub const RAW: u16 = 8;
    pub const FAVORITE: u16 = 16;
    pub const PROBLEM: u16 = 32;
    pub const IMMICH: u16 = 64;
    pub const PRIVATE: u16 = 128;
    pub const EDITED: u16 = 256;
    pub const NO_THUMB: u16 = 512;
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RootInfo {
    pub id: i32,
    pub path: String,
    pub kind: String,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Category {
    pub id: i32,
    pub key: String,
    pub name: String,
    pub parent_id: Option<i32>,
    pub definition: String,
    pub folder_template: Option<String>,
    pub own_folder: bool,
    pub immich_default: bool,
    pub auto_threshold: Option<f32>,
    pub builtin: bool,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TagCount {
    pub id: i32,
    pub path: String,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FolderNode {
    pub root_id: i32,
    /// Path relative to the root, `/` separated; empty for the root itself.
    pub path: String,
    pub name: String,
    /// Items directly in this folder.
    pub count: i32,
    /// Items in this folder and below.
    pub total: i32,
    pub children: Vec<FolderNode>,
}

/// Why a field has its value.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Why {
    pub field: String,
    pub source: String,
    pub text: String,
    pub conf: Option<f32>,
    #[ts(type = "number")]
    pub at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AssetDetail {
    pub id: i32,
    pub root_id: i32,
    pub path: String,
    pub rel_path: String,
    pub name: String,
    pub kind: String,
    pub format: String,
    #[ts(type = "number")]
    pub size: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration_ms: Option<i32>,
    pub fps: Option<f32>,
    pub codec: Option<String>,
    pub has_audio: Option<bool>,
    #[ts(type = "number | null")]
    pub taken_utc: Option<i64>,
    pub taken_local: Option<String>,
    pub tz_offset: Option<i32>,
    pub date_source: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub loc_acc: Option<f64>,
    pub loc_source: Option<String>,
    pub place: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub software: Option<String>,
    pub category: Option<String>,
    pub category_name: Option<String>,
    pub category_conf: Option<f32>,
    pub review: i32,
    pub tags: Vec<String>,
    pub rating: i32,
    pub favorite: bool,
    pub description: Option<String>,
    pub private: bool,
    pub immich: String,
    pub immich_id: Option<String>,
    pub group_kind: Option<String>,
    pub group_role: Option<String>,
    pub group_members: Vec<i32>,
    pub problems: i32,
    pub thumb_v: i32,
    pub why: Vec<Why>,
    pub sidecars: Vec<String>,
    pub missing: bool,
    pub trashed: bool,
}

/// Lightweight per-tile info fetched lazily for visible ranges.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TileInfo {
    pub id: i32,
    pub name: String,
    pub thumb_v: i32,
    pub thumbhash: Option<String>,
    pub duration_ms: Option<i32>,
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScanReport {
    pub root_id: i32,
    pub seen: i32,
    pub added: i32,
    pub updated: i32,
    pub relinked: i32,
    pub missing: i32,
    pub placeholders: i32,
    pub sidecars: i32,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct JobInfo {
    pub id: i32,
    pub kind: String,
    pub label: String,
    pub state: String,
    pub done: i32,
    pub total: i32,
    pub rate: f32,
    pub eta_secs: Option<i32>,
    pub cost_usd: f64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BatchInfo {
    pub id: i32,
    pub kind: String,
    pub label: String,
    pub state: String,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number | null")]
    pub applied_at: Option<i64>,
    pub ops: i32,
    pub failed: i32,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SavedFilter {
    pub id: i32,
    pub name: String,
    pub query: Query,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EventInfo {
    pub id: i32,
    pub name: String,
    #[ts(type = "number | null")]
    pub start_utc: Option<i64>,
    #[ts(type = "number | null")]
    pub end_utc: Option<i64>,
    pub place: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub state: String,
    pub own_folder: bool,
    pub immich: String,
    pub count: i32,
    pub cover: Option<i32>,
}

/// A structured filter. Every field narrows the result; empty matches all.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(default)]
pub struct Query {
    pub text: Option<String>,
    #[ts(type = "number | null")]
    pub from_utc: Option<i64>,
    #[ts(type = "number | null")]
    pub to_utc: Option<i64>,
    /// Local wall-clock bounds, "YYYY-MM-DDTHH:MM:SS": from inclusive, to
    /// exclusive. Years, months and seasons in search use these.
    pub from_local: Option<String>,
    pub to_local: Option<String>,
    /// "photo" or "video", plus "live", "burst", "raw".
    pub media: Vec<String>,
    pub categories: Vec<String>,
    pub exclude_categories: Vec<String>,
    pub tags: Vec<String>,
    pub event_id: Option<i32>,
    pub cameras: Vec<String>,
    pub lenses: Vec<String>,
    pub formats: Vec<String>,
    /// "any", "none", "approximate", "exact"
    pub location: Option<String>,
    /// [south, west, north, east]
    pub bbox: Option<Vec<f64>>,
    pub root_id: Option<i32>,
    pub folder: Option<String>,
    pub recursive: bool,
    pub min_rating: Option<i32>,
    pub favorite: Option<bool>,
    /// Problem bits; matches items with any of them.
    pub problems: Option<i32>,
    /// 0..3 review states.
    pub review: Vec<i32>,
    /// "sent", "local", "pending"
    pub immich: Option<String>,
    pub orientation: Option<String>,
    pub private: Option<bool>,
    pub inbox: Option<bool>,
    pub trashed: bool,
    pub missing: bool,
    /// The staging root of an import: its items instead of the archive's.
    pub staged: Option<i32>,
    pub ids: Option<Vec<i32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Facet {
    pub key: String,
    pub label: String,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Facets {
    pub categories: Vec<Facet>,
    pub media: Vec<Facet>,
    pub cameras: Vec<Facet>,
    pub lenses: Vec<Facet>,
    pub formats: Vec<Facet>,
    pub years: Vec<Facet>,
    pub location: Vec<Facet>,
    pub problems: Vec<Facet>,
    pub tags: Vec<Facet>,
    pub immich: Vec<Facet>,
    pub orientation: Vec<Facet>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Counts {
    pub total: i32,
    pub inbox: i32,
    pub review: i32,
    pub date_issues: i32,
    pub no_location: i32,
    pub trash: i32,
    pub duplicates: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub exiftool: Option<String>,
    pub ffmpeg: Option<String>,
    pub os: String,
}
