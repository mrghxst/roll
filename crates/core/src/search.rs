//! Search: a deterministic parser turns the words it knows (years, months,
//! seasons, media types, categories, places, cameras, "no location") into
//! filters; the rest is matched with SQLite FTS5 against names, folders,
//! tags, place names and descriptions. Semantic matching joins with M4's
//! embeddings.

use chrono::{Datelike, Months, NaiveDate};
use roll_geo::gazetteer::{Gazetteer, Kind, fold};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::{Query, problem};

/// An FTS5 query for free text: every word must match as a prefix.
pub fn fts_match(text: &str) -> Option<String> {
    let words: Vec<String> = fold(text).split(' ').filter(|w| !w.is_empty()).map(|w| format!("\"{w}\"*")).collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// id, name, folder, lat, lon, description, camera, tags.
type IndexRow = (i64, String, String, Option<f64>, Option<f64>, String, String, Option<String>);

/// Rebuild the index rows of `ids`, or of every item.
pub fn reindex(c: &Connection, ids: Option<&[i64]>, place: &dyn Fn(f64, f64) -> Option<String>) -> rusqlite::Result<usize> {
    let filter = match ids {
        Some([]) => return Ok(0),
        Some(ids) => format!("WHERE a.id IN ({})", ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")),
        None => String::new(),
    };
    match ids {
        Some(ids) => {
            let list = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
            c.execute(&format!("DELETE FROM fts WHERE rowid IN ({list})"), [])?;
        }
        None => {
            c.execute("DELETE FROM fts", [])?;
        }
    }
    let mut st = c.prepare(&format!(
        "SELECT a.id, a.name, a.dir, a.lat, a.lon, coalesce(a.description, ''), coalesce(a.make, '') || ' ' || coalesce(a.model, ''),
                (SELECT group_concat(t.path, ' ') FROM asset_tags at JOIN tags t ON t.id = at.tag_id WHERE at.asset_id = a.id)
         FROM assets a {filter}"
    ))?;
    let rows: Vec<IndexRow> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))?
        .collect::<Result<_, _>>()?;
    let mut ins = c.prepare("INSERT INTO fts (rowid, name, folder, tags, place, body) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
    let spaced = |s: &str| s.replace(['_', '-', '.', '/', '\\'], " ");
    for (id, name, dir, lat, lon, desc, camera, tags) in &rows {
        let place = lat.zip(*lon).and_then(|(la, lo)| place(la, lo)).unwrap_or_default();
        ins.execute(params![id, spaced(name), spaced(dir), spaced(tags.as_deref().unwrap_or("")), place, format!("{desc} {camera}")])?;
    }
    Ok(rows.len())
}

/// What the parser understood, for chips the owner can edit.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ParsedSearch {
    /// Only the fields the parser set.
    pub query: Query,
    /// Words left for full-text search.
    pub rest: String,
    /// Plain-language labels for what was understood, in order.
    pub understood: Vec<String>,
}

const MONTHS: [&str; 12] =
    ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];

fn month_of(w: &str) -> Option<u32> {
    if w.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| m.starts_with(w) && (w.len() >= 3)).map(|i| i as u32 + 1)
}

fn local(d: NaiveDate) -> String {
    format!("{}T00:00:00", d.format("%Y-%m-%d"))
}

