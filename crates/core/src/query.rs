//! Filters to SQL, and the packed timeline payload.

use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use crate::model::{Query, problem, tile};

pub struct Sql {
    pub clause: String,
    pub params: Vec<Value>,
}

fn list_param(params: &mut Vec<Value>, items: &[String]) -> String {
    let start = params.len();
    params.extend(items.iter().map(|s| Value::Text(s.clone())));
    (start + 1..=params.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ")
}

fn push(params: &mut Vec<Value>, v: Value) -> String {
    params.push(v);
    format!("?{}", params.len())
}

/// Whether an asset goes to Immich, after overrides and defaults.
pub const IMMICH_SELECTED: &str = "(CASE a.immich WHEN 'send' THEN 1 WHEN 'local' THEN 0 ELSE
    CASE WHEN a.private = 1 THEN 0
         WHEN EXISTS (SELECT 1 FROM event_assets ea JOIN events e ON e.id = ea.event_id
                      WHERE ea.asset_id = a.id AND e.state = 'confirmed' AND e.immich != 'local') THEN 1
         ELSE coalesce((SELECT c.immich_default FROM categories c WHERE c.id = a.category_id), 0) END END)";

/// The WHERE clause for a query over `assets a`.
pub fn where_clause(q: &Query) -> Sql {
    let mut w: Vec<String> = Vec::new();
    let mut p: Vec<Value> = Vec::new();
    w.push(if q.missing { "a.missing = 1".into() } else { "a.missing = 0".into() });
    w.push(if q.trashed { "a.trashed_at IS NOT NULL".into() } else { "a.trashed_at IS NULL".into() });
    if q.ids.is_none() {
        w.push("(a.group_role IS NULL OR a.group_role = 'primary')".into());
    }
    if let Some(ids) = &q.ids {
        let ids: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
        w.push(format!("a.id IN ({})", if ids.is_empty() { "NULL".into() } else { ids.join(",") }));
    }
    if let Some(f) = q.from_utc {
        w.push(format!("a.taken_utc >= {}", push(&mut p, Value::Integer(f))));
    }
    if let Some(t) = q.to_utc {
        w.push(format!("a.taken_utc < {}", push(&mut p, Value::Integer(t))));
    }
    if !q.media.is_empty() {
        let parts: Vec<&str> = q
            .media
            .iter()
            .filter_map(|m| match m.as_str() {
                "photo" => Some("a.kind = 0"),
                "video" => Some("a.kind = 1"),
                "live" => Some("a.group_id IN (SELECT id FROM groups WHERE kind = 'live')"),
                "burst" => Some("a.group_id IN (SELECT id FROM groups WHERE kind = 'burst')"),
                "raw" => Some("(a.format = 'raw' OR a.group_id IN (SELECT id FROM groups WHERE kind = 'raw'))"),
                _ => None,
            })
            .collect();
        if !parts.is_empty() {
            w.push(format!("({})", parts.join(" OR ")));
        }
    }
    let category_set = |p: &mut Vec<Value>, keys: &[String]| {
        let list = list_param(p, keys);
        let unsorted = if keys.iter().any(|k| k == "unsorted") { " OR a.category_id IS NULL" } else { "" };
        format!(
            "(coalesce(a.category_id, -1) IN (SELECT id FROM categories WHERE key IN ({list}) OR parent_id IN (SELECT id FROM categories WHERE key IN ({list}))){unsorted})"
        )
    };
    if !q.categories.is_empty() {
        let c = category_set(&mut p, &q.categories);
        w.push(c);
    }
    if !q.exclude_categories.is_empty() {
        let c = category_set(&mut p, &q.exclude_categories);
        w.push(format!("NOT {c}"));
    }
    for t in &q.tags {
        let a = push(&mut p, Value::Text(t.clone()));
        let b = push(&mut p, Value::Text(format!("{t}/%")));
        w.push(format!("EXISTS (SELECT 1 FROM asset_tags at JOIN tags t ON t.id = at.tag_id WHERE at.asset_id = a.id AND (t.path = {a} OR t.path LIKE {b}))"));
    }
    if let Some(e) = q.event_id {
        w.push(format!("a.id IN (SELECT asset_id FROM event_assets WHERE event_id = {})", push(&mut p, Value::Integer(e as i64))));
    }
    if !q.cameras.is_empty() {
        w.push(format!("coalesce(a.model, a.make) IN ({})", list_param(&mut p, &q.cameras)));
    }
    if !q.lenses.is_empty() {
        w.push(format!("a.lens IN ({})", list_param(&mut p, &q.lenses)));
    }
    if !q.formats.is_empty() {
        w.push(format!("a.format IN ({})", list_param(&mut p, &q.formats)));
    }
    match q.location.as_deref() {
        Some("any") => w.push("a.lat IS NOT NULL".into()),
        Some("none") => w.push("a.lat IS NULL".into()),
        Some("approximate") => w.push("a.lat IS NOT NULL AND a.loc_acc > 100".into()),
        Some("exact") => w.push("a.lat IS NOT NULL AND (a.loc_acc IS NULL OR a.loc_acc <= 100)".into()),
        _ => {}
    }
    if let Some(b) = q.bbox.as_ref().filter(|b| b.len() == 4) {
        let (s, west, n, e) = (b[0], b[1], b[2], b[3]);
        let lat = format!("a.lat BETWEEN {} AND {}", push(&mut p, Value::Real(s)), push(&mut p, Value::Real(n)));
        let lon = if west <= e {
            format!("a.lon BETWEEN {} AND {}", push(&mut p, Value::Real(west)), push(&mut p, Value::Real(e)))
        } else {
            format!("(a.lon >= {} OR a.lon <= {})", push(&mut p, Value::Real(west)), push(&mut p, Value::Real(e)))
        };
        w.push(format!("{lat} AND {lon}"));
    }
    if let Some(r) = q.root_id {
        w.push(format!("a.root_id = {}", push(&mut p, Value::Integer(r as i64))));
    }
    if let Some(f) = &q.folder {
        if q.recursive {
            if !f.is_empty() {
                let a = push(&mut p, Value::Text(f.clone()));
                let b = push(&mut p, Value::Text(format!("{f}/%")));
                w.push(format!("(a.dir = {a} OR a.dir LIKE {b})"));
            }
        } else {
            w.push(format!("a.dir = {}", push(&mut p, Value::Text(f.clone()))));
        }
    }
    if let Some(r) = q.min_rating {
        w.push(format!("a.rating >= {}", push(&mut p, Value::Integer(r as i64))));
    }
    if let Some(f) = q.favorite {
        w.push(format!("a.favorite = {}", i32::from(f)));
    }
    if let Some(bits) = q.problems {
        w.push(format!("(a.problems & {}) != 0", push(&mut p, Value::Integer(bits as i64))));
    }
    if !q.review.is_empty() {
        let list: Vec<String> = q.review.iter().map(|r| r.to_string()).collect();
        w.push(format!("a.review IN ({})", list.join(",")));
    }
    match q.immich.as_deref() {
        Some("sent") => w.push("a.immich_id IS NOT NULL".into()),
        Some("local") => w.push(format!("{IMMICH_SELECTED} = 0")),
        Some("pending") => w.push(format!("{IMMICH_SELECTED} = 1 AND a.immich_id IS NULL")),
        Some("selected") => w.push(format!("{IMMICH_SELECTED} = 1")),
        _ => {}
    }
    match q.orientation.as_deref() {
        Some("portrait") => w.push("a.height > a.width".into()),
        Some("landscape") => w.push("a.width > a.height".into()),
        Some("square") => w.push("a.width = a.height".into()),
        _ => {}
    }
    if let Some(v) = q.private {
        w.push(format!("a.private = {}", i32::from(v)));
    }
    if let Some(v) = q.inbox {
        w.push(if v { "a.inbox > 0".into() } else { "a.inbox = 0".into() });
    }
    if let Some(t) = q.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let like = push(&mut p, Value::Text(format!("%{}%", t.replace('%', ""))));
        w.push(format!(
            "(a.name LIKE {like} OR a.dir LIKE {like} OR a.description LIKE {like} OR coalesce(a.model, '') LIKE {like}
              OR EXISTS (SELECT 1 FROM asset_tags at JOIN tags t ON t.id = at.tag_id WHERE at.asset_id = a.id AND t.path LIKE {like}))"
        ));
    }
    Sql { clause: w.join(" AND "), params: p }
}

