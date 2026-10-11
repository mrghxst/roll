//! Duplicates: exact copies (BLAKE3), visual near-duplicates (dHash), lower
//! quality copies of a photo the owner has in full quality (the WhatsApp
//! copy of a camera photo) and bursts. Each group suggests the best item to
//! keep; merging moves dates, places and tags to it and the others to Roll's
//! trash, as one undoable batch.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use crate::edits::{self, Change};

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DupeGroup {
    pub key: String,
    /// "exact", "similar", "copy" or "burst".
    pub kind: String,
    /// Best first: the suggested item to keep.
    pub ids: Vec<i32>,
    /// Why the first one is suggested.
    pub why: String,
}

#[derive(Debug, Clone)]
struct Item {
    id: i64,
    size: i64,
    pixels: i64,
    aspect: f64,
    blake3: Option<String>,
    dhash: Option<u64>,
    camera: bool,
    makernotes: bool,
    gps: bool,
    exif_date: bool,
    burst: Option<i64>,
    junk: bool,
    taken: Option<i64>,
    screenshot: bool,
}

/// The keep-best order: original camera file, then the largest resolution,
/// then the richest metadata, then the bigger file.
fn score(i: &Item) -> (bool, i64, i32, i64) {
    (i.camera && i.makernotes, i.pixels, i32::from(i.gps) + i32::from(i.exif_date) + i32::from(i.camera), i.size)
}

fn why(best: &Item, others: &[&Item]) -> String {
    if best.camera && best.makernotes && others.iter().any(|o| !o.camera) {
        "The original camera file.".into()
    } else if others.iter().all(|o| o.pixels < best.pixels) {
        "The largest resolution.".into()
    } else if others.iter().all(|o| (o.gps as i32 + o.exif_date as i32) < (best.gps as i32 + best.exif_date as i32)) {
        "The richest metadata.".into()
    } else {
        "Identical; the first one found.".into()
    }
}

/// Union-find over item indices.
struct Sets(Vec<usize>);

impl Sets {
    fn find(&mut self, i: usize) -> usize {
        let mut r = i;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut j = i;
        while self.0[j] != r {
            let next = self.0[j];
            self.0[j] = r;
            j = next;
        }
        r
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra] = rb;
        }
    }
}

/// Visual near-duplicates: dHash within this many bits, found through four
/// 16-bit bands (two hashes within 3 bits share a band; most within 6 do).
const NEAR_BITS: u32 = 6;

