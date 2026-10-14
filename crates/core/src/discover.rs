//! Discover: clusters of undecided items that probably belong together, so
//! one decision sorts many. By metadata signature (same format, same pixel
//! size, same encoder, no camera): 4,122 files at 854×1590 from one app
//! become one cluster. And, with the image model, by look: undecided items
//! whose embeddings lie close together.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Cluster {
    pub key: String,
    /// "854 × 1590 JPEG, no camera data, one encoder".
    pub label: String,
    pub count: i32,
    /// Up to 12 for the card.
    pub samples: Vec<i32>,
    pub ids: Vec<i32>,
    /// The most common proposed category, if any.
    pub proposed: Option<String>,
}

const MIN_SIZE: usize = 5;

/// A name the decision model suggests for a group.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NameSuggestion {
    pub name: String,
    pub definition: String,
    /// An existing category that already fits.
    pub existing: Option<String>,
    pub cost_usd: f64,
}

fn describe(signature: &str) -> String {
    let parts: Vec<&str> = signature.splitn(4, ':').collect();
    let format = parts.first().map(|f| f.to_uppercase()).unwrap_or_default();
    let size = parts.get(1).map(|s| s.replace('x', " × ")).unwrap_or_default();
    format!("{size} {format}, no camera data, one encoder")
}