/// Header: magic, version, count. Then arrays, each 4-byte aligned:
/// ids u32, days u32 (yyyymmdd), aspect f32, color u32, flags u16,
/// thumbnail version u16.
pub const MAGIC: u32 = 0x4C4C_4F52; // "ROLL"

pub fn timeline(c: &Connection, q: &Query) -> rusqlite::Result<Vec<u8>> {
    let sql = where_clause(q);
    let stmt = format!(
        "SELECT a.id, a.taken_local, a.width, a.height, a.kind, g.kind, a.favorite, a.problems, a.avg_color, a.private, a.thumb, a.format, a.thumb_v
         FROM assets a LEFT JOIN groups g ON g.id = a.group_id WHERE {} ORDER BY a.taken_local DESC, a.id DESC",
        sql.clause
    );
    let mut st = c.prepare(&stmt)?;
    let mut ids = Vec::new();
    let mut days = Vec::new();
    let mut aspects = Vec::new();
    let mut colors = Vec::new();
    let mut flags = Vec::new();
    let mut versions = Vec::new();
    let mut rows = st.query(params_from_iter(sql.params.iter()))?;
    while let Some(r) = rows.next()? {
        ids.push(r.get::<_, i64>(0)? as u32);
        let local: Option<String> = r.get(1)?;
        let day = local.as_deref().and_then(|l| l.get(0..10)).map(|d| d.replace('-', "")).and_then(|d| d.parse::<u32>().ok()).unwrap_or(0);
        days.push(day);
        let (w, h): (Option<f64>, Option<f64>) = (r.get(2)?, r.get(3)?);
        aspects.push(match (w, h) {
            (Some(w), Some(h)) if w > 0.0 && h > 0.0 => (w / h).clamp(0.2, 5.0) as f32,
            _ => 1.0,
        });
        colors.push(r.get::<_, Option<i64>>(8)?.unwrap_or(0) as u32);
        let mut f = 0u16;
        if r.get::<_, i64>(4)? == 1 {
            f |= tile::VIDEO;
        }
        match r.get::<_, Option<String>>(5)?.as_deref() {
            Some("live") => f |= tile::LIVE,
            Some("burst") => f |= tile::BURST,
            Some("raw") => f |= tile::RAW,
            Some("edit") => f |= tile::EDITED,
            _ => {}
        }
        if r.get::<_, String>(11)? == "raw" {
            f |= tile::RAW;
        }
        if r.get::<_, i64>(6)? != 0 {
            f |= tile::FAVORITE;
        }
        if r.get::<_, i64>(7)? & (problem::NO_DATE | problem::SUSPICIOUS_DATE | problem::BROKEN) != 0 {
            f |= tile::PROBLEM;
        }
        if r.get::<_, i64>(9)? != 0 {
            f |= tile::PRIVATE;
        }
        if r.get::<_, i64>(10)? != 1 {
            f |= tile::NO_THUMB;
        }
        flags.push(f);
        versions.push(r.get::<_, i64>(12)?.clamp(0, u16::MAX as i64) as u16);
    }
    let n = ids.len();
    let mut out = Vec::with_capacity(12 + n * 20);
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    for v in &ids {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in &days {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in &aspects {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in &colors {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in flags.iter().chain(&versions) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    Ok(out)
}

pub fn count(c: &Connection, q: &Query) -> rusqlite::Result<i64> {
    let sql = where_clause(q);
    c.query_row(&format!("SELECT count(*) FROM assets a WHERE {}", sql.clause), params_from_iter(sql.params.iter()), |r| r.get(0))
}

pub fn ids(c: &Connection, q: &Query) -> rusqlite::Result<Vec<i64>> {
    let sql = where_clause(q);
    let mut st = c.prepare(&format!("SELECT a.id FROM assets a WHERE {} ORDER BY a.taken_local DESC, a.id DESC", sql.clause))?;
    let rows = st.query_map(params_from_iter(sql.params.iter()), |r| r.get(0))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    #[test]
    fn filters_and_payload() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let a = insert_asset(&c, "2019/a.jpg");
        let b = insert_asset(&c, "2019/sub/b.mov");
        let x = insert_asset(&c, "2020/c.jpg");
        c.execute("UPDATE assets SET kind = 1, taken_local = '2019-08-12T10:00:00', width = 1920, height = 1080 WHERE id = ?1", [b])
            .unwrap();
        c.execute("UPDATE assets SET taken_local = '2019-08-13T10:00:00', lat = 45.0, lon = 10.0, loc_acc = 5000 WHERE id = ?1", [a])
            .unwrap();
        c.execute("UPDATE assets SET category_id = (SELECT id FROM categories WHERE key = 'memes') WHERE id = ?1", [x]).unwrap();
        crate::tags::add(&c, a, "Event/2019-08 Lake Garda", "manual", None).unwrap();

        let q = |q: Query| ids(&c, &q).unwrap();
        assert_eq!(q(Query { media: vec!["video".into()], ..Default::default() }), vec![b]);
        assert_eq!(q(Query { folder: Some("2019".into()), recursive: true, ..Default::default() }).len(), 2);
        assert_eq!(q(Query { folder: Some("2019".into()), ..Default::default() }), vec![a]);
        assert_eq!(q(Query { location: Some("approximate".into()), ..Default::default() }), vec![a]);
        assert_eq!(q(Query { tags: vec!["Event".into()], ..Default::default() }), vec![a]);
        assert_eq!(q(Query { categories: vec!["memes".into()], ..Default::default() }), vec![x]);
        assert_eq!(q(Query { exclude_categories: vec!["memes".into()], ..Default::default() }).len(), 2);
        assert_eq!(q(Query { bbox: Some(vec![44.0, 9.0, 46.0, 11.0]), ..Default::default() }), vec![a]);
        assert_eq!(q(Query { text: Some("sub".into()), ..Default::default() }), vec![b]);

        let bytes = timeline(&c, &Query::default()).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), MAGIC);
        let n = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        assert_eq!(n, 3);
        assert_eq!(bytes.len(), 12 + n * 20);
        // Newest first: a (Aug 13), then b (Aug 12).
        assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as i64, a);
        let day_b = u32::from_le_bytes(bytes[12 + n * 4 + 4..12 + n * 4 + 8].try_into().unwrap());
        assert_eq!(day_b, 20190812);
    }
}