pub fn groups(c: &Connection) -> rusqlite::Result<Vec<DupeGroup>> {
    let items: Vec<Item> = {
        let mut st = c.prepare(
            "SELECT a.id, a.size, coalesce(a.width, 0) * coalesce(a.height, 0), a.width, a.height, a.blake3, a.phash,
                    a.make IS NOT NULL OR a.model IS NOT NULL, a.has_makernotes, a.lat IS NOT NULL,
                    a.date_source IN ('exif_original', 'exif_digitized', 'quicktime'),
                    CASE WHEN g.kind = 'burst' THEN a.group_id END, a.stats LIKE '%\"junk\":\"%', a.taken_utc,
                    a.screenshot != 0 OR coalesce((SELECT key FROM categories WHERE id = a.category_id), '') = 'screenshots'
             FROM assets a LEFT JOIN groups g ON g.id = a.group_id
             WHERE a.missing = 0 AND a.trashed_at IS NULL AND a.kind = 0 AND (a.group_role IS NULL OR a.group_role = 'primary' OR g.kind = 'burst')",
        )?;
        st.query_map([], |r| {
            let (w, h): (Option<f64>, Option<f64>) = (r.get(3)?, r.get(4)?);
            Ok(Item {
                id: r.get(0)?,
                size: r.get(1)?,
                pixels: r.get(2)?,
                aspect: match (w, h) {
                    (Some(w), Some(h)) if h > 0.0 => w / h,
                    _ => 0.0,
                },
                blake3: r.get(5)?,
                dhash: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
                camera: r.get(7)?,
                makernotes: r.get::<_, i64>(8)? != 0,
                gps: r.get(9)?,
                exif_date: r.get::<_, Option<bool>>(10)?.unwrap_or(false),
                burst: r.get(11)?,
                junk: r.get::<_, Option<bool>>(12)?.unwrap_or(false),
                taken: r.get(13)?,
                screenshot: r.get(14)?,
            })
        })?
        .collect::<Result<_, _>>()?
    };
    let dismissed: HashSet<String> = {
        let mut st = c.prepare("SELECT key FROM dismissed WHERE kind = 'dupe'")?;
        st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
    };

    let n = items.len();
    let mut exact = Sets((0..n).collect());
    let mut by_hash: HashMap<&str, usize> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        if let Some(h) = it.blake3.as_deref() {
            match by_hash.get(h) {
                Some(&j) => exact.union(i, j),
                None => {
                    by_hash.insert(h, i);
                }
            }
        }
    }
    let mut similar = Sets((0..n).collect());
    let mut bands: Vec<HashMap<u16, Vec<usize>>> = vec![HashMap::new(); 4];
    for (i, it) in items.iter().enumerate() {
        // Flat or junk images all hash alike; leave them out.
        let Some(h) = it.dhash.filter(|h| h.count_ones() > 6 && h.count_ones() < 58) else { continue };
        // Screenshots of one app look alike without being copies; until
        // embeddings can tell them apart, only exact copies count.
        if it.junk || it.screenshot {
            continue;
        }
        for (b, band) in bands.iter_mut().enumerate() {
            let key = (h >> (b * 16)) as u16;
            for &j in band.get(&key).into_iter().flatten() {
                let o = &items[j];
                let close = (h ^ o.dhash.unwrap_or(0)).count_ones() <= NEAR_BITS;
                let same_shape = it.aspect > 0.0 && o.aspect > 0.0 && (it.aspect / o.aspect - 1.0).abs() < 0.03;
                // Two camera originals are only near-duplicates when shot
                // moments apart; otherwise similar scenes would match.
                let both_originals = it.camera && o.camera && it.exif_date && o.exif_date;
                let moments = match (it.taken, o.taken) {
                    (Some(a), Some(b)) => (a - b).abs() <= 10 * 60_000,
                    _ => false,
                };
                // Without a camera original, ask for near-identical pictures.
                let neither = !it.camera && !o.camera;
                let strict = (h ^ o.dhash.unwrap_or(0)).count_ones() <= 2 && it.pixels == o.pixels;
                if close && same_shape && (!both_originals || moments) && (!neither || strict) {
                    similar.union(i, j);
                }
            }
            band.entry(key).or_default().push(i);
        }
    }

    let mut out = Vec::new();
    let mut taken: HashSet<usize> = HashSet::new();
    let collect = |sets: &mut Sets| -> Vec<Vec<usize>> {
        let mut m: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..n {
            m.entry(sets.find(i)).or_default().push(i);
        }
        m.into_values().filter(|g| g.len() > 1).collect()
    };
    let push = |kind: &str, mut members: Vec<usize>, out: &mut Vec<DupeGroup>| {
        members.sort_by(|a, b| score(&items[*b]).cmp(&score(&items[*a])).then(items[*a].id.cmp(&items[*b].id)));
        let mut ids: Vec<i64> = members.iter().map(|i| items[*i].id).collect();
        let key = {
            let mut k = ids.clone();
            k.sort();
            format!("{kind}:{}", k.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","))
        };
        if dismissed.contains(&key) {
            return;
        }
        let best = &items[members[0]];
        let others: Vec<&Item> = members[1..].iter().map(|i| &items[*i]).collect();
        let why = why(best, &others);
        ids.truncate(200);
        out.push(DupeGroup { key, kind: kind.into(), ids: ids.into_iter().map(|i| i as i32).collect(), why });
    };
    for g in collect(&mut exact) {
        taken.extend(&g);
        push("exact", g, &mut out);
    }
    for g in collect(&mut similar) {
        // Exact copies already form their own group; keep one of them here.
        let mut seen_hash = HashSet::new();
        let g: Vec<usize> = g
            .into_iter()
            .filter(|i| items[*i].blake3.as_deref().is_none_or(|h| seen_hash.insert(h.to_string()) || !taken.contains(i)))
            .collect();
        if g.len() < 2 {
            continue;
        }
        // Members of one burst are a burst, not duplicates.
        let bursts: HashSet<Option<i64>> = g.iter().map(|i| items[*i].burst).collect();
        if bursts.len() == 1 && !bursts.contains(&None) {
            continue;
        }
        let max_px = g.iter().map(|i| items[*i].pixels).max().unwrap_or(0);
        let lesser = g.iter().any(|i| items[*i].pixels * 2 < max_px || !items[*i].camera);
        let kind = if lesser && g.iter().any(|i| items[*i].camera) { "copy" } else { "similar" };
        push(kind, g, &mut out);
    }
    let mut bursts: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        if let Some(b) = it.burst {
            bursts.entry(b).or_default().push(i);
        }
    }
    for (_, g) in bursts {
        if g.len() > 1 {
            push("burst", g, &mut out);
        }
    }
    let order = |k: &str| match k {
        "exact" => 0,
        "copy" => 1,
        "similar" => 2,
        _ => 3,
    };
    out.sort_by(|a, b| order(&a.kind).cmp(&order(&b.kind)).then(b.ids.len().cmp(&a.ids.len())));
    Ok(out)
}

