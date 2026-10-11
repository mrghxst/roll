//! Field edits with provenance. Every change records the old and new value,
//! its source, confidence and a plain-language reason in `field_history`.
//! Edits made together form a batch that can be undone. Edits change the
//! database only and mark the asset `meta_dirty`; files are written when a
//! plan is applied.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::db::now_ms;
use crate::model::problem;
use crate::tags;

pub const FIELDS: &[&str] =
    &["category", "tags", "date", "location", "rating", "favorite", "description", "private", "immich", "trashed", "orientation"];

/// Fields that end up in file metadata.
fn file_field(field: &str) -> bool {
    matches!(field, "category" | "tags" | "date" | "location" | "rating" | "favorite" | "description" | "private" | "orientation")
}

/// Formats whose EXIF orientation Roll both honors when decoding and can
/// write. HEIF rotation lives in the container, video rotation in the track.
pub fn rotatable(format: &str) -> bool {
    matches!(format, "jpeg" | "png" | "webp" | "tiff")
}

/// EXIF orientation as (clockwise quarter turns, mirrored).
fn orientation_parts(o: i64) -> (i64, bool) {
    match o {
        2 => (0, true),
        3 => (2, false),
        4 => (2, true),
        5 => (3, true),
        6 => (1, false),
        7 => (1, true),
        8 => (3, false),
        _ => (0, false),
    }
}

/// The orientation after turning the displayed image `turns` quarter turns
/// clockwise (negative turns go counterclockwise).
pub fn rotate_orientation(o: i64, turns: i64) -> i64 {
    let (q, m) = orientation_parts(o);
    let q = (q + turns).rem_euclid(4);
    match (q, m) {
        (0, false) => 1,
        (1, false) => 6,
        (2, false) => 3,
        (3, false) => 8,
        (0, true) => 2,
        (1, true) => 7,
        (2, true) => 4,
        _ => 5,
    }
}

pub fn begin_batch(c: &Connection, kind: &str, label: &str) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO batches (kind, label, state, created_at, applied_at) VALUES (?1, ?2, 'applied', ?3, ?3)",
        params![kind, label, now_ms()],
    )?;
    Ok(c.last_insert_rowid())
}

