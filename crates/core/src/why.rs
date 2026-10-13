//! Plain-language answers to "why does this field have this value".

use rusqlite::{Connection, params};

use crate::model::Why;

pub fn date_source(source: &str) -> &'static str {
    match source {
        "exif_original" => "the camera's date taken",
        "exif_create" => "the camera's creation date",
        "quicktime" => "the video's creation date",
        "xmp" => "the date in the file's XMP data",
        "sidecar" => "the XMP sidecar",
        "takeout" => "the Google Takeout sidecar",
        "filename" => "the file name",
        "folder" => "the folder name",
        "file_time" => "the file's modified time, as a last resort",
        "neighbors" => "nearby photos in the same sequence",
        "manual" => "a manual change",
        _ => "the file",
    }
}

pub fn location_source(source: &str) -> &'static str {
    match source {
        "file" => "the file's GPS data",
        "sidecar" => "the XMP sidecar",
        "takeout" => "the Google Takeout sidecar",
        "neighbors" => "photos taken around the same time",
        "history" | "track" => "your location history",
        "event" => "other photos of the same event",
        "name" => "the folder or file name",
        "manual" => "a manual change",
        _ => "the file",
    }
}

/// The base explanations for an asset, newest edits first.
pub fn explain(
    c: &Connection,
    asset: i64,
    date_src: Option<&str>,
    loc_src: Option<&str>,
    cat_src: Option<&str>,
    cat_name: Option<&str>,
) -> rusqlite::Result<Vec<Why>> {
    let mut out = Vec::new();
    let mut st = c.prepare_cached(
        "SELECT field, source, why, conf, at FROM field_history WHERE asset_id = ?1 AND id IN
           (SELECT max(id) FROM field_history WHERE asset_id = ?1 GROUP BY field) ORDER BY at DESC",
    )?;
    let rows = st.query_map(params![asset], |r| {
        Ok(Why {
            field: r.get(0)?,
            source: r.get(1)?,
            text: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            conf: r.get(3)?,
            at: r.get(4)?,
        })
    })?;
    for row in rows {
        let w = row?;
        if !w.text.is_empty() {
            out.push(w);
        }
    }
    let has = |f: &str, out: &[Why]| out.iter().any(|w| w.field == f);
    if let (Some(s), false) = (date_src, has("date", &out)) {
        out.push(Why { field: "date".into(), source: s.into(), text: format!("Date from {}.", date_source(s)), conf: None, at: 0 });
    }
    if let (Some(s), false) = (loc_src, has("location", &out)) {
        out.push(Why {
            field: "location".into(),
            source: s.into(),
            text: format!("Place from {}.", location_source(s)),
            conf: None,
            at: 0,
        });
    }
    if let (Some(s), Some(name), false) = (cat_src, cat_name, has("category", &out)) {
        let text = match s {
            "file" => format!("{name}, from the tags in the file."),
            "manual" => format!("{name}, chosen by you."),
            _ => format!("{name}."),
        };
        out.push(Why { field: "category".into(), source: s.into(), text, conf: None, at: 0 });
    }
    Ok(out)
}
