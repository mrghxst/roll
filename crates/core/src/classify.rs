//! Stage one of sorting: categories from tags already in the files, then
//! rules. Owner decisions (review 3) are never changed here.

use rusqlite::{Connection, params};

use crate::edits::{self, Change};
use crate::rules::{self, Facts};
use crate::tags;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Outcome {
    pub decided: usize,
    pub proposed: usize,
    pub tagged: usize,
}

struct Row {
    id: i64,
    facts: Facts,
    review: i64,
    current: Option<String>,
}

fn load(c: &Connection, ids: Option<&[i64]>) -> rusqlite::Result<Vec<Row>> {
    let filter = match ids {
        Some([]) => return Ok(Vec::new()),
        Some(ids) => format!("AND a.id IN ({})", ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")),
        None => String::new(),
    };
    let mut st = c.prepare(&format!(
        "SELECT a.id, a.name, a.rel_path, a.ext, a.format, a.make, a.model, a.software, a.kind, a.screenshot, a.width, a.height,
                a.duration_ms, a.size, a.problems, a.stats, a.review, c.key
         FROM assets a LEFT JOIN categories c ON c.id = a.category_id
         WHERE a.missing = 0 AND a.trashed_at IS NULL AND a.review < 3 {filter}"
    ))?;
    let rows = st.query_map([], |r| {
        let (w, h): (Option<i64>, Option<i64>) = (r.get(10)?, r.get(11)?);
        let stats: Option<String> = r.get(15)?;
        let junk = stats
            .as_deref()
            .and_then(|s| serde_json::from_str::<roll_media::stats::ImageStats>(s).ok())
            .and_then(|s| roll_media::stats::junk_reason(&s, w.unwrap_or(0) as u32, h.unwrap_or(0) as u32))
            .map(String::from)
            .or_else(|| (w.zip(h).is_some_and(|(w, h)| w.max(h) < 200)).then(|| "smaller than 200 px".to_string()));
        Ok(Row {
            id: r.get(0)?,
            facts: Facts {
                name: r.get(1)?,
                path: r.get(2)?,
                ext: r.get(3)?,
                format: r.get(4)?,
                make: r.get(5)?,
                model: r.get(6)?,
                software: r.get(7)?,
                video: r.get::<_, i64>(8)? == 1,
                screenshot: r.get::<_, i64>(9)? != 0,
                width: w,
                height: h,
                duration_ms: r.get(12)?,
                size: r.get(13)?,
                broken: r.get::<_, i64>(14)? & crate::model::problem::BROKEN != 0,
                junk,
            },
            review: r.get(16)?,
            current: r.get(17)?,
        })
    })?;
    rows.collect()
}

/// The category named by a `Category/…` tag in the file, if any.
fn category_from_tags(c: &Connection, asset: i64) -> rusqlite::Result<Option<String>> {
    let file_tags: Vec<String> = {
        let mut st = c.prepare_cached(
            "SELECT t.path FROM asset_tags at JOIN tags t ON t.id = at.tag_id WHERE at.asset_id = ?1 AND at.source IN ('file', 'sidecar') AND t.path LIKE 'Category/%'",
        )?;
        st.query_map([asset], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    for t in file_tags {
        let Some(path) = t.strip_prefix(tags::CATEGORY_PREFIX) else { continue };
        // The deepest category whose tag matches.
        let mut st = c.prepare_cached("SELECT key FROM categories")?;
        let keys: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let mut best: Option<(usize, String)> = None;
        for k in keys {
            if let Some(tag) = edits::category_tag(c, &k)? {
                let tp = tag.trim_start_matches(tags::CATEGORY_PREFIX);
                if (tp.eq_ignore_ascii_case(path) || path.to_lowercase().starts_with(&format!("{}/", tp.to_lowercase())))
                    && best.as_ref().is_none_or(|(d, _)| tp.len() > *d)
                {
                    best = Some((tp.len(), k));
                }
            }
        }
        if let Some((_, k)) = best {
            return Ok(Some(k));
        }
    }
    Ok(None)
}

/// Classify assets (all undecided ones when `ids` is None).
pub fn run(c: &Connection, ids: Option<&[i64]>) -> rusqlite::Result<Outcome> {
    let rules = rules::load(c)?;
    let names: std::collections::HashMap<String, String> = {
        let mut st = c.prepare("SELECT key, name FROM categories")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    let mut out = Outcome::default();
    for row in load(c, ids)? {
        let d = rules::evaluate(&rules, &row.facts);
        for (tag, key, because) in &d.tags {
            if tags::add(c, row.id, tag, &format!("rule:{key}"), None)? {
                c.execute(
                    "INSERT INTO field_history (asset_id, field, old, new, source, source_ref, conf, why, at) VALUES (?1, 'tags', NULL, ?2, 'rule', ?3, 1.0, ?4, ?5)",
                    params![row.id, serde_json::json!([tag]).to_string(), key, format!("Tagged {tag}, because of {because}."), crate::db::now_ms()],
                )?;
                out.tagged += 1;
            }
        }
        let was_dirty: bool = c.query_row("SELECT meta_dirty FROM assets WHERE id = ?1", [row.id], |r| r.get(0))?;
        // The file already says this, and a proposal changes nothing in it.
        let keep_clean = |c: &Connection| -> rusqlite::Result<()> {
            if !was_dirty {
                c.execute("UPDATE assets SET meta_dirty = 0 WHERE id = ?1", [row.id])?;
            }
            Ok(())
        };
        if let Some(key) = category_from_tags(c, row.id)? {
            let name = names.get(&key).cloned().unwrap_or_default();
            edits::set(
                c,
                None,
                row.id,
                &Change {
                    field: "category",
                    value: edits::category_value(Some(&key), "file", Some(1.0), 3),
                    source: "file",
                    source_ref: None,
                    conf: Some(1.0),
                    why: Some(format!("{name}, from the tags in the file.")),
                },
            )?;
            keep_clean(c)?;
            out.decided += 1;
            continue;
        }
        let Some((cat, conf, certain, key, because)) = d.category else { continue };
        // Keep an earlier, equally good proposal from a model or the owner.
        if row.current.as_deref() == Some(cat.as_str()) && row.review >= if certain { 2 } else { 1 } {
            continue;
        }
        let name = names.get(&cat).cloned().unwrap_or_default();
        let review = if certain { 2 } else { 1 };
        edits::set(
            c,
            None,
            row.id,
            &Change {
                field: "category",
                value: edits::category_value(Some(&cat), &format!("rule:{key}"), Some(conf), review),
                source: "rule",
                source_ref: Some(&key),
                conf: Some(conf),
                why: Some(format!("{name}, because of {because}.")),
            },
        )?;
        if certain {
            out.decided += 1;
        } else {
            keep_clean(c)?;
            out.proposed += 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    #[test]
    fn rules_and_file_tags() {
        let db = Db::memory().unwrap();
        let c = db.write();
        rules::seed(&c).unwrap();
        let shot = insert_asset(&c, "Screenshot_20200301-101500.png");
        let wa = insert_asset(&c, "IMG-20200102-WA0004.jpg");
        let phone = insert_asset(&c, "IMG_1.HEIC");
        c.execute("UPDATE assets SET make = 'Apple', width = 4032, height = 3024 WHERE id = ?1", [phone]).unwrap();
        let tagged = insert_asset(&c, "meme.jpg");
        tags::add(&c, tagged, "Category/Memes", "file", None).unwrap();
        let owner = insert_asset(&c, "Screenshot_1.png");
        c.execute("UPDATE assets SET review = 3, category_id = (SELECT id FROM categories WHERE key = 'documents') WHERE id = ?1", [owner])
            .unwrap();

        let o = run(&c, None).unwrap();
        assert_eq!((o.decided, o.proposed, o.tagged), (2, 1, 1));
        let cat = |id: i64| -> (Option<String>, i64) {
            c.query_row("SELECT c.key, a.review FROM assets a LEFT JOIN categories c ON c.id = a.category_id WHERE a.id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap()
        };
        assert_eq!(cat(shot), (Some("screenshots".into()), 2));
        assert_eq!(cat(phone), (Some("photos".into()), 1));
        assert_eq!(cat(tagged), (Some("memes".into()), 3));
        assert_eq!(cat(owner), (Some("documents".into()), 3));
        assert_eq!(cat(wa), (None, 0));
        assert_eq!(tags::of_asset(&c, wa).unwrap(), vec!["Source/WhatsApp"]);
        assert_eq!(tags::of_asset(&c, shot).unwrap(), vec!["Category/Screenshots"]);
        let why: String =
            c.query_row("SELECT why FROM field_history WHERE asset_id = ?1 AND field = 'category'", [shot], |r| r.get(0)).unwrap();
        assert_eq!(why, "Screenshots, because of a screenshot file name.");
        let dirty = |id: i64| -> i64 { c.query_row("SELECT meta_dirty FROM assets WHERE id = ?1", [id], |r| r.get(0)).unwrap() };
        assert_eq!((dirty(shot), dirty(phone), dirty(tagged)), (1, 0, 0));
        // Running again changes nothing.
        assert_eq!(run(&c, None).unwrap(), Outcome::default());
    }
}