/// The current value of a field, as stored in history.
pub fn read(c: &Connection, asset: i64, field: &str) -> rusqlite::Result<Value> {
    let v = match field {
        "category" => c
            .query_row(
                "SELECT c.key, a.category_source, a.category_conf, a.review FROM assets a LEFT JOIN categories c ON c.id = a.category_id WHERE a.id = ?1",
                [asset],
                |r| {
                    Ok(json!({
                        "key": r.get::<_, Option<String>>(0)?,
                        "source": r.get::<_, Option<String>>(1)?,
                        "conf": r.get::<_, Option<f64>>(2)?,
                        "review": r.get::<_, i64>(3)?,
                    }))
                },
            )
            .optional()?
            .unwrap_or(Value::Null),
        "tags" => {
            let mut st = c.prepare_cached(
                "SELECT t.path, at.source FROM asset_tags at JOIN tags t ON t.id = at.tag_id WHERE at.asset_id = ?1 AND t.path NOT LIKE 'Category/%' ORDER BY t.path",
            )?;
            let rows: Vec<Value> = st.query_map([asset], |r| Ok(json!({"p": r.get::<_, String>(0)?, "s": r.get::<_, String>(1)?})))?.collect::<Result<_, _>>()?;
            Value::Array(rows)
        }
        "date" => c.query_row("SELECT taken_utc, taken_local, tz_offset, date_source, date_conf FROM assets WHERE id = ?1", [asset], |r| {
            Ok(json!({
                "utc": r.get::<_, Option<i64>>(0)?,
                "local": r.get::<_, Option<String>>(1)?,
                "offset": r.get::<_, Option<i64>>(2)?,
                "source": r.get::<_, Option<String>>(3)?,
                "conf": r.get::<_, Option<f64>>(4)?,
            }))
        })?,
        "location" => c.query_row("SELECT lat, lon, alt, loc_acc, loc_source FROM assets WHERE id = ?1", [asset], |r| {
            let lat: Option<f64> = r.get(0)?;
            Ok(match lat {
                Some(lat) => json!({"lat": lat, "lon": r.get::<_, f64>(1)?, "alt": r.get::<_, Option<f64>>(2)?, "acc": r.get::<_, Option<f64>>(3)?, "source": r.get::<_, Option<String>>(4)?}),
                None => Value::Null,
            })
        })?,
        "rating" => json!(c.query_row("SELECT rating FROM assets WHERE id = ?1", [asset], |r| r.get::<_, i64>(0))?),
        "favorite" => json!(c.query_row("SELECT favorite FROM assets WHERE id = ?1", [asset], |r| r.get::<_, i64>(0))? != 0),
        "description" => json!(c.query_row("SELECT description FROM assets WHERE id = ?1", [asset], |r| r.get::<_, Option<String>>(0))?),
        "private" => json!(c.query_row("SELECT private FROM assets WHERE id = ?1", [asset], |r| r.get::<_, i64>(0))? != 0),
        "immich" => json!(c.query_row("SELECT immich FROM assets WHERE id = ?1", [asset], |r| r.get::<_, String>(0))?),
        "trashed" => json!(c.query_row("SELECT trashed_at IS NOT NULL FROM assets WHERE id = ?1", [asset], |r| r.get::<_, bool>(0))?),
        "orientation" => json!(c.query_row("SELECT coalesce(orientation, 1) FROM assets WHERE id = ?1", [asset], |r| r.get::<_, i64>(0))?),
        _ => Value::Null,
    };
    Ok(v)
}

fn category_name(c: &Connection, key: &str) -> rusqlite::Result<Option<(i64, String)>> {
    c.query_row("SELECT id, name FROM categories WHERE key = ?1", [key], |r| Ok((r.get(0)?, r.get(1)?))).optional()
}

