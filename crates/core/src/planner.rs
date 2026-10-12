//! Organize plans: where every decided item belongs according to its
//! category, event and the folder templates, and the operations that get it
//! there. A plan only lists differences, so re-foldering after a change
//! touches just what changed. Plans are stored as a `planned` batch and
//! applied by the journal.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use regex::Regex;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::db::now_ms;
use crate::paths::{join_rel, rel_dir};
use crate::template::{self, Ctx};

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanOptions {
    pub rename: bool,
    pub rename_template: String,
    /// Only these assets (and their groups); all when empty.
    #[ts(type = "Array<number>")]
    pub ids: Vec<i64>,
    /// Write pending metadata changes into files and sidecars.
    pub write_metadata: bool,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self { rename: false, rename_template: "{date} {time} {camera}".into(), ids: Vec::new(), write_metadata: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanFolder {
    pub root_id: i32,
    pub path: String,
    pub count: i32,
    pub samples: Vec<i32>,
    pub new: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanConflict {
    pub asset_id: i32,
    pub wanted: String,
    pub got: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanSummary {
    pub batch_id: i32,
    pub moved: i32,
    pub renamed: i32,
    pub unchanged: i32,
    pub undecided: i32,
    /// Private items that stay until the owner has reviewed them.
    pub private_held: i32,
    pub writes: i32,
    pub new_folders: i32,
    pub files: i32,
    pub conflicts: Vec<PlanConflict>,
    pub folders: Vec<PlanFolder>,
}

#[derive(Debug, Clone)]
struct Item {
    id: i64,
    root_id: i64,
    rel: String,
    name: String,
    group: Option<i64>,
    primary: bool,
    category: Option<String>,
    review: i64,
    private: bool,
    local: Option<String>,
    camera: Option<String>,
    dirty: bool,
}

pub(crate) struct Cat {
    name: String,
    parent: Option<String>,
    template: Option<String>,
    own_folder: bool,
}

pub(crate) fn load_categories(c: &Connection) -> rusqlite::Result<HashMap<String, Cat>> {
    let mut st = c.prepare(
        "SELECT c.key, c.name, p.key, c.folder_template, c.own_folder FROM categories c LEFT JOIN categories p ON p.id = c.parent_id",
    )?;
    st.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, Cat { name: r.get(1)?, parent: r.get(2)?, template: r.get(3)?, own_folder: r.get::<_, i64>(4)? != 0 }))
    })?
    .collect()
}

static EVENT_PREFIX: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}\b").unwrap());

/// An event's folder: `{year}/{yyyy-mm} {event}` from its first day, or
/// `{year}/{event}` when the name already starts with the month.
pub(crate) fn event_dir(name: &str, start_local: Option<String>) -> String {
    let t = if EVENT_PREFIX.is_match(name) { "{year}/{event}" } else { "{year}/{yyyy-mm} {event}" };
    template::render(t, &Ctx { local: start_local, event: Some(name.to_string()), ..Default::default() })
}

pub(crate) fn parent_of<'a>(cats: &'a HashMap<String, Cat>, key: &str) -> Option<&'a str> {
    cats.get(key)?.parent.as_deref()
}

/// The folder of an item in a category (not in an event folder).
pub(crate) fn category_dir(
    cats: &HashMap<String, Cat>,
    key: &str,
    local: Option<String>,
    source: Option<String>,
    camera: Option<String>,
) -> Option<String> {
    let (tmpl, cat_name, sub) = category_template(cats, key)?;
    let ctx = Ctx { local, category: Some(cat_name), subcategory: sub, source, camera, ..Default::default() };
    Some(template::render(&tmpl, &ctx))
}

/// Where items of a category go, before tokens are filled in.
fn category_template(cats: &HashMap<String, Cat>, key: &str) -> Option<(String, String, Option<String>)> {
    let c = cats.get(key)?;
    match &c.parent {
        None => Some((c.template.clone()?, c.name.clone(), None)),
        Some(p) => {
            let parent = cats.get(p)?;
            let base = parent.template.clone()?;
            if c.own_folder {
                Some((c.template.clone().unwrap_or_else(|| format!("{base}/{{subcategory}}")), parent.name.clone(), Some(c.name.clone())))
            } else {
                Some((base, parent.name.clone(), Some(c.name.clone())))
            }
        }
    }
}

