//! Asset groups: parts that belong together, always move together and show
//! as one tile. Live Photos, RAW + JPEG pairs, edited + original, bursts.

use std::collections::HashMap;

use roll_meta::detect::is_raw_ext;
use rusqlite::{Connection, params};

use crate::db::Db;

#[derive(Debug, Clone)]
pub struct Member {
    pub id: i64,
    pub dir: String,
    pub name: String,
    pub ext: String,
    pub video: bool,
    pub content_id: Option<String>,
    pub burst_id: Option<String>,
    pub taken_utc: Option<i64>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub kind: &'static str,
    pub key: String,
    /// (asset id, role). The first `primary` is the visible tile.
    pub members: Vec<(i64, &'static str)>,
}

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name)
}

const LIVE_MAX_MS: i64 = 4_000;

/// Find groups among the assets of one root. Each asset joins at most one
/// group; earlier kinds win (Live, RAW, edit, burst).
pub fn find(members: &[Member]) -> Vec<Group> {
    let mut out = Vec::new();
    let mut taken = std::collections::HashSet::new();
    let mut claim = |g: Group, taken: &mut std::collections::HashSet<i64>| {
        if g.members.len() >= 2 && g.members.iter().all(|(id, _)| !taken.contains(id)) {
            for (id, _) in &g.members {
                taken.insert(*id);
            }
            out.push(g);
        }
    };

    // Live Photos by content identifier.
    let mut by_cid: HashMap<&str, Vec<&Member>> = HashMap::new();
    for m in members {
        if let Some(c) = m.content_id.as_deref() {
            by_cid.entry(c).or_default().push(m);
        }
    }
    let mut cids: Vec<_> = by_cid.into_iter().collect();
    cids.sort_by(|a, b| a.0.cmp(b.0));
    for (cid, ms) in cids {
        let (Some(still), Some(motion)) = (ms.iter().find(|m| !m.video), ms.iter().find(|m| m.video)) else { continue };
        claim(Group { kind: "live", key: cid.to_string(), members: vec![(still.id, "primary"), (motion.id, "motion")] }, &mut taken);
    }

    let mut by_dir: HashMap<&str, Vec<&Member>> = HashMap::new();
    for m in members {
        by_dir.entry(&m.dir).or_default().push(m);
    }
    let mut dirs: Vec<_> = by_dir.into_iter().collect();
    dirs.sort_by(|a, b| a.0.cmp(b.0));
    for (dir, ms) in &dirs {
        let mut by_stem: HashMap<String, Vec<&Member>> = HashMap::new();
        for m in ms {
            by_stem.entry(stem(&m.name).to_lowercase()).or_default().push(m);
        }
        let mut stems: Vec<_> = by_stem.iter().collect();
        stems.sort_by(|a, b| a.0.cmp(b.0));
        for (s, list) in stems {
            // Live Photo by name: a still and a short clip with the same stem.
            let still = list.iter().find(|m| !m.video && !is_raw_ext(&m.ext));
            let clip = list.iter().find(|m| m.video && m.duration_ms.is_some_and(|d| d <= LIVE_MAX_MS + 1_000));
            if let (Some(a), Some(b)) = (still, clip) {
                claim(Group { kind: "live", key: format!("{dir}/{s}"), members: vec![(a.id, "primary"), (b.id, "motion")] }, &mut taken);
            }
            // RAW + JPEG with the same stem and time.
            let raw = list.iter().find(|m| is_raw_ext(&m.ext));
            let rendered = list.iter().find(|m| !m.video && !is_raw_ext(&m.ext));
            if let (Some(r), Some(j)) = (raw, rendered) {
                let same_time = match (r.taken_utc, j.taken_utc) {
                    (Some(a), Some(b)) => (a - b).abs() <= 2_000,
                    _ => true,
                };
                if same_time {
                    claim(Group { kind: "raw", key: format!("{dir}/{s}"), members: vec![(j.id, "primary"), (r.id, "raw")] }, &mut taken);
                }
            }
        }
        // Edited + original: IMG_E1234 with IMG_1234, `-edited` suffixes.
        for m in ms.iter() {
            let st = stem(&m.name);
            let orig_stem = if let Some(rest) = st.strip_prefix("IMG_E").filter(|r| r.chars().all(|c| c.is_ascii_digit())) {
                format!("IMG_{rest}")
            } else if let Some(base) = st.strip_suffix("-edited").or_else(|| st.strip_suffix(" (Edited)")) {
                base.to_string()
            } else {
                continue;
            };
            if let Some(o) = ms.iter().find(|o| o.id != m.id && stem(&o.name).eq_ignore_ascii_case(&orig_stem) && !o.video) {
                claim(
                    Group { kind: "edit", key: format!("{dir}/{orig_stem}"), members: vec![(m.id, "primary"), (o.id, "original")] },
                    &mut taken,
                );
            }
        }
        // FullSizeRender.jpg inside a folder named after the original.
        for m in ms.iter().filter(|m| stem(&m.name).eq_ignore_ascii_case("FullSizeRender")) {
            let (parent, folder) = dir.rsplit_once('/').map(|(p, f)| (p.to_string(), f)).unwrap_or((String::new(), dir));
            if let Some((_, siblings)) = dirs.iter().find(|(d, _)| *d == parent)
                && let Some(o) = siblings.iter().find(|o| stem(&o.name).eq_ignore_ascii_case(folder) && !o.video)
            {
                claim(
                    Group { kind: "edit", key: format!("{parent}/{folder}"), members: vec![(m.id, "primary"), (o.id, "original")] },
                    &mut taken,
                );
            }
        }
    }

    // Bursts by identifier.
    let mut by_burst: HashMap<&str, Vec<&Member>> = HashMap::new();
    for m in members {
        if let Some(b) = m.burst_id.as_deref() {
            by_burst.entry(b).or_default().push(m);
        }
    }
    let mut bursts: Vec<_> = by_burst.into_iter().collect();
    bursts.sort_by(|a, b| a.0.cmp(b.0));
    for (b, mut ms) in bursts {
        ms.sort_by(|a, b| a.name.cmp(&b.name));
        let members = ms.iter().enumerate().map(|(i, m)| (m.id, if i == 0 { "primary" } else { "burst" })).collect();
        claim(Group { kind: "burst", key: b.to_string(), members }, &mut taken);
    }
    out
}