/// Keep `keep`, move the others to Roll's trash, and give `keep` the date,
/// place and tags it lacks. One batch; returns (batch, items trashed).
pub fn merge(c: &Connection, keep: i64, others: &[i64]) -> rusqlite::Result<(i64, usize)> {
    let others: Vec<i64> = others.iter().copied().filter(|o| *o != keep).collect();
    let n = others.len();
    let label = if n == 1 { "Merge 1 duplicate".to_string() } else { format!("Merge {n} duplicates") };
    let batch = edits::begin_batch(c, "edit", &label)?;
    let name: String = c.query_row("SELECT name FROM assets WHERE id = ?1", [keep], |r| r.get(0))?;
    let why = |what: &str, from: &str| Some(format!("{what} from the duplicate {from}."));
    let trusted = |v: &Value| {
        v.get("source")
            .and_then(Value::as_str)
            .is_some_and(|s| matches!(s, "exif_original" | "exif_digitized" | "quicktime" | "manual" | "xmp"))
    };
    let mut tags: Vec<Value> = edits::read(c, keep, "tags")?.as_array().cloned().unwrap_or_default();
    let mut tags_changed = false;
    for o in &others {
        let oname: String = c.query_row("SELECT name FROM assets WHERE id = ?1", [o], |r| r.get(0))?;
        let (kd, od) = (edits::read(c, keep, "date")?, edits::read(c, *o, "date")?);
        if !trusted(&kd) && trusted(&od) {
            edits::set(
                c,
                Some(batch),
                keep,
                &Change { field: "date", value: od, source: "manual", source_ref: None, conf: None, why: why("Date", &oname) },
            )?;
        }
        let (kl, ol) = (edits::read(c, keep, "location")?, edits::read(c, *o, "location")?);
        if kl.is_null() && !ol.is_null() {
            edits::set(
                c,
                Some(batch),
                keep,
                &Change { field: "location", value: ol, source: "manual", source_ref: None, conf: None, why: why("Place", &oname) },
            )?;
        }
        for t in edits::read(c, *o, "tags")?.as_array().cloned().unwrap_or_default() {
            let p = t.get("p").and_then(Value::as_str).unwrap_or("");
            if !p.is_empty() && !tags.iter().any(|x| x.get("p").and_then(Value::as_str).is_some_and(|q| q.eq_ignore_ascii_case(p))) {
                tags.push(json!({"p": p, "s": "manual"}));
                tags_changed = true;
            }
        }
        edits::set(
            c,
            Some(batch),
            *o,
            &Change {
                field: "trashed",
                value: json!(true),
                source: "manual",
                source_ref: None,
                conf: None,
                why: Some(format!("A duplicate of {name}.")),
            },
        )?;
    }
    if tags_changed {
        edits::set(
            c,
            Some(batch),
            keep,
            &Change {
                field: "tags",
                value: Value::Array(tags),
                source: "manual",
                source_ref: None,
                conf: None,
                why: Some("Tags from duplicates.".into()),
            },
        )?;
    }
    c.execute("UPDATE batches SET summary = ?2 WHERE id = ?1", params![batch, json!({"kept": keep}).to_string()])?;
    Ok((batch, n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    fn set(c: &Connection, id: i64, sql: &str) {
        c.execute(&format!("UPDATE assets SET {sql} WHERE id = ?1"), [id]).unwrap();
    }

    #[test]
    fn finds_groups_and_suggests_the_best() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let a = insert_asset(&c, "a.jpg");
        let b = insert_asset(&c, "b.jpg");
        set(&c, a, "blake3 = 'x', width = 4000, height = 3000");
        set(&c, b, "blake3 = 'x', width = 4000, height = 3000");
        // A camera photo and its small WhatsApp copy: dHash 2 bits apart.
        let cam = insert_asset(&c, "DSC_1.JPG");
        let wa = insert_asset(&c, "IMG-20190805-WA0007.jpg");
        let h: i64 = 0x0F0F_3C3C_5A5A_A5A5;
        set(&c, cam, &format!("phash = {h}, width = 6000, height = 4000, make = 'NIKON', has_makernotes = 1, blake3 = 'c'"));
        set(&c, wa, &format!("phash = {}, width = 1600, height = 1067, blake3 = 'w'", h ^ 0b101));
        // Unrelated.
        let x = insert_asset(&c, "x.jpg");
        set(&c, x, &format!("phash = {}, width = 6000, height = 4000", !h));
        let g = groups(&c).unwrap();
        assert_eq!(g.len(), 2, "{g:?}");
        assert_eq!(g[0].kind, "exact");
        assert_eq!(g[1].kind, "copy");
        assert_eq!(g[1].ids, vec![cam as i32, wa as i32]);
        assert_eq!(g[1].why, "The original camera file.");

        // Merge: the copy's place and tags move to the original.
        set(&c, wa, "lat = 45.0, lon = 10.0, loc_source = 'manual'");
        crate::tags::add(&c, wa, "Source/WhatsApp", "rule", None).unwrap();
        let (batch, n) = merge(&c, cam, &[wa]).unwrap();
        assert_eq!(n, 1);
        let lat: Option<f64> = c.query_row("SELECT lat FROM assets WHERE id = ?1", [cam], |r| r.get(0)).unwrap();
        assert_eq!(lat, Some(45.0));
        assert!(crate::tags::of_asset(&c, cam).unwrap().contains(&"Source/WhatsApp".to_string()));
        let trashed: bool = c.query_row("SELECT trashed_at IS NOT NULL FROM assets WHERE id = ?1", [wa], |r| r.get(0)).unwrap();
        assert!(trashed);
        edits::undo_batch(&c, batch).unwrap();
        let trashed: bool = c.query_row("SELECT trashed_at IS NOT NULL FROM assets WHERE id = ?1", [wa], |r| r.get(0)).unwrap();
        assert!(!trashed);
        // Dismissed groups stay away.
        crate::places::dismiss(&c, "dupe", &g[0].key).unwrap();
        assert_eq!(groups(&c).unwrap().len(), 1);
    }
}