/// The `Category/…` tag path for a category, including its parents.
pub fn category_tag(c: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    let mut parts = Vec::new();
    let mut cur: Option<i64> = c.query_row("SELECT id FROM categories WHERE key = ?1", [key], |r| r.get(0)).optional()?;
    while let Some(id) = cur {
        let (name, parent): (String, Option<i64>) =
            c.query_row("SELECT name, parent_id FROM categories WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        parts.push(name);
        cur = parent;
    }
    if parts.is_empty() {
        return Ok(None);
    }
    parts.reverse();
    Ok(Some(format!("{}{}", tags::CATEGORY_PREFIX, parts.join("/"))))
}

/// Set a field to a value in the history format. Keeps mirrored tags
/// (`Category/…`, `Favorite`, `Private`) and problem flags in step.
pub fn write(c: &Connection, asset: i64, field: &str, v: &Value) -> rusqlite::Result<()> {
    match field {
        "category" => {
            let key = v.get("key").and_then(Value::as_str);
            let review = v.get("review").and_then(Value::as_i64).unwrap_or(0);
            let cat = match key {
                Some(k) => category_name(c, k)?,
                None => None,
            };
            c.execute(
                "UPDATE assets SET category_id = ?2, category_source = ?3, category_conf = ?4, review = ?5 WHERE id = ?1",
                params![
                    asset,
                    cat.as_ref().map(|c| c.0),
                    v.get("source").and_then(Value::as_str),
                    v.get("conf").and_then(Value::as_f64),
                    review
                ],
            )?;
            tags::remove_prefix(c, asset, tags::CATEGORY_PREFIX)?;
            // Only decided categories become tags (proposals stay internal).
            if review >= 2
                && let Some(t) = key.filter(|k| *k != "unsorted").map(|k| category_tag(c, k)).transpose()?.flatten()
            {
                tags::add(c, asset, &t, v.get("source").and_then(Value::as_str).unwrap_or("manual"), None)?;
            }
        }
        "tags" => {
            c.execute(
                "DELETE FROM asset_tags WHERE asset_id = ?1 AND tag_id IN (SELECT id FROM tags WHERE path NOT LIKE 'Category/%')",
                [asset],
            )?;
            for t in v.as_array().into_iter().flatten() {
                let (path, source) = match t {
                    Value::String(p) => (p.as_str(), "manual"),
                    other => {
                        (other.get("p").and_then(Value::as_str).unwrap_or(""), other.get("s").and_then(Value::as_str).unwrap_or("manual"))
                    }
                };
                tags::add(c, asset, path, source, None)?;
            }
            let fav = tags::of_asset(c, asset)?.iter().any(|t| t.eq_ignore_ascii_case(tags::FAVORITE));
            let private = tags::of_asset(c, asset)?.iter().any(|t| t.eq_ignore_ascii_case(tags::PRIVATE));
            c.execute("UPDATE assets SET favorite = ?2, private = ?3 WHERE id = ?1", params![asset, fav, private])?;
        }
        "date" => {
            c.execute(
                "UPDATE assets SET taken_utc = ?2, taken_local = ?3, tz_offset = ?4, date_source = ?5, date_conf = ?6 WHERE id = ?1",
                params![
                    asset,
                    v.get("utc").and_then(Value::as_i64),
                    v.get("local").and_then(Value::as_str),
                    v.get("offset").and_then(Value::as_i64),
                    v.get("source").and_then(Value::as_str),
                    v.get("conf").and_then(Value::as_f64)
                ],
            )?;
            // Problem flags follow from the value, so undo restores them too.
            let source = v.get("source").and_then(Value::as_str);
            let mut flags = 0;
            if v.get("utc").is_none_or(Value::is_null) || matches!(source, None | Some("file_time" | "folder")) {
                flags |= problem::NO_DATE;
            }
            if v.get("offset").is_none_or(Value::is_null) {
                flags |= problem::NO_OFFSET;
            }
            let local =
                v.get("local").and_then(Value::as_str).and_then(|l| chrono::NaiveDateTime::parse_from_str(l, "%Y-%m-%dT%H:%M:%S").ok());
            if local.is_some_and(|l| crate::dates::is_suspicious(l, now_ms())) {
                flags |= problem::SUSPICIOUS_DATE;
            }
            let mask = problem::NO_DATE | problem::NO_OFFSET | problem::SUSPICIOUS_DATE;
            c.execute("UPDATE assets SET problems = (problems & ~?2) | ?3 WHERE id = ?1", params![asset, mask, flags])?;
        }
        "location" => {
            if v.is_null() {
                c.execute(
                    "UPDATE assets SET lat = NULL, lon = NULL, alt = NULL, loc_acc = NULL, loc_source = NULL, problems = problems | ?2 WHERE id = ?1",
                    params![asset, problem::NO_LOCATION],
                )?;
            } else {
                c.execute(
                    "UPDATE assets SET lat = ?2, lon = ?3, alt = ?4, loc_acc = ?5, loc_source = ?6, problems = problems & ~?7 WHERE id = ?1",
                    params![
                        asset,
                        v.get("lat").and_then(Value::as_f64),
                        v.get("lon").and_then(Value::as_f64),
                        v.get("alt").and_then(Value::as_f64),
                        v.get("acc").and_then(Value::as_f64),
                        v.get("source").and_then(Value::as_str),
                        problem::NO_LOCATION
                    ],
                )?;
            }
        }
        "rating" => {
            c.execute("UPDATE assets SET rating = ?2 WHERE id = ?1", params![asset, v.as_i64().unwrap_or(0).clamp(0, 5)])?;
        }
        "favorite" | "private" => {
            let on = v.as_bool().unwrap_or(false);
            let tag = if field == "favorite" { tags::FAVORITE } else { tags::PRIVATE };
            c.execute(&format!("UPDATE assets SET {field} = ?2 WHERE id = ?1"), params![asset, on])?;
            if on {
                tags::add(c, asset, tag, "manual", None)?;
            } else {
                tags::remove(c, asset, tag)?;
            }
        }
        "description" => {
            c.execute("UPDATE assets SET description = ?2 WHERE id = ?1", params![asset, v.as_str().filter(|s| !s.trim().is_empty())])?;
        }
        "immich" => {
            c.execute("UPDATE assets SET immich = ?2 WHERE id = ?1", params![asset, v.as_str().unwrap_or("default")])?;
        }
        "trashed" => {
            let at = v.as_bool().unwrap_or(false).then(now_ms);
            c.execute("UPDATE assets SET trashed_at = ?2 WHERE id = ?1", params![asset, at])?;
        }
        "orientation" => {
            let new = v.as_i64().unwrap_or(1).clamp(1, 8);
            let old: i64 = c.query_row("SELECT coalesce(orientation, 1) FROM assets WHERE id = ?1", [asset], |r| r.get(0))?;
            // A quarter turn swaps the displayed sides; thumbnails are redone.
            let swap = (orientation_parts(old).0 + orientation_parts(new).0) % 2 == 1;
            c.execute(
                "UPDATE assets SET orientation = ?2, thumb = 0,
                   width = CASE WHEN ?3 THEN height ELSE width END, height = CASE WHEN ?3 THEN width ELSE height END
                 WHERE id = ?1",
                params![asset, new, swap],
            )?;
        }
        _ => {}
    }
    if file_field(field) {
        c.execute("UPDATE assets SET meta_dirty = 1 WHERE id = ?1", [asset])?;
    }
    Ok(())
}

pub struct Change<'a> {
    pub field: &'a str,
    pub value: Value,
    pub source: &'a str,
    pub source_ref: Option<&'a str>,
    pub conf: Option<f32>,
    pub why: Option<String>,
}

