//! Batch edits from the UI: one action on a selection becomes one undoable
//! batch. Actions apply to whole groups (a Live Photo's still and motion
//! always share a category and tags).

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use crate::edits::{self, Change};
use crate::trash::with_groups;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditAction {
    /// `None` makes items unsorted again.
    Category {
        key: Option<String>,
    },
    AddTag {
        path: String,
    },
    RemoveTag {
        path: String,
    },
    Rating {
        rating: i32,
    },
    Favorite {
        on: bool,
    },
    Description {
        text: Option<String>,
    },
    Private {
        on: bool,
    },
    Immich {
        mode: String,
    },
    /// Accept the proposed category of each item as it is.
    Accept,
}

fn plural(n: usize) -> String {
    if n == 1 { "1 item".into() } else { format!("{n} items") }
}

pub fn label(c: &Connection, action: &EditAction, n: usize) -> String {
    let items = plural(n);
    match action {
        EditAction::Category { key: Some(k) } => {
            let name: String = c.query_row("SELECT name FROM categories WHERE key = ?1", [k], |r| r.get(0)).unwrap_or_else(|_| k.clone());
            format!("Move {items} to {name}")
        }
        EditAction::Category { key: None } => format!("Mark {items} as unsorted"),
        EditAction::AddTag { path } => format!("Tag {items} {path}"),
        EditAction::RemoveTag { path } => format!("Remove {path} from {items}"),
        EditAction::Rating { rating } => format!("Rate {items} {rating}"),
        EditAction::Favorite { on: true } => format!("Favorite {items}"),
        EditAction::Favorite { on: false } => format!("Remove {items} from favorites"),
        EditAction::Description { .. } => format!("Describe {items}"),
        EditAction::Private { on: true } => format!("Mark {items} private"),
        EditAction::Private { on: false } => format!("Mark {items} not private"),
        EditAction::Immich { mode } => match mode.as_str() {
            "send" => format!("Send {items} to Immich"),
            "local" => format!("Keep {items} local only"),
            _ => format!("Use the default Immich choice for {items}"),
        },
        EditAction::Accept => format!("Accept {items}"),
    }
}

fn manual(field: &'static str, value: Value) -> Change<'static> {
    Change { field, value, source: "manual", source_ref: None, conf: None, why: None }
}

/// Apply an action. Returns the batch id and how many items changed.
pub fn apply(c: &Connection, ids: &[i64], action: &EditAction) -> rusqlite::Result<(i64, usize)> {
    let all = with_groups(c, ids)?;
    let batch = edits::begin_batch(c, "edit", &label(c, action, ids.len()))?;
    let mut changed = 0;
    for id in all {
        let did = match action {
            EditAction::Category { key } => {
                let name: Option<String> =
                    key.as_ref().and_then(|k| c.query_row("SELECT name FROM categories WHERE key = ?1", [k], |r| r.get(0)).ok());
                let review = if key.is_some() { 3 } else { 0 };
                let why = name.map(|n| format!("{n}, chosen by you."));
                edits::set(
                    c,
                    Some(batch),
                    id,
                    &Change { why, ..manual("category", edits::category_value(key.as_deref(), "manual", None, review)) },
                )?
            }
            EditAction::Accept => {
                let cur = edits::read(c, id, "category")?;
                match cur.get("key").and_then(Value::as_str) {
                    Some(k) if cur.get("review").and_then(Value::as_i64).unwrap_or(0) < 3 => {
                        let v = edits::category_value(Some(k), "manual", cur.get("conf").and_then(Value::as_f64).map(|f| f as f32), 3);
                        edits::set(c, Some(batch), id, &manual("category", v))?
                    }
                    _ => false,
                }
            }
            EditAction::AddTag { path } | EditAction::RemoveTag { path } => {
                let mut list: Vec<Value> = edits::read(c, id, "tags")?.as_array().cloned().unwrap_or_default();
                let norm = roll_meta::tags::normalize(path).unwrap_or_default();
                let has = list.iter().any(|t| t.get("p").and_then(Value::as_str).is_some_and(|p| p.eq_ignore_ascii_case(&norm)));
                match (action, has) {
                    (EditAction::AddTag { .. }, false) if !norm.is_empty() => list.push(json!({"p": norm, "s": "manual"})),
                    (EditAction::RemoveTag { .. }, true) => {
                        list.retain(|t| !t.get("p").and_then(Value::as_str).is_some_and(|p| p.eq_ignore_ascii_case(&norm)))
                    }
                    _ => continue,
                }
                edits::set(c, Some(batch), id, &manual("tags", Value::Array(list)))?
            }
            EditAction::Rating { rating } => edits::set(c, Some(batch), id, &manual("rating", json!(rating.clamp(&0, &5))))?,
            EditAction::Favorite { on } => edits::set(c, Some(batch), id, &manual("favorite", json!(on)))?,
            EditAction::Description { text } => edits::set(c, Some(batch), id, &manual("description", json!(text)))?,
            EditAction::Private { on } => edits::set(c, Some(batch), id, &manual("private", json!(on)))?,
            EditAction::Immich { mode } => edits::set(c, Some(batch), id, &manual("immich", json!(mode)))?,
        };
        if did {
            changed += 1;
        }
    }
    if changed == 0 {
        c.execute("DELETE FROM batches WHERE id = ?1", [batch])?;
    }
    Ok((batch, changed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    #[test]
    fn group_members_follow_and_undo() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let a = insert_asset(&c, "IMG_1.HEIC");
        let b = insert_asset(&c, "IMG_1.MOV");
        c.execute("INSERT INTO groups (id, kind, key) VALUES (1, 'live', 'x')", []).unwrap();
        c.execute("UPDATE assets SET group_id = 1, group_role = CASE id WHEN ?1 THEN 'primary' ELSE 'motion' END", [a]).unwrap();
        let (batch, n) = apply(&c, &[a], &EditAction::AddTag { path: "Event/2019-08 Lake Garda".into() }).unwrap();
        assert_eq!(n, 2);
        assert_eq!(crate::tags::of_asset(&c, b).unwrap(), vec!["Event/2019-08 Lake Garda"]);
        let label: String = c.query_row("SELECT label FROM batches WHERE id = ?1", [batch], |r| r.get(0)).unwrap();
        assert_eq!(label, "Tag 1 item Event/2019-08 Lake Garda");
        // Adding again changes nothing and leaves no empty batch.
        let (_, n) = apply(&c, &[a], &EditAction::AddTag { path: "event/2019-08 lake garda".into() }).unwrap();
        assert_eq!(n, 0);
        edits::undo_batch(&c, batch).unwrap();
        assert!(crate::tags::of_asset(&c, b).unwrap().is_empty());
    }
}
