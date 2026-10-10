use serde::Serialize;

/// Ground truth for one generated file.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Item {
    /// Path relative to the output folder, with `/` separators.
    pub path: String,
    pub kind: &'static str,
    /// Expected category key; `None` for sidecars and broken files.
    pub category: Option<&'static str>,
    /// Decidable by default rules alone (must be 100% correct).
    pub rule_decidable: bool,
    pub tags: Vec<String>,
    /// True capture time, local wall time `YYYY-MM-DDTHH:MM:SS`.
    pub true_local: Option<String>,
    pub true_offset_min: Option<i32>,
    pub true_gps: Option<[f64; 2]>,
    /// The file itself carries GPS coordinates.
    pub has_gps: bool,
    /// What the file claims, when it differs from the truth.
    pub file_local: Option<String>,
    /// The zone of `file_local` when the file states no offset.
    pub file_offset_min: Option<i32>,
    pub camera: Option<String>,
    /// Items that must move together (`live:…`, `raw:…`, `edit:…`, `burst:…`).
    pub group: Option<String>,
    pub duplicate_of: Option<String>,
    pub flags: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackFile {
    pub path: String,
    pub format: &'static str,
    pub covers: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct CameraSet {
    pub camera: &'static str,
    /// Seconds to add to the file's time to get the true time.
    pub true_offset_secs: i64,
    pub note: &'static str,
}

#[derive(Debug, Default, Serialize)]
pub struct Manifest {
    pub generator: &'static str,
    pub seed: u64,
    pub library: &'static str,
    pub items: Vec<Item>,
    pub tracks: Vec<TrackFile>,
    pub camera_sets: Vec<CameraSet>,
}
