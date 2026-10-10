//! Categories the owner can add, rename, nest and reorder.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(default)]
pub struct CategoryPatch {
    pub name: Option<String>,
    pub definition: Option<String>,
    /// Empty string clears the template (items stay where they are).
    pub folder_template: Option<String>,
    pub own_folder: Option<bool>,
    pub immich_default: Option<bool>,
    pub auto_threshold: Option<f32>,
    pub sort: Option<i32>,
}

/// A stable key from a name: "Pokémon GO" → "pokemon_go".
pub fn key_for(c: &Connection, name: &str, parent: Option<&str>) -> rusqlite::Result<String> {
    let base: String = name
        .chars()
        .map(|ch| match ch {
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'á' | 'à' | 'â' | 'ä' => 'a',
            'ó' | 'ò' | 'ô' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            c if c.is_ascii_alphanumeric() => c.to_ascii_lowercase(),
            _ => '_',
        })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    let base = match parent {
        Some(p) => format!("{p}.{base}"),
        None => base,
    };
    let mut key = base.clone();
    let mut n = 2;
    while c.query_row("SELECT 1 FROM categories WHERE key = ?1", [&key], |_| Ok(())).optional()?.is_some() {
        key = format!("{base}_{n}");
        n += 1;
    }
    Ok(key)
}

/// Add a category or, with a parent, a subcategory. Subcategories are tags
/// only unless they get their own folder.
pub fn add(c: &Connection, name: &str, parent: Option<&str>, definition: &str) -> anyhow::Result<String> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("A category needs a name.");
    }
    let parent_id: Option<i64> = match parent {
        Some(p) => Some(
            c.query_row("SELECT id FROM categories WHERE key = ?1", [p], |r| r.get(0))
                .optional()?
                .ok_or_else(|| anyhow::anyhow!("Unknown parent category"))?,
        ),
        None => None,
    };
    let dup: Option<i64> = c
        .query_row("SELECT id FROM categories WHERE name = ?1 COLLATE NOCASE AND parent_id IS ?2", params![name, parent_id], |r| r.get(0))
        .optional()?;
    if dup.is_some() {
        anyhow::bail!("A category named {name} already exists here.");
    }
    let key = key_for(c, name, parent)?;
    let sort: i64 = c.query_row("SELECT coalesce(max(sort), 0) + 1 FROM categories WHERE parent_id IS ?1", [parent_id], |r| r.get(0))?;
    let template = if parent.is_some() { None } else { Some(format!("{{year}}/{}", crate::template::safe_segment(name))) };
    c.execute(
        "INSERT INTO categories (key, name, parent_id, definition, folder_template, own_folder, immich_default, sort) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)",
        params![key, name, parent_id, definition.trim(), template, parent.is_none(), sort],
    )?;
    Ok(key)
}

pub fn update(c: &Connection, key: &str, p: &CategoryPatch) -> anyhow::Result<()> {
    let id: i64 = c.query_row("SELECT id FROM categories WHERE key = ?1", [key], |r| r.get(0))?;
    if let Some(name) = p.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        c.execute("UPDATE categories SET name = ?2 WHERE id = ?1", params![id, name])?;
    }
    if let Some(d) = &p.definition {
        c.execute("UPDATE categories SET definition = ?2 WHERE id = ?1", params![id, d.trim()])?;
    }
    if let Some(t) = &p.folder_template {
        if key == "unsorted" && !t.trim().is_empty() {
            anyhow::bail!("Unsorted items always stay where they are.");
        }
        let t = t.trim();
        c.execute("UPDATE categories SET folder_template = ?2 WHERE id = ?1", params![id, (!t.is_empty()).then_some(t)])?;
    }
    if let Some(v) = p.own_folder {
        c.execute("UPDATE categories SET own_folder = ?2 WHERE id = ?1", params![id, v])?;
    }
    if let Some(v) = p.immich_default {
        c.execute("UPDATE categories SET immich_default = ?2 WHERE id = ?1", params![id, v])?;
    }
    if let Some(v) = p.auto_threshold {
        c.execute("UPDATE categories SET auto_threshold = ?2 WHERE id = ?1", params![id, v.clamp(0.5, 1.0)])?;
    }
    if let Some(v) = p.sort {
        c.execute("UPDATE categories SET sort = ?2 WHERE id = ?1", params![id, v])?;
    }
    // Renamed categories change the mirrored tags of decided items.
    if p.name.is_some() {
        refresh_tags(c, key)?;
    }
    Ok(())
}

/// Rewrite `Category/…` tags after a rename.
fn refresh_tags(c: &Connection, key: &str) -> rusqlite::Result<()> {
    let ids: Vec<i64> = {
        let mut st = c.prepare(
            "SELECT a.id FROM assets a JOIN categories cat ON cat.id = a.category_id
             WHERE a.review >= 2 AND (cat.key = ?1 OR cat.parent_id = (SELECT id FROM categories WHERE key = ?1))",
        )?;
        st.query_map([key], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    for id in ids {
        let v = crate::edits::read(c, id, "category")?;
        crate::edits::write(c, id, "category", &v)?;
    }
    Ok(())
}

/// Delete a category the owner added. Its items become unsorted.
pub fn delete(c: &Connection, key: &str) -> anyhow::Result<()> {
    let (id, builtin): (i64, bool) =
        c.query_row("SELECT id, builtin FROM categories WHERE key = ?1", [key], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)))?;
    if builtin {
        anyhow::bail!("Built-in categories can be renamed but not deleted.");
    }
    let ids: Vec<i64> = {
        let mut st =
            c.prepare("SELECT id FROM assets WHERE category_id = ?1 OR category_id IN (SELECT id FROM categories WHERE parent_id = ?1)")?;
        st.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    for a in ids {
        crate::edits::write(c, a, "category", &crate::edits::category_value(None, "manual", None, 0))?;
    }
    c.execute("DELETE FROM categories WHERE id = ?1", [id])?;
    crate::tags::prune(c)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn add_nest_rename_delete() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let k = add(&c, "Pokémon GO", Some("screenshots"), "Screenshots of the game").unwrap();
        assert_eq!(k, "screenshots.pokemon_go");
        assert!(add(&c, "pokémon go", Some("screenshots"), "").is_err());
        let top = add(&c, "Recipes", None, "").unwrap();
        let t: String = c.query_row("SELECT folder_template FROM categories WHERE key = ?1", [&top], |r| r.get(0)).unwrap();
        assert_eq!(t, "{year}/Recipes");
        update(&c, &k, &CategoryPatch { name: Some("Pokemon".into()), own_folder: Some(true), ..Default::default() }).unwrap();
        assert!(delete(&c, "photos").is_err());
        delete(&c, &top).unwrap();
    }
}