/// Clusters of undecided items, largest first. `split` breaks one cluster
/// up by folder.
pub fn clusters(c: &Connection, split: Option<&str>) -> rusqlite::Result<Vec<Cluster>> {
    let rows: Vec<(i64, String, String, Option<String>)> = {
        let mut st = c.prepare(
            "SELECT a.id, a.signature, a.dir, CASE WHEN a.review = 1 THEN cat.key END FROM assets a LEFT JOIN categories cat ON cat.id = a.category_id
             WHERE a.missing = 0 AND a.trashed_at IS NULL AND a.review < 2 AND a.kind = 0
               AND (a.group_role IS NULL OR a.group_role = 'primary')
               AND a.signature LIKE '%:nocam:%' AND a.signature NOT LIKE '%:0x0:%'
             ORDER BY a.taken_local, a.id",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?
    };
    let dismissed: HashSet<String> = {
        let mut st = c.prepare("SELECT key FROM dismissed WHERE kind = 'discover'")?;
        st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    let mut by: HashMap<String, Vec<(i64, Option<String>)>> = HashMap::new();
    let mut label_of: HashMap<String, String> = HashMap::new();
    for (id, sig, dir, proposed) in rows {
        let key = match split {
            Some(s) if s == sig => format!("{sig}|{dir}"),
            Some(_) => continue,
            None => sig.clone(),
        };
        label_of.entry(key.clone()).or_insert_with(|| {
            let base = describe(&sig);
            if split.is_some() { format!("{base}, in {}", if dir.is_empty() { "the library folder" } else { &dir }) } else { base }
        });
        by.entry(key).or_default().push((id, proposed));
    }
    let mut out: Vec<Cluster> = by
        .into_iter()
        .filter(|(k, v)| v.len() >= if split.is_some() { 1 } else { MIN_SIZE } && !dismissed.contains(k))
        .map(|(key, v)| {
            let mut votes: HashMap<&str, usize> = HashMap::new();
            for (_, p) in &v {
                if let Some(p) = p {
                    *votes.entry(p.as_str()).or_default() += 1;
                }
            }
            let proposed = votes.into_iter().max_by_key(|(_, n)| *n).filter(|(_, n)| n * 2 > v.len()).map(|(k, _)| k.to_string());
            let ids: Vec<i32> = v.iter().map(|(id, _)| *id as i32).collect();
            Cluster {
                label: label_of[&key].clone(),
                count: ids.len() as i32,
                samples: ids.iter().copied().take(12).collect(),
                ids,
                proposed,
                key,
            }
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.key.cmp(&b.key)));
    Ok(out)
}

/// Items at least this similar to a group's centre join it.
const LOOK_ALIKE: f32 = 0.80;

/// A look-alike group while it grows: the sum of its members' vectors,
/// their normalized centre, and the members (with their proposed category).
type Group = (Vec<f32>, Vec<f32>, Vec<(i64, Option<String>)>);

/// Groups of undecided items that look alike (embeddings in `index`),
/// leaving out `taken` (items already in a signature cluster). Greedy:
/// each item joins the closest group centre within reach or starts one.
pub fn look_alike(
    c: &Connection,
    index: &roll_ai::vectors::Index,
    protos: Option<&crate::vision::Prototypes>,
    taken: &HashSet<i64>,
) -> rusqlite::Result<Vec<Cluster>> {
    let rows: Vec<(i64, Option<String>)> = {
        let mut st = c.prepare(
            "SELECT a.id, CASE WHEN a.review = 1 THEN cat.key END FROM assets a LEFT JOIN categories cat ON cat.id = a.category_id
             WHERE a.missing = 0 AND a.trashed_at IS NULL AND a.review < 2
               AND (a.group_role IS NULL OR a.group_role = 'primary')
             ORDER BY a.taken_local, a.id",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    let dismissed: HashSet<String> = {
        let mut st = c.prepare("SELECT key FROM dismissed WHERE kind = 'discover'")?;
        st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    let mut groups: Vec<Group> = Vec::new();
    for (id, proposed) in rows {
        if taken.contains(&id) {
            continue;
        }
        let Some(v) = index.get(id) else { continue };
        let best = groups
            .iter()
            .enumerate()
            .map(|(i, g)| (i, roll_ai::vectors::dot(&g.1, v)))
            .filter(|(_, s)| *s >= LOOK_ALIKE)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((i, _)) => {
                let g = &mut groups[i];
                g.0.iter_mut().zip(v).for_each(|(a, b)| *a += b);
                g.1 = g.0.clone();
                roll_ai::vectors::normalize(&mut g.1);
                g.2.push((id, proposed));
            }
            None => groups.push((v.to_vec(), v.to_vec(), vec![(id, proposed)])),
        }
    }
    let mut out = Vec::new();
    for (_, centre, members) in groups {
        if members.len() < MIN_SIZE {
            continue;
        }
        let key = format!("look:{}", members.iter().map(|m| m.0).min().unwrap_or(0));
        if dismissed.contains(&key) {
            continue;
        }
        let desc = protos.and_then(|p| crate::vision::zero_shot(p, &centre).first().map(|z| z.2.to_string()));
        let mut votes: HashMap<&str, usize> = HashMap::new();
        for (_, p) in &members {
            if let Some(p) = p {
                *votes.entry(p.as_str()).or_default() += 1;
            }
        }
        let proposed = votes.into_iter().max_by_key(|(_, n)| *n).filter(|(_, n)| n * 2 > members.len()).map(|(k, _)| k.to_string());
        let ids: Vec<i32> = members.iter().map(|m| m.0 as i32).collect();
        out.push(Cluster {
            label: match desc {
                Some(d) => format!("Look alike: {d}"),
                None => "Look alike".into(),
            },
            count: ids.len() as i32,
            samples: ids.iter().copied().take(12).collect(),
            ids,
            proposed,
            key,
        });
    }
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.key.cmp(&b.key)));
    Ok(out)
}

pub fn ignore(c: &Connection, key: &str) -> rusqlite::Result<()> {
    c.execute("INSERT OR REPLACE INTO dismissed (kind, key, at) VALUES ('discover', ?1, ?2)", params![key, crate::db::now_ms()])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    #[test]
    fn clusters_by_signature() {
        let db = Db::memory().unwrap();
        let c = db.write();
        for i in 0..6 {
            let id = insert_asset(&c, &format!("{}/IMG_{i}.jpeg", if i < 4 { "2020" } else { "2021" }));
            c.execute("UPDATE assets SET signature = 'jpeg:854x1590:nocam:q1' WHERE id = ?1", [id]).unwrap();
        }
        for i in 0..3 {
            let id = insert_asset(&c, &format!("x/{i}.png"));
            c.execute("UPDATE assets SET signature = 'png:100x100:nocam:-' WHERE id = ?1", [id]).unwrap();
        }
        let cam = insert_asset(&c, "DSC_1.JPG");
        c.execute("UPDATE assets SET signature = 'jpeg:6000x4000:cam:q2' WHERE id = ?1", [cam]).unwrap();
        let cl = clusters(&c, None).unwrap();
        assert_eq!(cl.len(), 1, "small groups and camera photos are left out");
        assert_eq!(cl[0].count, 6);
        assert_eq!(cl[0].label, "854 × 1590 JPEG, no camera data, one encoder");
        let parts = clusters(&c, Some("jpeg:854x1590:nocam:q1")).unwrap();
        assert_eq!(parts.iter().map(|p| p.count).collect::<Vec<_>>(), vec![4, 2]);
        ignore(&c, &cl[0].key).unwrap();
        assert!(clusters(&c, None).unwrap().is_empty());
    }

    #[test]
    fn groups_items_that_look_alike() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let mut index = roll_ai::vectors::Index::new(3);
        let mut ids = Vec::new();
        for i in 0..12 {
            let id = insert_asset(&c, &format!("x/{i}.jpg"));
            let mut v = if i < 7 { vec![1.0, 0.05 * i as f32, 0.0] } else { vec![0.0, 0.1, 1.0] };
            roll_ai::vectors::normalize(&mut v);
            index.insert(id, &v);
            ids.push(id);
        }
        let taken: HashSet<i64> = [ids[11]].into();
        let g = look_alike(&c, &index, None, &taken).unwrap();
        assert_eq!(g.len(), 1, "the second group has 4 items, below the minimum");
        assert_eq!(g[0].count, 7);
        assert_eq!(g[0].label, "Look alike");
        ignore(&c, &g[0].key).unwrap();
        assert!(look_alike(&c, &index, None, &taken).unwrap().is_empty());
    }
}