/// Parse search text. `today` anchors "last year" and friends.
pub fn parse(c: &Connection, text: &str, gaz: Option<&Gazetteer>, today: NaiveDate) -> ParsedSearch {
    let mut q = Query::default();
    let mut understood = Vec::new();
    let folded = fold(text);
    let words: Vec<&str> = folded.split(' ').filter(|w| !w.is_empty()).collect();
    let mut used = vec![false; words.len()];
    let mut range = |q: &mut Query, from: NaiveDate, to: NaiveDate, label: String| {
        q.from_local = Some(local(from));
        q.to_local = Some(local(to));
        understood.push(label);
    };
    let year_of = |w: &str| w.parse::<i32>().ok().filter(|y| (1900..=today.year() + 1).contains(y) && w.len() == 4);

    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        let next = words.get(i + 1).copied();
        // "summer 2021", "august 2019", "aug 2019"
        if let Some(y) = next.and_then(year_of) {
            let season = match w {
                "spring" => Some((3, 3, "Spring")),
                "summer" => Some((6, 3, "Summer")),
                "autumn" | "fall" => Some((9, 3, "Autumn")),
                "winter" => Some((12, 3, "Winter")),
                _ => None,
            };
            if let Some((m, len, name)) = season {
                let from = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
                range(&mut q, from, from + Months::new(len), format!("{name} {y}"));
                used[i] = true;
                used[i + 1] = true;
                i += 2;
                continue;
            }
            if let Some(m) = month_of(w) {
                let from = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
                let name = MONTHS[m as usize - 1];
                range(&mut q, from, from + Months::new(1), format!("{}{} {y}", name[..1].to_uppercase(), &name[1..]));
                used[i] = true;
                used[i + 1] = true;
                i += 2;
                continue;
            }
        }
        // "last year", "this year", "last month"
        if let (Some(a), Some(b)) = (Some(w), next) {
            let r = match (a, b) {
                ("last", "year") => Some((NaiveDate::from_ymd_opt(today.year() - 1, 1, 1).unwrap(), 12, "Last year")),
                ("this", "year") => Some((NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap(), 12, "This year")),
                ("last", "month") => {
                    Some((NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap() - Months::new(1), 1, "Last month"))
                }
                ("this", "month") => Some((NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap(), 1, "This month")),
                _ => None,
            };
            if let Some((from, len, label)) = r {
                range(&mut q, from, from + Months::new(len), label.to_string());
                used[i] = true;
                used[i + 1] = true;
                i += 2;
                continue;
            }
        }
        // "2019", "2019 08" (from 2019-08)
        if let Some(y) = year_of(w) {
            if let Some(m) = next.and_then(|n| n.parse::<u32>().ok()).filter(|m| (1..=12).contains(m) && next.is_some_and(|n| n.len() == 2))
            {
                let from = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
                range(&mut q, from, from + Months::new(1), format!("{y}-{m:02}"));
                used[i] = true;
                used[i + 1] = true;
                i += 2;
                continue;
            }
            let from = NaiveDate::from_ymd_opt(y, 1, 1).unwrap();
            range(&mut q, from, from + Months::new(12), y.to_string());
            used[i] = true;
        }
        i += 1;
    }

    // Phrases: "no location", "without date", "without screenshots".
    let categories: Vec<(String, String)> = c
        .prepare("SELECT key, name FROM categories")
        .and_then(|mut st| st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect())
        .unwrap_or_default();
    let category = |w: &str| -> Option<(String, String)> {
        let w = w.trim_start_matches('#');
        categories
            .iter()
            .find(|(k, n)| {
                let n = fold(n);
                *k == w || n == w || format!("{n}s") == w || n.strip_suffix('s') == Some(w) || k.replace('_', " ") == w
            })
            .cloned()
    };
    let mut i = 0;
    while i < words.len() {
        if used[i] {
            i += 1;
            continue;
        }
        let w = words[i];
        let next = words.get(i + 1).copied().filter(|_| !used.get(i + 1).copied().unwrap_or(true));
        let negate = matches!(w, "no" | "without" | "not" | "except");
        if negate && let Some(n) = next {
            match n {
                "location" | "place" | "gps" | "places" | "locations" => {
                    q.location = Some("none".into());
                    understood.push("No location".into());
                }
                "date" | "dates" => {
                    q.problems = Some(problem::NO_DATE as i32);
                    understood.push("No date".into());
                }
                n => match category(n) {
                    Some((k, name)) if k != "photos" => {
                        q.exclude_categories.push(k);
                        understood.push(format!("Without {name}"));
                    }
                    _ => {
                        i += 1;
                        continue;
                    }
                },
            }
            used[i] = true;
            used[i + 1] = true;
            i += 2;
            continue;
        }
        let media = match w {
            "video" | "videos" | "movie" | "movies" | "clip" | "clips" => Some(("video", "Videos")),
            "photo" | "photos" | "picture" | "pictures" | "pics" => Some(("photo", "Photos")),
            "live" => Some(("live", "Live Photos")),
            "raw" | "raws" => Some(("raw", "RAW")),
            "burst" | "bursts" => Some(("burst", "Bursts")),
            _ => None,
        };
        if let Some((m, label)) = media {
            q.media.push(m.into());
            understood.push(label.into());
            used[i] = true;
            if m == "live" && next.is_some_and(|n| n.starts_with("photo")) {
                used[i + 1] = true;
                i += 1;
            }
        } else if matches!(w, "favorite" | "favorites" | "favourite" | "favourites") {
            q.favorite = Some(true);
            understood.push("Favorites".into());
            used[i] = true;
        } else if let Some((k, name)) = category(w).filter(|(k, _)| k != "photos") {
            q.categories.push(k);
            understood.push(name);
            used[i] = true;
        }
        i += 1;
    }

    // Cameras by model name ("iphone 12", "d750").
    let rest_text: String = words.iter().zip(&used).filter(|(_, u)| !**u).map(|(w, _)| *w).collect::<Vec<_>>().join(" ");
    if !rest_text.is_empty() {
        let models: Vec<String> = c
            .prepare("SELECT DISTINCT model FROM assets WHERE model IS NOT NULL")
            .and_then(|mut st| st.query_map([], |r| r.get::<_, String>(0))?.collect())
            .unwrap_or_default();
        let padded = format!(" {rest_text} ");
        let mut best: Option<String> = None;
        for m in models {
            let f = fold(&m);
            let short = f.split(' ').filter(|p| p.chars().any(|ch| ch.is_ascii_digit())).collect::<Vec<_>>().join(" ");
            for cand in [f.as_str(), short.as_str()] {
                if cand.len() >= 3 && padded.contains(&format!(" {cand} ")) && best.as_ref().is_none_or(|b| b.len() < cand.len()) {
                    best = Some(m.clone());
                }
            }
        }
        if let Some(m) = best {
            let f = fold(&m);
            let short = f.split(' ').filter(|p| p.chars().any(|ch| ch.is_ascii_digit())).collect::<Vec<_>>().join(" ");
            for (k, w) in words.iter().enumerate() {
                if !used[k] && (f.split(' ').any(|p| p == *w) || short.split(' ').any(|p| p == *w)) {
                    used[k] = true;
                }
            }
            understood.push(m.clone());
            q.cameras.push(m);
        }
    }

    // A place named in what is left.
    if let Some(g) = gaz {
        let rest: Vec<(usize, &str)> = words.iter().enumerate().filter(|(k, _)| !used[*k]).map(|(k, w)| (k, *w)).collect();
        let text: String = rest.iter().map(|(_, w)| *w).collect::<Vec<_>>().join(" ");
        if let Some(hit) = g.in_text(&text) {
            let r_km = match hit.kind {
                Kind::City => 15.0,
                Kind::Region => 120.0,
                Kind::Country => 450.0,
            };
            let dlat = r_km / 111.0;
            let dlon = r_km / (111.0 * hit.lat.to_radians().cos().max(0.05));
            q.bbox = Some(vec![hit.lat - dlat, hit.lon - dlon, hit.lat + dlat, hit.lon + dlon]);
            let name = fold(&hit.name);
            for (k, w) in &rest {
                if name.split(' ').any(|p| p == *w) {
                    used[*k] = true;
                }
            }
            for (k, w) in &rest {
                if matches!(*w, "in" | "at" | "from" | "near") && used.get(k + 1).copied().unwrap_or(false) {
                    used[*k] = true;
                }
            }
            understood.push(format!("Near {}", hit.name));
        }
    }
    // Filler words between understood parts.
    for (k, w) in words.iter().enumerate() {
        if matches!(*w, "in" | "from" | "of" | "and" | "with" | "the" | "my" | "taken" | "at") && !used[k] {
            let neighbours = (k > 0 && used[k - 1]) || used.get(k + 1).copied().unwrap_or(false);
            if neighbours {
                used[k] = true;
            }
        }
    }
    let rest = words.iter().zip(&used).filter(|(_, u)| !**u).map(|(w, _)| *w).collect::<Vec<_>>().join(" ");
    ParsedSearch { query: q, rest, understood }
}

