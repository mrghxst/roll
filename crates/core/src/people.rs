//! People (PROMPT M7): faces from the faces pack, grouped into people.
//! A face joins the person whose faces it is most like (on average); faces
//! that fit nobody start a new person once three of them, from three
//! different items, look alike. Two faces in one item are never one person.
//! Names and changes by the owner are the `people` field in history, so
//! naming, merging and "not this person" undo like any edit.

use std::collections::{HashMap, HashSet};

use roll_ai::vectors;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use crate::db::now_ms;

/// The faces pack's models; a new one scans again.
pub const MODEL: &str = "yunet-2023mar+sface-2021dec";
/// A face this alike to a person's faces (on average) joins them.
pub const JOIN: f32 = 0.42;
/// Faces this alike start a new person together.
pub const SEED: f32 = 0.45;
/// Faces (in as many items) before a group becomes a person.
pub const MIN_PERSON: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FaceBox {
    pub id: i32,
    pub asset: i32,
    /// Left, top, width and height, 0–1 of the displayed image.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub person: Option<i32>,
    /// The item's width over its height, to cut out a square face.
    pub ratio: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Person {
    pub id: i32,
    pub name: Option<String>,
    pub faces: i32,
    pub items: i32,
    /// The sharpest of its faces, for the card.
    pub cover: Option<FaceBox>,
    pub hidden: bool,
}

/// A face as the detector found it, in the image's pixels.
pub struct Found {
    pub rect: [f32; 4],
    pub score: f32,
    pub sharpness: f32,
    pub vec: Option<Vec<f32>>,
}

/// Store the faces of `asset` (replacing earlier ones) and mark it scanned.
pub fn store(c: &Connection, asset: i64, width: u32, height: u32, found: &[Found]) -> rusqlite::Result<()> {
    c.execute("DELETE FROM faces WHERE asset_id = ?1", [asset])?;
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let mut ins =
        c.prepare_cached("INSERT INTO faces (asset_id, x, y, w, h, score, sharpness, vec) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)")?;
    for f in found {
        ins.execute(params![
            asset,
            f.rect[0] / w,
            f.rect[1] / h,
            f.rect[2] / w,
            f.rect[3] / h,
            f.score,
            f.sharpness,
            f.vec.as_deref().map(vectors::to_blob)
        ])?;
    }
    c.execute("INSERT OR REPLACE INTO face_scans (asset_id, model, at) VALUES (?1, ?2, ?3)", params![asset, MODEL, now_ms()])?;
    Ok(())
}

/// Photos that were not looked at for faces yet, newest first.
pub fn missing(c: &Connection) -> rusqlite::Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT a.id FROM assets a LEFT JOIN categories cat ON cat.id = a.category_id
         WHERE a.missing = 0 AND a.trashed_at IS NULL AND a.kind = 0 AND (a.group_role IS NULL OR a.group_role = 'primary')
           AND a.screenshot = 0 AND coalesce(cat.key, '') NOT IN ('screenshots', 'documents', 'wallpapers')
           AND NOT EXISTS (SELECT 1 FROM face_scans s WHERE s.asset_id = a.id AND s.model = ?1)
         ORDER BY a.taken_local DESC, a.id DESC",
    )?;
    st.query_map([MODEL], |r| r.get(0))?.collect()
}

struct Row {
    id: i64,
    asset: i64,
    person: Option<i64>,
    vec: Vec<f32>,
}