pub(crate) fn case_key(rel: &str) -> String {
    if cfg!(any(windows, target_os = "macos")) { rel.to_lowercase() } else { rel.to_string() }
}

fn ext_of(name: &str) -> &str {
    name.rsplit_once('.').map(|(_, e)| e).unwrap_or("")
}

fn stem_of(name: &str) -> &str {
    name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name)
}

/// The name a sidecar gets when its media file is renamed.
pub(crate) fn sidecar_name(side: &str, old_media: &str, new_media: &str) -> String {
    if old_media == new_media {
        return side.to_string();
    }
    if let Some(rest) = side.strip_prefix(old_media) {
        return format!("{new_media}{rest}");
    }
    let (old_stem, new_stem) = (stem_of(old_media), stem_of(new_media));
    match side.strip_prefix(old_stem) {
        Some(rest) => format!("{new_stem}{rest}"),
        None => side.to_string(),
    }
}

/// One file to move: an asset or a sidecar.
#[derive(Debug, Clone)]
struct FileMove {
    asset: Option<i64>,
    sidecar: Option<i64>,
    root: i64,
    from: String,
    to: String,
}

/// Compute a plan and store it as the current `planned` batch.
pub fn plan(c: &Connection, opts: &PlanOptions) -> anyhow::Result<PlanSummary> {
    let roots: HashMap<i64, String> = {
        let mut st = c.prepare("SELECT id, path FROM roots WHERE kind = 'library'")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    let cats = load_categories(c)?;
    let items: Vec<Item> = {
        let mut st = c.prepare(
            "SELECT a.id, a.root_id, a.rel_path, a.name, a.group_id, a.group_role, c.key, a.review, a.taken_local, coalesce(a.model, a.make), a.meta_dirty,
                    a.private
             FROM assets a JOIN roots r ON r.id = a.root_id AND r.kind = 'library' LEFT JOIN categories c ON c.id = a.category_id
             WHERE a.missing = 0 AND a.trashed_at IS NULL",
        )?;
        st.query_map([], |r| {
            Ok(Item {
                id: r.get(0)?,
                root_id: r.get(1)?,
                rel: r.get(2)?,
                name: r.get(3)?,
                group: r.get(4)?,
                primary: r.get::<_, Option<String>>(5)?.is_none_or(|g| g == "primary"),
                category: r.get(6)?,
                review: r.get(7)?,
                local: r.get(8)?,
                camera: r.get(9)?,
                dirty: r.get::<_, i64>(10)? != 0,
                private: r.get::<_, i64>(11)? != 0,
            })
        })?
        .collect::<Result<_, _>>()?
    };
    // Confirmed events with their own folder.
    let events: HashMap<i64, (String, Option<String>)> = {
        let mut st = c.prepare(
            "SELECT ea.asset_id, e.name, (SELECT min(a2.taken_local) FROM event_assets ea2 JOIN assets a2 ON a2.id = ea2.asset_id WHERE ea2.event_id = e.id)
             FROM event_assets ea JOIN events e ON e.id = ea.event_id WHERE e.state = 'confirmed' AND e.own_folder = 1",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?.collect::<Result<_, _>>()?
    };
    let sources: HashMap<i64, String> = {
        let mut st = c.prepare(
            "SELECT at.asset_id, substr(t.path, 8) FROM asset_tags at JOIN tags t ON t.id = at.tag_id WHERE t.path LIKE 'Source/%'",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    let sidecars: HashMap<i64, Vec<(i64, String)>> = {
        let mut st = c.prepare("SELECT asset_id, id, rel_path FROM sidecars WHERE asset_id IS NOT NULL")?;
        let mut m: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
        for row in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))? {
            let (a, id, rel) = row?;
            m.entry(a).or_default().push((id, rel));
        }
        m
    };

    let scope: Option<HashSet<i64>> = (!opts.ids.is_empty()).then(|| {
        let wanted: HashSet<i64> = opts.ids.iter().copied().collect();
        let groups: HashSet<i64> = items.iter().filter(|i| wanted.contains(&i.id)).filter_map(|i| i.group).collect();
        items.iter().filter(|i| wanted.contains(&i.id) || i.group.is_some_and(|g| groups.contains(&g))).map(|i| i.id).collect()
    });
    let in_scope = |i: &Item| scope.as_ref().is_none_or(|s| s.contains(&i.id));

    let mut by_group: HashMap<i64, Vec<&Item>> = HashMap::new();
    for i in &items {
        if let Some(g) = i.group {
            by_group.entry(g).or_default().push(i);
        }
    }

    // Paths that stay where they are claim their names first.
    let mut claimed: HashSet<(i64, String)> = HashSet::new();
    let mut summary = PlanSummary::default();
    struct Unit<'a> {
        members: Vec<&'a Item>,
        dir: String,
        new_stem: Option<String>,
    }
    let mut units: Vec<Unit> = Vec::new();
    for i in items.iter().filter(|i| i.primary && in_scope(i)) {
        let members: Vec<&Item> = match i.group {
            Some(g) => {
                let mut m = by_group.get(&g).cloned().unwrap_or_default();
                m.sort_by_key(|x| !x.primary);
                m
            }
            None => vec![i],
        };
        let decided = i.review >= 2 && i.category.as_deref().is_some_and(|k| k != "unsorted");
        if !decided {
            summary.undecided += 1;
            continue;
        }
        // Private items move only after the owner looked at them.
        if members.iter().any(|m| m.private) && i.review < 3 {
            summary.private_held += 1;
            continue;
        }
        let key = i.category.clone().unwrap_or_default();
        let Some((tmpl, cat_name, sub)) = category_template(&cats, &key) else {
            summary.undecided += 1;
            continue;
        };
        let event =
            (key == "photos" || cats.get(&key).and_then(|c| c.parent.as_deref()) == Some("photos")).then(|| events.get(&i.id)).flatten();
        let dir = match event {
            Some((name, start)) => event_dir(name, start.clone().or(i.local.clone())),
            None => {
                let ctx = Ctx {
                    local: i.local.clone(),
                    category: Some(cat_name),
                    subcategory: sub,
                    source: sources.get(&i.id).cloned(),
                    camera: i.camera.clone(),
                    ..Default::default()
                };
                template::render(&tmpl, &ctx)
            }
        };
        let new_stem = opts.rename.then(|| {
            let base =
                template::render(&opts.rename_template, &Ctx { local: i.local.clone(), camera: i.camera.clone(), ..Default::default() });
            template::safe_segment(&base.replace('/', " "))
        });
        let unchanged = members.iter().all(|m| rel_dir(&m.rel) == dir && new_stem.as_ref().is_none_or(|s| stem_of(&m.name) == s));
        if unchanged {
            summary.unchanged += 1;
            continue;
        }
        units.push(Unit { members, dir, new_stem });
    }
    let moving: HashSet<i64> = units.iter().flat_map(|u| u.members.iter().map(|m| m.id)).collect();
    for i in &items {
        if !moving.contains(&i.id) {
            claimed.insert((i.root_id, case_key(&i.rel)));
            for (_, rel) in sidecars.get(&i.id).into_iter().flatten() {
                claimed.insert((i.root_id, case_key(rel)));
            }
        }
    }

    let mut moves: Vec<FileMove> = Vec::new();
    let mut folders: HashMap<(i64, String), PlanFolder> = HashMap::new();
    for u in &units {
        let root = u.members[0].root_id;
        let root_path = roots.get(&root).map(String::as_str).unwrap_or("");
        let target_names = |n: u32| -> Vec<(Option<i64>, Option<i64>, String, String)> {
            let mut out = Vec::new();
            for m in &u.members {
                let base = match &u.new_stem {
                    Some(s) => format!("{s}.{}", ext_of(&m.name)),
                    None => m.name.clone(),
                };
                let name = if n > 1 { template::numbered(&base, n) } else { base };
                let to = if u.dir.is_empty() { name.clone() } else { format!("{}/{name}", u.dir) };
                out.push((Some(m.id), None, m.rel.clone(), to.clone()));
                for (sid, srel) in sidecars.get(&m.id).into_iter().flatten() {
                    let sname = sidecar_name(srel.rsplit('/').next().unwrap_or(srel), &m.name, &name);
                    let sto = if u.dir.is_empty() { sname } else { format!("{}/{sname}", u.dir) };
                    out.push((None, Some(*sid), srel.clone(), sto));
                }
            }
            out
        };
        let mut n = 1;
        let names = loop {
            let names = target_names(n);
            // Free: the file's own current path, or unclaimed and not on disk.
            let free = names.iter().all(|(_, _, from, to)| {
                let k = case_key(to);
                case_key(from) == k || (!claimed.contains(&(root, k)) && !join_rel(Path::new(root_path), to).exists())
            });
            if free || n > 999 {
                break names;
            }
            n += 1;
        };
        if n > 1 {
            let wanted = target_names(1);
            summary.conflicts.push(PlanConflict { asset_id: u.members[0].id as i32, wanted: wanted[0].3.clone(), got: names[0].3.clone() });
        }
        if u.new_stem.is_some() {
            summary.renamed += 1;
        }
        summary.moved += 1;
        let f = folders.entry((root, u.dir.clone())).or_insert_with(|| PlanFolder {
            root_id: root as i32,
            path: u.dir.clone(),
            count: 0,
            samples: Vec::new(),
            new: !join_rel(Path::new(root_path), &u.dir).is_dir(),
        });
        f.count += 1;
        if f.samples.len() < 12 {
            f.samples.push(u.members[0].id as i32);
        }
        for (asset, sidecar, from, to) in names {
            claimed.insert((root, case_key(&to)));
            if from != to {
                moves.push(FileMove { asset, sidecar, root, from, to });
            }
        }
    }

    // Folders to create, parents first.
    let mut mkdirs: Vec<(i64, String)> = Vec::new();
    for ((root, dir), f) in &folders {
        if !f.new {
            continue;
        }
        let root_path = roots.get(root).map(String::as_str).unwrap_or("");
        let mut acc = String::new();
        for part in dir.split('/') {
            acc = if acc.is_empty() { part.to_string() } else { format!("{acc}/{part}") };
            if !join_rel(Path::new(root_path), &acc).is_dir() && !mkdirs.contains(&(*root, acc.clone())) {
                mkdirs.push((*root, acc.clone()));
            }
        }
    }
    mkdirs.sort_by_key(|(_, d)| d.matches('/').count());

    // Source folders left empty afterwards are removed (undo recreates them).
    let mut leaving: HashMap<(i64, String), usize> = HashMap::new();
    for m in &moves {
        if rel_dir(&m.from) != rel_dir(&m.to) {
            *leaving.entry((m.root, rel_dir(&m.from))).or_default() += 1;
        }
    }
    let mut rmdirs: Vec<(i64, String)> = Vec::new();
    for ((root, dir), n) in &leaving {
        if dir.is_empty() {
            continue;
        }
        let root_path = roots.get(root).map(String::as_str).unwrap_or("");
        let on_disk = std::fs::read_dir(join_rel(Path::new(root_path), dir)).map(|r| r.count()).unwrap_or(usize::MAX);
        if on_disk == *n && !folders.contains_key(&(*root, dir.clone())) {
            rmdirs.push((*root, dir.clone()));
        }
    }
    rmdirs.sort_by_key(|(_, d)| std::cmp::Reverse(d.matches('/').count()));

    let writes: Vec<(i64, i64, String)> = if opts.write_metadata {
        items.iter().filter(|i| i.dirty && in_scope(i)).map(|i| (i.id, i.root_id, i.rel.clone())).collect()
    } else {
        Vec::new()
    };

    summary.writes = writes.len() as i32;
    summary.new_folders = mkdirs.len() as i32;
    summary.files = moves.len() as i32;
    let mut fl: Vec<PlanFolder> = folders.into_values().collect();
    fl.sort_by(|a, b| a.path.cmp(&b.path));
    summary.folders = fl;

    // Store as the one current plan.
    c.execute("DELETE FROM batches WHERE state = 'planned'", [])?;
    let label = plan_label(&summary);
    c.execute(
        "INSERT INTO batches (kind, label, state, created_at, summary) VALUES ('organize', ?1, 'planned', ?2, ?3)",
        params![label, now_ms(), serde_json::to_string(&summary)?],
    )?;
    let batch = c.last_insert_rowid();
    summary.batch_id = batch as i32;
    let mut seq = 0;
    let mut st = c.prepare("INSERT INTO ops (batch_id, seq, kind, asset_id, src, dst, payload) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?;
    for (root, dir) in &mkdirs {
        seq += 1;
        st.execute(params![batch, seq, "mkdir", None::<i64>, None::<String>, dir, serde_json::json!({"root": root}).to_string()])?;
    }
    for (id, root, rel) in &writes {
        seq += 1;
        st.execute(params![batch, seq, "write_meta", id, rel, rel, serde_json::json!({"root": root}).to_string()])?;
    }
    for m in &moves {
        seq += 1;
        st.execute(params![
            batch,
            seq,
            "move",
            m.asset,
            m.from,
            m.to,
            serde_json::json!({"root": m.root, "sidecar": m.sidecar}).to_string()
        ])?;
    }
    for (root, dir) in &rmdirs {
        seq += 1;
        st.execute(params![batch, seq, "rmdir", None::<i64>, dir, None::<String>, serde_json::json!({"root": root}).to_string()])?;
    }
    c.execute("UPDATE batches SET summary = ?2 WHERE id = ?1", params![batch, serde_json::to_string(&summary)?])?;
    Ok(summary)
}

pub fn plan_label(s: &PlanSummary) -> String {
    match (s.moved, s.writes) {
        (0, 0) => "Nothing to change".into(),
        (m, 0) => format!("Organize {m} items"),
        (0, w) => format!("Save changes to {w} files"),
        (m, w) => format!("Organize {m} items and save changes to {w} files"),
    }
}

/// The plan as CSV: one line per file operation.
pub fn csv(c: &Connection, batch: i64) -> rusqlite::Result<String> {
    let mut out = String::from("operation,asset,from,to\n");
    let mut st = c.prepare("SELECT kind, asset_id, src, dst FROM ops WHERE batch_id = ?1 ORDER BY seq")?;
    let q = |s: Option<String>| format!("\"{}\"", s.unwrap_or_default().replace('"', "\"\""));
    for row in st.query_map([batch], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?))
    })? {
        let (k, a, s, d) = row?;
        out.push_str(&format!("{k},{},{},{}\n", a.map(|a| a.to_string()).unwrap_or_default(), q(s), q(d)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_items_wait_for_review() {
        let db = crate::db::Db::memory().unwrap();
        let c = db.write();
        let a = crate::testutil::insert_asset(&c, "in/a.png");
        let b = crate::testutil::insert_asset(&c, "in/b.png");
        c.execute(
            "UPDATE assets SET review = 2, taken_local = '2020-05-01T10:00:00',
               category_id = (SELECT id FROM categories WHERE key = 'screenshots')",
            [],
        )
        .unwrap();
        c.execute("UPDATE assets SET private = 1 WHERE id = ?1", [b]).unwrap();
        let s = plan(&c, &PlanOptions::default()).unwrap();
        assert_eq!((s.moved, s.private_held), (1, 1));
        assert_eq!(s.folders.len(), 1);
        let _ = a;
        c.execute("UPDATE assets SET review = 3 WHERE id = ?1", [b]).unwrap();
        let s = plan(&c, &PlanOptions::default()).unwrap();
        assert_eq!((s.moved, s.private_held), (2, 0));
    }

    #[test]
    fn sidecar_names_follow() {
        assert_eq!(sidecar_name("IMG_1.HEIC.xmp", "IMG_1.HEIC", "2019-08-12 x.HEIC"), "2019-08-12 x.HEIC.xmp");
        assert_eq!(sidecar_name("IMG_1.AAE", "IMG_1.HEIC", "new.HEIC"), "new.AAE");
        assert_eq!(sidecar_name("IMG_1.HEIC.json", "IMG_1.HEIC", "IMG_1.HEIC"), "IMG_1.HEIC.json");
    }
}