pub fn load_members(c: &Connection, root_id: i64) -> rusqlite::Result<Vec<Member>> {
    let mut st = c.prepare(
        "SELECT id, dir, name, ext, kind, content_id, burst_id, taken_utc, duration_ms
         FROM assets WHERE root_id = ?1 AND missing = 0 AND trashed_at IS NULL",
    )?;
    let rows = st.query_map([root_id], |r| {
        Ok(Member {
            id: r.get(0)?,
            dir: r.get(1)?,
            name: r.get(2)?,
            ext: r.get(3)?,
            video: r.get::<_, i64>(4)? == 1,
            content_id: r.get(5)?,
            burst_id: r.get(6)?,
            taken_utc: r.get(7)?,
            duration_ms: r.get(8)?,
        })
    })?;
    rows.collect()
}

/// Recompute all groups of a root.
pub fn rebuild(db: &Db, root_id: i64) -> anyhow::Result<usize> {
    let members = load_members(&db.read(), root_id)?;
    let groups = find(&members);
    db.tx(|tx| {
        tx.execute("UPDATE assets SET group_id = NULL, group_role = NULL WHERE root_id = ?1", [root_id])?;
        for g in &groups {
            tx.execute("INSERT INTO groups (kind, key) VALUES (?1, ?2) ON CONFLICT(kind, key) DO NOTHING", params![g.kind, g.key])?;
            let gid: i64 = tx.query_row("SELECT id FROM groups WHERE kind = ?1 AND key = ?2", params![g.kind, g.key], |r| r.get(0))?;
            for (id, role) in &g.members {
                tx.execute("UPDATE assets SET group_id = ?2, group_role = ?3 WHERE id = ?1", params![id, gid, role])?;
            }
        }
        tx.execute("DELETE FROM groups WHERE id NOT IN (SELECT DISTINCT group_id FROM assets WHERE group_id IS NOT NULL)", [])?;
        Ok(())
    })?;
    Ok(groups.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: i64, dir: &str, name: &str) -> Member {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
        let video = matches!(ext.as_str(), "mov" | "mp4");
        Member {
            id,
            dir: dir.into(),
            name: name.into(),
            ext,
            video,
            content_id: None,
            burst_id: None,
            taken_utc: Some(1000),
            duration_ms: video.then_some(2000),
        }
    }

    #[test]
    fn finds_all_kinds() {
        let mut live_a = m(1, "d", "IMG_1.HEIC");
        live_a.content_id = Some("X".into());
        let mut live_b = m(2, "d", "clip.MOV");
        live_b.content_id = Some("X".into());
        let mut b1 = m(9, "d", "IMG_9.JPG");
        b1.burst_id = Some("B".into());
        let mut b2 = m(10, "d", "IMG_10.JPG");
        b2.burst_id = Some("B".into());
        let members = vec![
            live_a,
            live_b,
            m(3, "d", "IMG_2.HEIC"),
            m(4, "d", "IMG_2.MOV"),
            m(5, "d", "DSC_1.NEF"),
            m(6, "d", "DSC_1.JPG"),
            m(7, "d", "IMG_E3.JPG"),
            m(8, "d", "IMG_3.HEIC"),
            b1,
            b2,
            m(11, "d/IMG_4", "FullSizeRender.jpg"),
            m(12, "d", "IMG_4.HEIC"),
            m(13, "d", "lonely.jpg"),
        ];
        let g = find(&members);
        let kinds: Vec<(&str, Vec<i64>)> = g.iter().map(|g| (g.kind, g.members.iter().map(|m| m.0).collect())).collect();
        assert!(kinds.contains(&("live", vec![1, 2])));
        assert!(kinds.contains(&("live", vec![3, 4])));
        assert!(kinds.contains(&("raw", vec![6, 5])));
        assert!(kinds.contains(&("edit", vec![7, 8])));
        assert!(kinds.contains(&("edit", vec![11, 12])));
        assert!(kinds.contains(&("burst", vec![10, 9])) || kinds.contains(&("burst", vec![9, 10])));
        assert_eq!(g.len(), 6);
    }

    #[test]
    fn long_clip_is_not_live() {
        let mut v = m(2, "d", "IMG_2.MOV");
        v.duration_ms = Some(30_000);
        assert!(find(&[m(1, "d", "IMG_2.HEIC"), v]).is_empty());
    }
}