fn unit(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[derive(Debug, Default, PartialEq)]
pub struct Grouped {
    /// Faces that joined a person.
    pub joined: usize,
    /// New (unnamed) people.
    pub created: usize,
    /// Items whose named faces changed (their files need the names).
    pub named_items: Vec<i64>,
}

/// Group faces that belong to nobody yet.
pub fn group(c: &Connection) -> rusqlite::Result<Grouped> {
    let rows: Vec<Row> = {
        let mut st = c.prepare(
            "SELECT f.id, f.asset_id, f.person_id, f.vec FROM faces f JOIN assets a ON a.id = f.asset_id
             WHERE f.vec IS NOT NULL AND a.trashed_at IS NULL AND a.missing = 0 ORDER BY f.sharpness DESC, f.id",
        )?;
        st.query_map([], |r| {
            Ok(Row { id: r.get(0)?, asset: r.get(1)?, person: r.get(2)?, vec: vectors::from_blob(&r.get::<_, Vec<u8>>(3)?) })
        })?
        .collect::<Result<_, _>>()?
    };
    let rejects: HashSet<(i64, i64)> = {
        let mut st = c.prepare("SELECT face_id, person_id FROM face_rejects")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    // People as they are: the mean of their faces and the items they are in.
    let mut centres: HashMap<i64, (Vec<f32>, HashSet<i64>)> = HashMap::new();
    for r in rows.iter().filter(|r| r.person.is_some()) {
        let e = centres.entry(r.person.unwrap()).or_insert_with(|| (vec![0.0; r.vec.len()], HashSet::new()));
        e.0.iter_mut().zip(&r.vec).for_each(|(a, b)| *a += b);
        e.1.insert(r.asset);
    }
    centres.values_mut().for_each(|(v, _)| unit(v));
    let mut out = Grouped::default();
    let mut assign: Vec<(i64, i64)> = Vec::new();
    let mut rest: Vec<&Row> = Vec::new();
    for r in rows.iter().filter(|r| r.person.is_none()) {
        let best = centres
            .iter()
            .filter(|(p, (_, items))| !items.contains(&r.asset) && !rejects.contains(&(r.id, **p)))
            .map(|(p, (v, _))| (*p, dot(v, &r.vec)))
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
        match best {
            Some((p, s)) if s >= JOIN => {
                assign.push((r.id, p));
                centres.get_mut(&p).unwrap().1.insert(r.asset);
            }
            _ => rest.push(r),
        }
    }
    // The rest, greedily: each face joins the most alike new group.
    struct Group<'a> {
        sum: Vec<f32>,
        centre: Vec<f32>,
        items: HashSet<i64>,
        faces: Vec<&'a Row>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for r in rest {
        let best = groups
            .iter_mut()
            .filter(|g| !g.items.contains(&r.asset))
            .map(|g| {
                let s = dot(&g.centre, &r.vec);
                (g, s)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((g, s)) if s >= SEED => {
                g.sum.iter_mut().zip(&r.vec).for_each(|(a, b)| *a += b);
                g.centre = g.sum.clone();
                unit(&mut g.centre);
                g.items.insert(r.asset);
                g.faces.push(r);
            }
            _ => groups.push(Group { sum: r.vec.clone(), centre: r.vec.clone(), items: HashSet::from([r.asset]), faces: vec![r] }),
        }
    }
    for g in groups.iter().filter(|g| g.faces.len() >= MIN_PERSON) {
        c.execute("INSERT INTO people (created_at) VALUES (?1)", [now_ms()])?;
        let p = c.last_insert_rowid();
        out.created += 1;
        assign.extend(g.faces.iter().map(|f| (f.id, p)));
    }
    let named: HashSet<i64> = {
        let mut st = c.prepare("SELECT id FROM people WHERE name IS NOT NULL")?;
        st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    let mut up = c.prepare_cached("UPDATE faces SET person_id = ?2, person_source = 'auto' WHERE id = ?1 AND person_id IS NULL")?;
    let asset_of: HashMap<i64, i64> = rows.iter().map(|r| (r.id, r.asset)).collect();
    for (face, p) in &assign {
        up.execute(params![face, p])?;
        out.joined += 1;
        if named.contains(p) && !out.named_items.contains(&asset_of[face]) {
            out.named_items.push(asset_of[face]);
        }
    }
    Ok(out)
}

fn face_row(r: &rusqlite::Row) -> rusqlite::Result<FaceBox> {
    Ok(FaceBox {
        id: r.get::<_, i64>(0)? as i32,
        asset: r.get::<_, i64>(1)? as i32,
        x: r.get(2)?,
        y: r.get(3)?,
        w: r.get(4)?,
        h: r.get(5)?,
        person: r.get::<_, Option<i64>>(6)?.map(|p| p as i32),
        ratio: {
            let (w, h): (Option<f64>, Option<f64>) = (r.get(7)?, r.get(8)?);
            match (w, h) {
                (Some(w), Some(h)) if w > 0.0 && h > 0.0 => (w / h) as f32,
                _ => 1.0,
            }
        },
    })
}

const FACE_COLS: &str = "f.id, f.asset_id, f.x, f.y, f.w, f.h, f.person_id, a.width, a.height";
/// Faces that count: in items still in the library.
const LIVE: &str = "a.id = f.asset_id AND a.trashed_at IS NULL AND a.missing = 0";

/// Everyone with at least one face, named people first, then by faces.
pub fn list(c: &Connection) -> rusqlite::Result<Vec<Person>> {
    let mut st = c.prepare(&format!(
        "SELECT p.id, p.name, p.hidden, count(f.id), count(DISTINCT f.asset_id) FROM people p
         JOIN faces f ON f.person_id = p.id JOIN assets a ON {LIVE}
         GROUP BY p.id ORDER BY p.name IS NULL, count(f.id) DESC, p.id"
    ))?;
    let people: Vec<(i64, Option<String>, bool, i64, i64)> =
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<Result<_, _>>()?;
    let mut cover = c.prepare_cached(&format!(
        "SELECT {FACE_COLS} FROM faces f JOIN assets a ON {LIVE} WHERE f.person_id = ?1
         ORDER BY f.person_source = 'owner' DESC, f.sharpness * min(f.w, 0.3) DESC LIMIT 1"
    ))?;
    people
        .into_iter()
        .map(|(id, name, hidden, faces, items)| {
            Ok(Person {
                id: id as i32,
                name,
                faces: faces as i32,
                items: items as i32,
                cover: cover.query_row([id], face_row).optional()?,
                hidden,
            })
        })
        .collect()
}

pub fn faces_of_person(c: &Connection, person: i64) -> rusqlite::Result<Vec<FaceBox>> {
    let mut st = c.prepare(&format!(
        "SELECT {FACE_COLS} FROM faces f JOIN assets a ON {LIVE} WHERE f.person_id = ?1 ORDER BY a.taken_local DESC, f.id"
    ))?;
    st.query_map([person], face_row)?.collect()
}

pub fn faces_of_asset(c: &Connection, asset: i64) -> rusqlite::Result<Vec<FaceBox>> {
    let mut st =
        c.prepare(&format!("SELECT {FACE_COLS} FROM faces f JOIN assets a ON a.id = f.asset_id WHERE f.asset_id = ?1 ORDER BY f.x"))?;
    st.query_map([asset], face_row)?.collect()
}

/// Items with faces of `person`.
pub fn items_of(c: &Connection, person: i64) -> rusqlite::Result<Vec<i64>> {
    let mut st = c.prepare("SELECT DISTINCT asset_id FROM faces WHERE person_id = ?1")?;
    st.query_map([person], |r| r.get(0))?.collect()
}

/// The `people` field of an item for history: each face's person, how it
/// got there, and that person's name.
pub fn read(c: &Connection, asset: i64) -> rusqlite::Result<Value> {
    let mut st = c.prepare_cached(
        "SELECT f.id, f.person_id, f.person_source, p.name FROM faces f LEFT JOIN people p ON p.id = f.person_id
         WHERE f.asset_id = ?1 ORDER BY f.id",
    )?;
    let rows: Vec<Value> = st
        .query_map([asset], |r| {
            Ok(json!({"f": r.get::<_, i64>(0)?, "p": r.get::<_, Option<i64>>(1)?, "s": r.get::<_, Option<String>>(2)?, "n": r.get::<_, Option<String>>(3)?}))
        })?
        .collect::<Result<_, _>>()?;
    Ok(Value::Array(rows))
}

/// Put the `people` field back: faces to their people, names on people.
pub fn write(c: &Connection, v: &Value) -> rusqlite::Result<()> {
    for e in v.as_array().into_iter().flatten() {
        let Some(face) = e.get("f").and_then(Value::as_i64) else { continue };
        let person = e.get("p").and_then(Value::as_i64);
        c.execute(
            "UPDATE faces SET person_id = ?2, person_source = ?3 WHERE id = ?1",
            params![face, person, e.get("s").and_then(Value::as_str)],
        )?;
        if let Some(p) = person {
            let name = e.get("n").and_then(Value::as_str).filter(|n| !n.trim().is_empty());
            c.execute("UPDATE people SET name = ?2, merged_into = NULL WHERE id = ?1", params![p, name])?;
            if let Some(n) = name {
                c.execute("INSERT OR IGNORE INTO people_names (name) VALUES (?1)", [n])?;
            }
        }
    }
    Ok(())
}

/// The value of the `people` field after `change` is applied to each face.
pub fn changed(c: &Connection, asset: i64, change: impl Fn(&mut serde_json::Map<String, Value>)) -> rusqlite::Result<Value> {
    let mut v = read(c, asset)?;
    for e in v.as_array_mut().into_iter().flatten() {
        if let Some(m) = e.as_object_mut() {
            change(m);
        }
    }
    Ok(v)
}

pub fn name_of(c: &Connection, person: i64) -> rusqlite::Result<Option<String>> {
    Ok(c.query_row("SELECT name FROM people WHERE id = ?1", [person], |r| r.get(0)).optional()?.flatten())
}

pub fn reject(c: &Connection, face: i64, person: i64) -> rusqlite::Result<()> {
    c.execute("INSERT OR IGNORE INTO face_rejects (face_id, person_id) VALUES (?1, ?2)", params![face, person])?;
    Ok(())
}

pub fn set_hidden(c: &Connection, person: i64, hidden: bool) -> rusqlite::Result<()> {
    c.execute("UPDATE people SET hidden = ?2 WHERE id = ?1", params![person, hidden])?;
    Ok(())
}

/// Named faces of an item, for the file: (name, x, y, w, h) with the box
/// as left, top, width, height 0–1 of the displayed image.
pub fn named_regions(c: &Connection, asset: i64) -> rusqlite::Result<Vec<(String, [f32; 4])>> {
    let mut st = c.prepare_cached(
        "SELECT p.name, f.x, f.y, f.w, f.h FROM faces f JOIN people p ON p.id = f.person_id
         WHERE f.asset_id = ?1 AND p.name IS NOT NULL ORDER BY f.x",
    )?;
    st.query_map([asset], |r| Ok((r.get(0)?, [r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?])))?.collect()
}

/// Every name Roll ever gave.
pub fn names_ever(c: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare_cached("SELECT name FROM people_names ORDER BY name")?;
    st.query_map([], |r| r.get(0))?.collect()
}

/// Names of the people in an item, for search.
pub fn names_in(c: &Connection, asset: i64) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare_cached(
        "SELECT DISTINCT p.name FROM faces f JOIN people p ON p.id = f.person_id WHERE f.asset_id = ?1 AND p.name IS NOT NULL",
    )?;
    st.query_map([asset], |r| r.get(0))?.collect()
}

// The owner's changes: one undoable batch each.

/// Change the `people` field of `assets`. Every old value is read first: a
/// person's name shows in all of their items at once.
fn change_all(
    c: &Connection,
    batch: i64,
    assets: &[i64],
    why: &str,
    f: impl Fn(&mut serde_json::Map<String, Value>),
) -> rusqlite::Result<()> {
    let mut plan = Vec::new();
    for a in assets {
        if !plan.iter().any(|(x, _, _)| x == a) {
            plan.push((*a, read(c, *a)?, changed(c, *a, &f)?));
        }
    }
    for (a, old, value) in plan {
        let ch =
            crate::edits::Change { field: "people", value, source: "manual", source_ref: None, conf: None, why: Some(why.to_string()) };
        crate::edits::set_from(c, Some(batch), a, old, &ch)?;
    }
    Ok(())
}

fn is(m: &serde_json::Map<String, Value>, key: &str, id: i64) -> bool {
    m.get(key).and_then(Value::as_i64) == Some(id)
}

/// Name (or rename) a person; `None` takes the name away.
pub fn name(c: &Connection, person: i64, name: Option<&str>) -> rusqlite::Result<Vec<i64>> {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    let label = match name {
        Some(n) => format!("Named {n}"),
        None => "Took a name away".into(),
    };
    let batch = crate::edits::begin_batch(c, "edit", &label)?;
    let items = items_of(c, person)?;
    change_all(c, batch, &items, &format!("{label}, by you."), |m| {
        if is(m, "p", person) {
            m.insert("n".into(), json!(name));
        }
    })?;
    Ok(items)
}

/// Make several people one: their faces go to `into`, which keeps its
/// name, or takes the first name among the others.
pub fn merge(c: &Connection, into: i64, others: &[i64]) -> rusqlite::Result<Vec<i64>> {
    let name = name_of(c, into)?.or(others.iter().find_map(|o| name_of(c, *o).ok().flatten()));
    let label = format!("Merged {} people{}", others.len() + 1, name.as_ref().map(|n| format!(" into {n}")).unwrap_or_default());
    let batch = crate::edits::begin_batch(c, "edit", &label)?;
    let others: Vec<i64> = others.iter().copied().filter(|o| *o != into).collect();
    let mut items = items_of(c, into)?;
    for o in &others {
        items.extend(items_of(c, *o)?);
    }
    change_all(c, batch, &items, &format!("{label}, by you."), |m| {
        let p = m.get("p").and_then(Value::as_i64);
        if p == Some(into) {
            m.insert("n".into(), json!(name));
        } else if p.is_some_and(|p| others.contains(&p)) {
            m.insert("p".into(), json!(into));
            m.insert("n".into(), json!(name));
            m.insert("s".into(), json!("owner"));
        }
    })?;
    for o in &others {
        c.execute("UPDATE people SET merged_into = ?2 WHERE id = ?1", params![o, into])?;
    }
    Ok(items)
}

/// "Not this person": the faces leave their person and stay out of it.
pub fn not_person(c: &Connection, faces: &[i64]) -> rusqlite::Result<Vec<i64>> {
    let batch = crate::edits::begin_batch(c, "edit", "Not this person")?;
    let mut items = Vec::new();
    for f in faces {
        let row: Option<(i64, Option<i64>)> =
            c.query_row("SELECT asset_id, person_id FROM faces WHERE id = ?1", [f], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        if let Some((asset, Some(person))) = row {
            reject(c, *f, person)?;
            items.push(asset);
        }
    }
    change_all(c, batch, &items, "Not this person, said by you.", |m| {
        if m.get("f").and_then(Value::as_i64).is_some_and(|f| faces.contains(&f)) {
            m.insert("p".into(), Value::Null);
            m.insert("n".into(), Value::Null);
            m.insert("s".into(), json!("owner"));
        }
    })?;
    Ok(items)
}

/// Put faces with a person: an existing one, or a new one by name.
pub fn assign(c: &Connection, faces: &[i64], person: Option<i64>, new_name: Option<&str>) -> rusqlite::Result<Vec<i64>> {
    let person = match person {
        Some(p) => p,
        None => {
            c.execute("INSERT INTO people (created_at) VALUES (?1)", [now_ms()])?;
            c.last_insert_rowid()
        }
    };
    let name = new_name.map(str::trim).filter(|n| !n.is_empty()).map(String::from).or(name_of(c, person)?);
    let label = match &name {
        Some(n) => format!("This is {n}"),
        None => "Grouped faces".into(),
    };
    let batch = crate::edits::begin_batch(c, "edit", &label)?;
    let mut items = Vec::new();
    for f in faces {
        if let Some(asset) = c.query_row("SELECT asset_id FROM faces WHERE id = ?1", [f], |r| r.get::<_, i64>(0)).optional()? {
            c.execute("DELETE FROM face_rejects WHERE face_id = ?1 AND person_id = ?2", params![f, person])?;
            items.push(asset);
        }
    }
    // A name shows in every item of the person.
    let mut all = items.clone();
    if name != name_of(c, person)? {
        all.extend(items_of(c, person)?);
    }
    change_all(c, batch, &all, &format!("{label}, said by you."), |m| {
        if m.get("f").and_then(Value::as_i64).is_some_and(|f| faces.contains(&f)) {
            m.insert("p".into(), json!(person));
            m.insert("s".into(), json!("owner"));
        }
        if is(m, "p", person) {
            m.insert("n".into(), json!(name));
        }
    })?;
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    fn face(v: &[f32]) -> Found {
        let mut v = v.to_vec();
        unit(&mut v);
        Found { rect: [10.0, 10.0, 50.0, 50.0], score: 0.9, sharpness: 100.0, vec: Some(v) }
    }

    #[test]
    fn groups_alike_faces_and_keeps_one_item_apart() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let ids: Vec<i64> = (0..5).map(|i| insert_asset(&c, &format!("{i}.jpg"))).collect();
        // Anna in items 0–3, twice in item 3 (a mirror) and Ben once.
        let anna = [1.0, 0.1, 0.0];
        let ben = [0.0, 1.0, 0.2];
        store(&c, ids[0], 100, 100, &[face(&anna), face(&ben)]).unwrap();
        store(&c, ids[1], 100, 100, &[face(&[0.95, 0.15, 0.05])]).unwrap();
        store(&c, ids[2], 100, 100, &[face(&[1.0, 0.0, 0.1])]).unwrap();
        store(&c, ids[3], 100, 100, &[face(&anna), face(&[0.98, 0.12, 0.0])]).unwrap();
        let g = group(&c).unwrap();
        assert_eq!(g.created, 1, "Ben appears once, no person");
        let people = list(&c).unwrap();
        assert_eq!(people.len(), 1);
        assert_eq!((people[0].faces, people[0].items), (4, 4), "one face per item");
        // A later Anna joins her.
        store(&c, ids[4], 100, 100, &[face(&[0.9, 0.2, 0.0])]).unwrap();
        let g = group(&c).unwrap();
        assert_eq!((g.joined, g.created), (1, 0));
        assert_eq!(missing(&c).unwrap(), Vec::<i64>::new());
    }

    #[test]
    fn history_value_round_trips_names() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let a = insert_asset(&c, "a.jpg");
        store(&c, a, 200, 100, &[face(&[1.0, 0.0])]).unwrap();
        c.execute("INSERT INTO people (created_at) VALUES (0)", []).unwrap();
        let before = read(&c, a).unwrap();
        let after = changed(&c, a, |m| {
            m.insert("p".into(), json!(1));
            m.insert("n".into(), json!("Anna"));
            m.insert("s".into(), json!("owner"));
        })
        .unwrap();
        write(&c, &after).unwrap();
        assert_eq!(named_regions(&c, a).unwrap(), vec![("Anna".to_string(), [0.05, 0.1, 0.25, 0.5])]);
        assert_eq!(names_in(&c, a).unwrap(), vec!["Anna"]);
        write(&c, &before).unwrap();
        assert!(named_regions(&c, a).unwrap().is_empty());
    }

    #[test]
    fn naming_merging_and_not_this_person_undo() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let ids: Vec<i64> = (0..4).map(|i| insert_asset(&c, &format!("{i}.jpg"))).collect();
        for (i, a) in ids.iter().enumerate() {
            store(&c, *a, 100, 100, &[face(&[1.0, 0.05 * i as f32, 0.0])]).unwrap();
        }
        c.execute("INSERT INTO people (created_at) VALUES (0), (0)", []).unwrap();
        c.execute("UPDATE faces SET person_id = CASE WHEN asset_id <= ?1 THEN 1 ELSE 2 END, person_source = 'auto'", [ids[1]]).unwrap();
        let named = name(&c, 1, Some(" Anna ")).unwrap();
        assert_eq!(named.len(), 2);
        assert_eq!(name_of(&c, 1).unwrap().as_deref(), Some("Anna"));
        let dirty: i64 = c.query_row("SELECT count(*) FROM assets WHERE meta_dirty = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(dirty, 2, "named items need their files written");
        merge(&c, 1, &[2]).unwrap();
        let people = list(&c).unwrap();
        assert_eq!((people.len(), people[0].faces, people[0].name.as_deref()), (1, 4, Some("Anna")));
        let f: i64 = c.query_row("SELECT id FROM faces WHERE asset_id = ?1", [ids[3]], |r| r.get(0)).unwrap();
        not_person(&c, &[f]).unwrap();
        assert_eq!(list(&c).unwrap()[0].faces, 3);
        // Grouping leaves the rejected face out of Anna.
        group(&c).unwrap();
        assert_eq!(list(&c).unwrap()[0].faces, 3);
        // Undo, newest first: the face, the merge, the name.
        let batches: Vec<i64> = {
            let mut st = c.prepare("SELECT id FROM batches ORDER BY id DESC").unwrap();
            st.query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
        };
        for b in batches {
            crate::edits::undo_batch(&c, b).unwrap();
        }
        let people = list(&c).unwrap();
        assert_eq!(people.iter().map(|p| (p.faces, p.name.clone())).collect::<Vec<_>>(), vec![(2, None), (2, None)]);
    }
}
