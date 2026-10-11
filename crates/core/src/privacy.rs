//! Applies the Private guard: text found in an item (OCR, its description)
//! that holds a card number, an IBAN or passport lines marks it Private,
//! with the reason in its history. The owner's own choice wins: an item
//! they marked not private stays that way.

use rusqlite::{Connection, params};
use serde_json::json;

use crate::edits::{self, Change};
use crate::tags;

/// Check `text` from `asset`; returns the reason when it was marked.
pub fn guard(c: &Connection, asset: i64, text: &str) -> rusqlite::Result<Option<String>> {
    let found = roll_ai::privacy::scan(text);
    if found.is_empty() {
        return Ok(None);
    }
    let owner_decided: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM field_history WHERE asset_id = ?1 AND field = 'private' AND source = 'manual')",
        [asset],
        |r| r.get(0),
    )?;
    if owner_decided {
        return Ok(None);
    }
    let why = found.iter().map(|f| f.why.as_str()).collect::<Vec<_>>().join("; ");
    let change = Change { field: "private", value: json!(true), source: "guard", source_ref: None, conf: None, why: Some(why.clone()) };
    if !edits::set(c, None, asset, &change)? {
        return Ok(None);
    }
    c.execute(
        "UPDATE asset_tags SET source = 'guard' WHERE asset_id = ?1 AND tag_id = (SELECT id FROM tags WHERE path = ?2)",
        params![asset, tags::PRIVATE],
    )?;
    Ok(Some(why))
}

/// Run the guard over the descriptions of `ids` (all items when `None`).
/// Returns how many were marked.
pub fn guard_descriptions(c: &Connection, ids: Option<&[i64]>) -> rusqlite::Result<usize> {
    let rows: Vec<(i64, String)> = {
        let mut st = c.prepare("SELECT id, description FROM assets WHERE description IS NOT NULL AND private = 0")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    let wanted: Option<std::collections::HashSet<i64>> = ids.map(|i| i.iter().copied().collect());
    let mut n = 0;
    for (id, text) in rows {
        if wanted.as_ref().is_none_or(|w| w.contains(&id)) && guard(c, id, &text)?.is_some() {
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    #[test]
    fn marks_private_with_a_safe_reason() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let card = insert_asset(&c, "a.jpg");
        let plain = insert_asset(&c, "b.jpg");
        let decided = insert_asset(&c, "c.jpg");
        c.execute("UPDATE assets SET description = 'my card 4242 4242 4242 4242' WHERE id IN (?1, ?2)", [card, decided]).unwrap();
        c.execute("UPDATE assets SET description = 'Lunch in Rome' WHERE id = ?1", [plain]).unwrap();
        // The owner said "not private" for this one before.
        let manual = Change { field: "private", value: json!(false), source: "manual", source_ref: None, conf: None, why: None };
        c.execute("UPDATE assets SET private = 1 WHERE id = ?1", [decided]).unwrap();
        edits::set(&c, None, decided, &manual).unwrap();

        assert_eq!(guard_descriptions(&c, None).unwrap(), 1);
        let private = |id: i64| c.query_row("SELECT private FROM assets WHERE id = ?1", [id], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!((private(card), private(plain), private(decided)), (1, 0, 0));
        let (why, source): (String, String) = c
            .query_row("SELECT why, source FROM field_history WHERE asset_id = ?1 AND field = 'private'", [card], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((why.as_str(), source.as_str()), ("Payment card number ending 4242", "guard"));
        assert!(tags::of_asset(&c, card).unwrap().iter().any(|t| t == tags::PRIVATE));
        let tag_source: String = c.query_row("SELECT source FROM asset_tags WHERE asset_id = ?1", [card], |r| r.get(0)).unwrap();
        assert_eq!(tag_source, "guard");
        // Running again changes nothing.
        assert_eq!(guard_descriptions(&c, None).unwrap(), 0);
    }
}