/// Change one field of one asset, recording history. Returns false when
/// the value was already the same.
pub fn set(c: &Connection, batch: Option<i64>, asset: i64, ch: &Change) -> rusqlite::Result<bool> {
    let old = read(c, asset, ch.field)?;
    if old == ch.value {
        return Ok(false);
    }
    write(c, asset, ch.field, &ch.value)?;
    if ch.field == "date" {
        c.execute("UPDATE assets SET problems = problems | ?2 WHERE id = ?1", params![asset, problem::DATE_CHANGED])?;
    }
    c.execute(
        "INSERT INTO field_history (asset_id, field, old, new, source, source_ref, conf, why, batch_id, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![asset, ch.field, old.to_string(), ch.value.to_string(), ch.source, ch.source_ref, ch.conf, ch.why, batch, now_ms()],
    )?;
    Ok(true)
}

/// Revert every field change of a batch, newest first. Returns the assets.
pub fn undo_batch(c: &Connection, batch: i64) -> rusqlite::Result<Vec<i64>> {
    let rows: Vec<(i64, i64, String, Option<String>)> = {
        let mut st = c.prepare("SELECT id, asset_id, field, old FROM field_history WHERE batch_id = ?1 ORDER BY id DESC")?;
        st.query_map([batch], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?
    };
    let mut assets = Vec::new();
    for (id, asset, field, old) in rows {
        let v: Value = old.as_deref().and_then(|o| serde_json::from_str(o).ok()).unwrap_or(Value::Null);
        write(c, asset, &field, &v)?;
        if field == "date" {
            let changed: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM field_history WHERE asset_id = ?1 AND field = 'date' AND id != ?2 AND (batch_id IS NULL OR batch_id != ?3))",
                params![asset, id, batch],
                |r| r.get(0),
            )?;
            if !changed {
                c.execute("UPDATE assets SET problems = problems & ~?2 WHERE id = ?1", params![asset, problem::DATE_CHANGED])?;
            }
        }
        c.execute("DELETE FROM field_history WHERE id = ?1", [id])?;
        if !assets.contains(&asset) {
            assets.push(asset);
        }
    }
    Ok(assets)
}

