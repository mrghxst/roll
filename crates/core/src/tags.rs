//! Hierarchical tags. Some tags mirror fields: `Category/<name>`,
//! `Event/<name>`, `Source/<app>`, `Favorite` and `Private`.

use rusqlite::{params, Connection, OptionalExtension};

use roll_meta::tags::normalize;

pub const FAVORITE: &str = "Favorite";
pub const PRIVATE: &str = "Private";
pub const CATEGORY_PREFIX: &str = "Category/";
pub const EVENT_PREFIX: &str = "Event/";
pub const SOURCE_PREFIX: &str = "Source/";

pub fn tag_id(c: &Connection, path: &str) -> rusqlite::Result<Option<i64>> {
    let Some(path) = normalize(path) else { return Ok(None) };
    if let Some(id) = c.query_row("SELECT id FROM tags WHERE path = ?1", [&path], |r| r.get(0)).optional()? {
        return Ok(Some(id));
    }
    c.execute("INSERT INTO tags (path) VALUES (?1)", [&path])?;
    Ok(Some(c.last_insert_rowid()))
}

pub fn add(c: &Connection, asset: i64, path: &str, source: &str, conf: Option<f32>) -> rusqlite::Result<bool> {
    let Some(id) = tag_id(c, path)? else { return Ok(false) };
    let n = c.execute(
        "INSERT INTO asset_tags (asset_id, tag_id, source, conf) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(asset_id, tag_id) DO NOTHING",
        params![asset, id, source, conf],
    )?;
    Ok(n > 0)
}

pub fn remove(c: &Connection, asset: i64, path: &str) -> rusqlite::Result<bool> {
    let Some(path) = normalize(path) else { return Ok(false) };
    let n = c.execute(
        "DELETE FROM asset_tags WHERE asset_id = ?1 AND tag_id = (SELECT id FROM tags WHERE path = ?2)",
        params![asset, path],
    )?;
    Ok(n > 0)
}

/// Remove every tag under `prefix` (e.g. all `Category/…` tags).
pub fn remove_prefix(c: &Connection, asset: i64, prefix: &str) -> rusqlite::Result<usize> {
    c.execute(
        "DELETE FROM asset_tags WHERE asset_id = ?1 AND tag_id IN (SELECT id FROM tags WHERE path LIKE ?2 ESCAPE '\\')",
        params![asset, format!("{}%", prefix.replace('%', "\\%").replace('_', "\\_"))],
    )
}

pub fn of_asset(c: &Connection, asset: i64) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare_cached("SELECT t.path FROM asset_tags a JOIN tags t ON t.id = a.tag_id WHERE a.asset_id = ?1 ORDER BY t.path")?;
    let rows = st.query_map([asset], |r| r.get(0))?;
    rows.collect()
}

/// Remove tags that no asset uses any more.
pub fn prune(c: &Connection) -> rusqlite::Result<usize> {
    c.execute("DELETE FROM tags WHERE id NOT IN (SELECT DISTINCT tag_id FROM asset_tags)", [])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    #[test]
    fn add_remove_prefix() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let a = insert_asset(&c, "a.jpg");
        assert!(add(&c, a, "Category/Memes", "rule", Some(0.9)).unwrap());
        assert!(!add(&c, a, "category / memes", "rule", None).unwrap());
        add(&c, a, "Source/WhatsApp", "rule", None).unwrap();
        add(&c, a, "Favorite", "manual", None).unwrap();
        assert_eq!(of_asset(&c, a).unwrap(), vec!["Category/Memes", "Favorite", "Source/WhatsApp"]);
        assert_eq!(remove_prefix(&c, a, CATEGORY_PREFIX).unwrap(), 1);
        assert!(remove(&c, a, "Favorite").unwrap());
        assert_eq!(of_asset(&c, a).unwrap(), vec!["Source/WhatsApp"]);
        assert_eq!(prune(&c).unwrap(), 2);
    }
}
