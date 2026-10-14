//! Teach (PROMPT 4.3): a category learned from the owner's examples. The
//! owner names a category or subcategory and sorts 10–30 examples into it;
//! Roll trains a linear classifier on their image embeddings (against the
//! owner's other decisions and a sample of the library), scores the
//! library and proposes matches in Review. Taught proposals are never
//! accepted on their own, and every correction in Review retrains it.

use std::collections::HashSet;

use roll_ai::classifier;
use roll_ai::vectors::{self, Index};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::edits::{self, Change};

/// Fewer examples than this teach nothing yet.
pub const MIN_EXAMPLES: usize = 5;
/// Proposals start here; items already sorted elsewhere by rules or the
/// image model need more.
const PROPOSE: f32 = 0.6;
const PROPOSE_OVER_SORTED: f32 = 0.85;
const SAMPLE: usize = 600;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Taught {
    pub examples: u32,
    pub proposed: u32,
}

/// Train and score `key`. `None` when it has too few examples.
pub fn run(c: &Connection, key: &str, index: &Index) -> rusqlite::Result<Option<Taught>> {
    let (cat_id, parent, name): (i64, Option<i64>, String) =
        c.query_row("SELECT id, parent_id, name FROM categories WHERE key = ?1", [key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let decided: Vec<(i64, Option<i64>, i64)> = {
        let mut st = c.prepare(
            "SELECT a.id, a.category_id, a.review FROM assets a WHERE a.missing = 0 AND a.trashed_at IS NULL AND (a.group_role IS NULL OR a.group_role = 'primary')",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?
    };
    let examples: Vec<&[f32]> = decided
        .iter()
        .filter(|(id, cat, review)| *review == 3 && *cat == Some(cat_id) && index.contains(*id))
        .filter_map(|(id, ..)| index.get(*id))
        .collect();
    if examples.len() < MIN_EXAMPLES {
        return Ok(None);
    }
    // Negatives: the owner's other decisions count fully; for a
    // subcategory, the rest of its parent too; any other item weakly.
    let mut neg: Vec<(&[f32], f32)> = Vec::new();
    let step = (decided.len() / (SAMPLE * 2)).max(1);
    for (i, (id, cat, review)) in decided.iter().enumerate() {
        if *cat == Some(cat_id) {
            continue;
        }
        let Some(v) = index.get(*id) else { continue };
        let w = if *review == 3 {
            1.0
        } else if parent.is_some() && *cat == parent {
            0.5
        } else if i % step == 0 {
            0.3
        } else {
            continue;
        };
        neg.push((v, w));
    }
    let pos: Vec<(&[f32], f32)> = examples.iter().map(|v| (*v, 1.0)).collect();
    let Some(model) = classifier::train(&pos, &neg) else { return Ok(None) };

    let mut proposed = 0u32;
    for (id, cat, review) in &decided {
        if *review == 3 || *cat == Some(cat_id) {
            continue;
        }
        // A subcategory only takes items of its parent.
        if parent.is_some() && *cat != parent {
            continue;
        }
        let Some(v) = index.get(*id) else { continue };
        let p = model.predict(v);
        let need = if *review == 2 && parent.is_none() { PROPOSE_OVER_SORTED } else { PROPOSE };
        if p < need {
            continue;
        }
        edits::set(
            c,
            None,
            *id,
            &Change {
                field: "category",
                value: edits::category_value(Some(key), "taught", Some(p), 1),
                source: "taught",
                source_ref: Some(key),
                conf: Some(p),
                why: Some(format!("{name}, like the examples you gave ({:.0}%).", p * 100.0)),
            },
        )?;
        proposed += 1;
    }
    c.execute("UPDATE categories SET taught = 1 WHERE id = ?1", [cat_id])?;
    Ok(Some(Taught { examples: examples.len() as u32, proposed }))
}

/// Retrain every taught category (after new embeddings or corrections).
pub fn rerun_all(c: &Connection, index: &Index) -> rusqlite::Result<usize> {
    let keys: Vec<String> = {
        let mut st = c.prepare("SELECT key FROM categories WHERE taught = 1 ORDER BY parent_id IS NOT NULL, id")?;
        st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    let mut n = 0;
    for k in keys {
        n += run(c, &k, index)?.map_or(0, |t| t.proposed as usize);
    }
    Ok(n)
}

/// Items that look most like `ids` (not those themselves), best first.
pub fn similar(index: &Index, ids: &[i64], limit: usize) -> Vec<i64> {
    let vs: Vec<&[f32]> = ids.iter().filter_map(|id| index.get(*id)).collect();
    if vs.is_empty() {
        return Vec::new();
    }
    let mut centre = vec![0f32; vs[0].len()];
    for v in &vs {
        centre.iter_mut().zip(v.iter()).for_each(|(a, b)| *a += b);
    }
    vectors::normalize(&mut centre);
    let skip: HashSet<i64> = ids.iter().copied().collect();
    index.search(&centre, limit + skip.len(), None).into_iter().map(|(id, _)| id).filter(|id| !skip.contains(id)).take(limit).collect()
}

/// Remember that `key` is taught (examples were given in the UI).
pub fn mark(c: &Connection, key: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE categories SET taught = 1 WHERE key = ?1", params![key])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    fn unit(v: [f32; 4]) -> Vec<f32> {
        let mut v = v.to_vec();
        vectors::normalize(&mut v);
        v
    }

    #[test]
    fn learns_a_subcategory_from_examples() {
        let db = Db::memory().unwrap();
        let c = db.write();
        c.execute("INSERT INTO categories (key, name, parent_id) VALUES ('pogo', 'Pokémon GO', (SELECT id FROM categories WHERE key = 'screenshots'))", [])
            .unwrap();
        let shots: i64 = c.query_row("SELECT id FROM categories WHERE key = 'screenshots'", [], |r| r.get(0)).unwrap();
        let pogo: i64 = c.query_row("SELECT id FROM categories WHERE key = 'pogo'", [], |r| r.get(0)).unwrap();
        let mut index = Index::new(4);
        let mut add = |name: &str, v: [f32; 4], cat: i64, review: i64| {
            let id = insert_asset(&c, name);
            c.execute("UPDATE assets SET category_id = ?2, review = ?3 WHERE id = ?1", params![id, cat, review]).unwrap();
            index.insert(id, &unit(v));
            id
        };
        for i in 0..6 {
            add(&format!("ex{i}.png"), [1.0, 0.1 * i as f32, 0.0, 0.1], pogo, 3);
        }
        for i in 0..8 {
            add(&format!("chat{i}.png"), [0.0, 0.1, 1.0, 0.1 * i as f32], shots, 3);
        }
        let game = add("game.png", [0.95, 0.2, 0.05, 0.1], shots, 2);
        let chat = add("chat.png", [0.05, 0.1, 0.95, 0.3], shots, 2);
        let photo = add("photo.jpg", [0.9, 0.3, 0.0, 0.0], 1, 2); // not a screenshot
        let t = run(&c, "pogo", &index).unwrap().unwrap();
        assert_eq!(t.examples, 6);
        let cat_of = |id: i64| -> (Option<i64>, i64, Option<String>) {
            c.query_row("SELECT category_id, review, category_source FROM assets WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap()
        };
        assert_eq!(cat_of(game), (Some(pogo), 1, Some("taught".into())), "proposed, in Review");
        assert_eq!(cat_of(chat).0, Some(shots));
        assert_eq!(cat_of(photo).0, Some(1), "only items of the parent");
        let near = similar(&index, &[game], 3);
        assert_eq!(near.len(), 3);
        assert!(!near.contains(&game) && !near.contains(&chat), "{near:?}");
    }
}
