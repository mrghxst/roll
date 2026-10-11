//! Review: proposals per category, most doubtful first; accepting in bulk;
//! and a random 1% audit of auto-accepted items that estimates how often
//! Roll is right ("estimated 97.8% correct, ±1.1%").

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::actions::{self, EditAction};
use crate::db::now_ms;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReviewTab {
    /// A category key, or "audit".
    pub key: String,
    pub name: String,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReviewItem {
    pub id: i32,
    pub conf: f32,
    pub why: String,
    pub thumb_v: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReviewSummary {
    pub reviewed: i32,
    pub auto_accepted: i32,
    pub left: i32,
    pub tabs: Vec<ReviewTab>,
    /// Share of audited auto-accepted items Roll got right, with a 95%
    /// margin; `None` before ten audits.
    pub precision: Option<f32>,
    pub margin: Option<f32>,
    pub audited: i32,
}

const LIVE: &str = "a.missing = 0 AND a.trashed_at IS NULL AND (a.group_role IS NULL OR a.group_role = 'primary')";

/// How many auto-accepted items to keep in the audit queue: 1%, at least 5.
fn audit_size(auto: i64) -> i64 {
    (auto / 100).max(5).min(auto)
}

pub fn summary(c: &Connection) -> rusqlite::Result<ReviewSummary> {
    let one = |sql: &str| -> rusqlite::Result<i32> { c.query_row(sql, [], |r| r.get(0)) };
    let reviewed = one(&format!("SELECT count(*) FROM assets a WHERE {LIVE} AND a.review = 3"))?;
    let auto = one(&format!("SELECT count(*) FROM assets a WHERE {LIVE} AND a.review = 2"))?;
    let left = one(&format!("SELECT count(*) FROM assets a WHERE {LIVE} AND a.review = 1"))?;
    let mut tabs: Vec<ReviewTab> = {
        let mut st = c.prepare(&format!(
            "SELECT c.key, c.name, count(*) FROM assets a JOIN categories c ON c.id = a.category_id
             WHERE {LIVE} AND a.review = 1 GROUP BY c.id ORDER BY count(*) DESC"
        ))?;
        st.query_map([], |r| Ok(ReviewTab { key: r.get(0)?, name: r.get(1)?, count: r.get(2)? }))?.collect::<Result<_, _>>()?
    };
    fill_audit(c, auto as i64)?;
    let pending = one("SELECT count(*) FROM audits WHERE ok IS NULL")?;
    if pending > 0 {
        tabs.push(ReviewTab { key: "audit".into(), name: "Audit".into(), count: pending });
    }
    let (n, ok): (i32, i32) =
        c.query_row("SELECT count(*), coalesce(sum(ok), 0) FROM audits WHERE ok IS NOT NULL", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let (precision, margin) = if n >= 10 {
        let p = ok as f64 / n as f64;
        (Some(p as f32), Some((1.96 * (p * (1.0 - p) / n as f64).sqrt()) as f32))
    } else {
        (None, None)
    };
    Ok(ReviewSummary { reviewed, auto_accepted: auto, left, tabs, precision, margin, audited: n })
}

/// Keep a random sample of auto-accepted items waiting for an audit.
fn fill_audit(c: &Connection, auto: i64) -> rusqlite::Result<()> {
    let have: i64 = c.query_row("SELECT count(*) FROM audits", [], |r| r.get(0))?;
    let want = audit_size(auto);
    if have >= want || auto == 0 {
        return Ok(());
    }
    c.execute(
        &format!(
            "INSERT OR IGNORE INTO audits (asset_id, category_key, ok, at)
             SELECT a.id, c.key, NULL, ?1 FROM assets a JOIN categories c ON c.id = a.category_id
             WHERE {LIVE} AND a.review = 2 AND a.id NOT IN (SELECT asset_id FROM audits)
             ORDER BY random() LIMIT ?2"
        ),
        params![now_ms(), want - have],
    )?;
    Ok(())
}

/// The items of one tab, most doubtful first.
pub fn items(c: &Connection, key: &str, limit: i64) -> rusqlite::Result<Vec<ReviewItem>> {
    let why = "(SELECT h.why FROM field_history h WHERE h.asset_id = a.id AND h.field = 'category' ORDER BY h.id DESC LIMIT 1)";
    let sql = if key == "audit" {
        format!(
            "SELECT a.id, coalesce(a.category_conf, 1), coalesce({why}, ''), a.thumb_v FROM audits u JOIN assets a ON a.id = u.asset_id
             WHERE u.ok IS NULL AND {LIVE} ORDER BY u.at, a.id LIMIT ?2"
        )
    } else {
        format!(
            "SELECT a.id, coalesce(a.category_conf, 0), coalesce({why}, ''), a.thumb_v FROM assets a JOIN categories c ON c.id = a.category_id
             WHERE {LIVE} AND a.review = 1 AND c.key = ?1 ORDER BY a.category_conf, a.taken_local, a.id LIMIT ?2"
        )
    };
    let mut st = c.prepare(&sql)?;
    st.query_map(params![key, limit], |r| Ok(ReviewItem { id: r.get(0)?, conf: r.get(1)?, why: r.get(2)?, thumb_v: r.get(3)? }))?.collect()
}

/// Record audit outcomes for items the owner just decided: right when the
/// category stayed, wrong when they chose another.
pub fn record_audits(c: &Connection, ids: &[i64]) -> rusqlite::Result<()> {
    for id in ids {
        let key: Option<String> =
            c.query_row("SELECT category_key FROM audits WHERE asset_id = ?1 AND ok IS NULL", [id], |r| r.get(0)).optional()?;
        let Some(key) = key else { continue };
        let now: Option<String> =
            c.query_row("SELECT c.key FROM assets a LEFT JOIN categories c ON c.id = a.category_id WHERE a.id = ?1", [id], |r| r.get(0))?;
        c.execute("UPDATE audits SET ok = ?2, at = ?3 WHERE asset_id = ?1", params![id, now.as_deref() == Some(key.as_str()), now_ms()])?;
    }
    Ok(())
}

/// Accept, in one batch, the proposals of a category at or above `min`
/// confidence. Returns how many.
pub fn accept_above(c: &Connection, key: &str, min: f32) -> rusqlite::Result<usize> {
    let ids: Vec<i64> = {
        let mut st = c.prepare(&format!(
            "SELECT a.id FROM assets a JOIN categories c ON c.id = a.category_id
             WHERE {LIVE} AND a.review = 1 AND c.key = ?1 AND a.category_conf >= ?2"
        ))?;
        st.query_map(params![key, min], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    if ids.is_empty() {
        return Ok(0);
    }
    let (_, n) = actions::apply(c, &ids, &EditAction::Accept)?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::edits;
    use crate::testutil::insert_asset;

    fn propose(c: &Connection, id: i64, key: &str, conf: f32, review: i64) {
        edits::write(c, id, "category", &edits::category_value(Some(key), "rule:test", Some(conf), review)).unwrap();
    }

    #[test]
    fn tabs_order_accept_and_audit() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let a = insert_asset(&c, "a.jpg");
        let b = insert_asset(&c, "b.jpg");
        let d = insert_asset(&c, "d.jpg");
        propose(&c, a, "photos", 0.97, 1);
        propose(&c, b, "photos", 0.85, 1);
        propose(&c, d, "memes", 0.6, 1);
        let s = summary(&c).unwrap();
        assert_eq!((s.left, s.tabs.len()), (3, 2));
        assert_eq!(s.tabs[0].key, "photos");
        let it = items(&c, "photos", 50).unwrap();
        assert_eq!(it.iter().map(|i| i.id as i64).collect::<Vec<_>>(), vec![b, a], "most doubtful first");
        assert_eq!(accept_above(&c, "photos", 0.95).unwrap(), 1);
        assert_eq!(summary(&c).unwrap().reviewed, 1);

        // Auto-accepted items get audited.
        let mut auto = Vec::new();
        for i in 0..12 {
            let id = insert_asset(&c, &format!("auto{i}.jpg"));
            propose(&c, id, "screenshots", 1.0, 2);
            auto.push(id);
        }
        let s = summary(&c).unwrap();
        let audit = s.tabs.iter().find(|t| t.key == "audit").unwrap();
        assert_eq!(audit.count, 5);
        let queue: Vec<i64> = items(&c, "audit", 50).unwrap().iter().map(|i| i.id as i64).collect();
        // The owner agrees with four and moves one to Memes.
        actions::apply(&c, &queue[..4], &EditAction::Accept).unwrap();
        actions::apply(&c, &queue[4..], &EditAction::Category { key: Some("memes".into()) }).unwrap();
        record_audits(&c, &queue).unwrap();
        let s = summary(&c).unwrap();
        assert_eq!(s.audited, 5);
        assert!(s.precision.is_none(), "too few audits for an estimate");
    }
}