/// Merge parsed filters into a query, keeping the rest as text.
pub fn expand(c: &Connection, q: &Query, gaz: Option<&Gazetteer>, today: NaiveDate) -> Query {
    let Some(text) = q.text.as_deref().filter(|t| !t.trim().is_empty()) else { return q.clone() };
    let p = parse(c, text, gaz, today);
    let mut out = q.clone();
    out.text = (!p.rest.is_empty()).then_some(p.rest);
    let pq = p.query;
    out.from_local = pq.from_local.or(out.from_local);
    out.to_local = pq.to_local.or(out.to_local);
    out.media.extend(pq.media);
    out.categories.extend(pq.categories);
    out.exclude_categories.extend(pq.exclude_categories);
    out.cameras.extend(pq.cameras);
    out.location = pq.location.or(out.location);
    out.problems = pq.problems.or(out.problems);
    out.favorite = pq.favorite.or(out.favorite);
    out.bbox = pq.bbox.or(out.bbox);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 11).unwrap()
    }

    #[test]
    fn understands_dates_media_and_categories() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let p = parse(&c, "videos from summer 2019 without screenshots", None, today());
        assert_eq!(p.query.media, vec!["video"]);
        assert_eq!(p.query.from_local.as_deref(), Some("2019-06-01T00:00:00"));
        assert_eq!(p.query.to_local.as_deref(), Some("2019-09-01T00:00:00"));
        assert_eq!(p.query.exclude_categories, vec!["screenshots"]);
        assert_eq!(p.rest, "");
        assert_eq!(p.understood, vec!["Summer 2019", "Videos", "Without Screenshots"]);

        let p = parse(&c, "aug 2019 lake", None, today());
        assert_eq!(p.query.from_local.as_deref(), Some("2019-08-01T00:00:00"));
        assert_eq!(p.understood, vec!["August 2019"]);
        assert_eq!(p.rest, "lake");

        let p = parse(&c, "memes last year no location", None, today());
        assert_eq!(p.query.categories, vec!["memes"]);
        assert_eq!(p.query.from_local.as_deref(), Some("2025-01-01T00:00:00"));
        assert_eq!(p.query.location.as_deref(), Some("none"));

        let p = parse(&c, "#documents 2019 08", None, today());
        assert_eq!(p.query.categories, vec!["documents"]);
        assert_eq!(p.query.to_local.as_deref(), Some("2019-09-01T00:00:00"));
    }

    #[test]
    fn cameras_places_and_full_text() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let a = insert_asset(&c, "2019-08 Lake Garda/DSC_0001.JPG");
        c.execute("UPDATE assets SET make = 'NIKON CORPORATION', model = 'NIKON D750', lat = 45.88, lon = 10.84 WHERE id = ?1", [a])
            .unwrap();
        let b = insert_asset(&c, "Dump/IMG_1.HEIC");
        c.execute("UPDATE assets SET model = 'iPhone 12', description = 'Birthday cake' WHERE id = ?1", [b]).unwrap();
        let p = parse(&c, "d750 garda", None, today());
        assert_eq!(p.query.cameras, vec!["NIKON D750"]);
        assert_eq!(p.rest, "garda");
        let p = parse(&c, "iphone 12", None, today());
        assert_eq!(p.query.cameras, vec!["iPhone 12"]);

        reindex(&c, None, &|_, _| Some("Riva del Garda, Italy".into())).unwrap();
        let ids = |text: &str| crate::query::ids(&c, &Query { text: Some(text.into()), ..Default::default() }).unwrap();
        assert_eq!(ids("garda"), vec![a]);
        assert_eq!(ids("riva"), vec![a], "place names are searchable");
        assert_eq!(ids("birth"), vec![b], "prefixes match");
        assert_eq!(ids("dsc 0001"), vec![a]);
        assert!(ids("nothing").is_empty());
        // Accents fold: "Zürich" finds "zurich".
        assert_eq!(fts_match("Zürich").as_deref(), Some("\"zurich\"*"));
    }
}