/// A category decision in the history format.
pub fn category_value(key: Option<&str>, source: &str, conf: Option<f32>, review: i64) -> Value {
    json!({"key": key, "source": source, "conf": conf, "review": review})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    #[test]
    fn category_mirrors_tag_and_undoes() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let a = insert_asset(&c, "a.jpg");
        let b = begin_batch(&c, "edit", "Set category").unwrap();
        let ch = Change {
            field: "category",
            value: category_value(Some("memes"), "manual", None, 3),
            source: "manual",
            source_ref: None,
            conf: None,
            why: Some("Memes, chosen by you.".into()),
        };
        assert!(set(&c, Some(b), a, &ch).unwrap());
        assert!(!set(&c, Some(b), a, &ch).unwrap());
        assert_eq!(tags::of_asset(&c, a).unwrap(), vec!["Category/Memes"]);
        let dirty: i64 = c.query_row("SELECT meta_dirty FROM assets WHERE id = ?1", [a], |r| r.get(0)).unwrap();
        assert_eq!(dirty, 1);
        undo_batch(&c, b).unwrap();
        assert!(tags::of_asset(&c, a).unwrap().is_empty());
        let cat: Option<i64> = c.query_row("SELECT category_id FROM assets WHERE id = ?1", [a], |r| r.get(0)).unwrap();
        assert!(cat.is_none());
    }

    #[test]
    fn subcategory_tag_has_parent() {
        let db = Db::memory().unwrap();
        let c = db.write();
        c.execute("INSERT INTO categories (key, name, parent_id) VALUES ('pogo', 'Pokémon GO', (SELECT id FROM categories WHERE key = 'screenshots'))", []).unwrap();
        assert_eq!(category_tag(&c, "pogo").unwrap().as_deref(), Some("Category/Screenshots/Pokémon GO"));
    }

    #[test]
    fn favorite_date_location_round_trip() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let a = insert_asset(&c, "a.jpg");
        c.execute("UPDATE assets SET problems = ?2 WHERE id = ?1", params![a, problem::NO_DATE | problem::NO_LOCATION]).unwrap();
        let b = begin_batch(&c, "edit", "Fix").unwrap();
        let mk = |field: &'static str, value: Value| Change { field, value, source: "manual", source_ref: None, conf: None, why: None };
        set(&c, Some(b), a, &mk("favorite", json!(true))).unwrap();
        set(
            &c,
            Some(b),
            a,
            &mk(
                "date",
                json!({"utc": 1_565_611_200_000i64, "local": "2019-08-12T14:00:00", "offset": 120, "source": "manual", "conf": 1.0}),
            ),
        )
        .unwrap();
        set(&c, Some(b), a, &mk("location", json!({"lat": 45.0, "lon": 10.0, "alt": null, "acc": 5000.0, "source": "manual"}))).unwrap();
        let p: i64 = c.query_row("SELECT problems FROM assets WHERE id = ?1", [a], |r| r.get(0)).unwrap();
        assert_eq!(p, problem::DATE_CHANGED);
        assert_eq!(tags::of_asset(&c, a).unwrap(), vec!["Favorite"]);
        undo_batch(&c, b).unwrap();
        let (p, fav, lat): (i64, i64, Option<f64>) = c
            .query_row("SELECT problems, favorite, lat FROM assets WHERE id = ?1", [a], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!(p, problem::NO_DATE | problem::NO_OFFSET | problem::NO_LOCATION);
        assert_eq!((fav, lat), (0, None));
    }
}
