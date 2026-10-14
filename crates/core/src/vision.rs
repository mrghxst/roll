//! Local image understanding (PROMPT 4.3 and 5.1, M4). Every photo and
//! video gets a SigLIP 2 embedding from its 256 px thumbnail. Undecided
//! items then get a category from two signals:
//!
//! - zero-shot: how close the item is to short descriptions of each
//!   category (the best-matching description counts);
//! - the owner's own decisions: the categories of the most similar items
//!   the owner has sorted (k nearest neighbours).
//!
//! The confidence is calibrated against the CC0 test set (docs/benchmarks.md);
//! an item is sorted on its own only at the category's threshold (97% by
//! default), otherwise it waits in Review with the reason. Owner decisions
//! (review 3) and rule decisions (review 2) are never changed here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use roll_ai::siglip::{self, ImageEncoder, TextEncoder};
use roll_ai::vectors::{self, Index};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db::now_ms;
use crate::edits::{self, Change};

pub use roll_ai::siglip::MODEL_ID;

/// Built-in categories: descriptions written for what the category looks
/// like, not only what it shows. Own categories use their name and
/// definition.
const PROMPTS: &[(&str, &[&str])] = &[
    (
        "photos",
        &[
            "a photo",
            "a photo of people",
            "a photo of a dog",
            "a photo of a cat",
            "a photo of food on a plate",
            "a landscape photo",
            "a photo of a city street",
            "a photo of the beach",
            "a photo of mountains",
            "a close-up photo of a flower",
            "an old photograph",
        ],
    ),
    (
        "screenshots",
        &[
            "a screenshot of a phone screen",
            "a screenshot of an app",
            "a screenshot of a chat conversation",
            "a screenshot of a website",
            "a screenshot of settings",
        ],
    ),
    ("memes", &["a meme with caption text", "an internet meme", "a funny image with white text on top and bottom"]),
    ("social_media", &["a social media post", "an instagram post with likes and comments", "a saved post from a social app"]),
    (
        "wallpapers",
        &[
            "a wallpaper",
            "an abstract background",
            "a desktop wallpaper",
            "a space nebula",
            "a seamless texture pattern",
            "a gradient background",
        ],
    ),
    ("documents", &["a photo of a document", "a scanned paper document", "a receipt", "a page of printed text"]),
    (
        "junk",
        &[
            "an accidental photo of the floor",
            "a completely black photo",
            "an extremely blurry photo with nothing in it",
            "a photo taken inside a pocket",
            "a photo of a finger covering the lens",
        ],
    ),
];

/// The descriptions for a category.
pub fn prompts(key: &str, name: &str, definition: &str) -> Vec<String> {
    if let Some((_, p)) = PROMPTS.iter().find(|(k, _)| *k == key) {
        return p.iter().map(|s| s.to_string()).collect();
    }
    let mut out = vec![format!("a photo of {}", name.to_lowercase())];
    let d = definition.trim();
    if !d.is_empty() {
        out.push(d.chars().take(200).collect());
    }
    out
}

/// Probability that the zero-shot category is right, from the margin
/// between the best and the second-best category. Measured on the CC0 set
/// (157 items, embeddings from 256 px thumbnails as in the library): items
/// with margins of 0.020 and more were right about 97–98% of the time.
pub fn zero_shot_conf(margin: f32) -> f32 {
    const PTS: &[(f32, f32)] = &[(0.0, 0.40), (0.008, 0.80), (0.014, 0.93), (0.020, 0.97), (0.035, 0.99)];
    if margin <= 0.0 {
        return PTS[0].1;
    }
    for w in PTS.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if margin <= x1 {
            return y0 + (y1 - y0) * (margin - x0) / (x1 - x0);
        }
    }
    PTS[PTS.len() - 1].1
}

/// The models, loaded when first needed. The text tower (1.1 GB) only for
/// new descriptions and searches.
pub struct Vision {
    dir: PathBuf,
    gpu: bool,
    image: Mutex<Option<Arc<ImageEncoder>>>,
    text: Mutex<Option<Arc<TextEncoder>>>,
}

impl Vision {
    pub fn new(dir: PathBuf, gpu: bool) -> Self {
        Vision { dir, gpu, image: Mutex::new(None), text: Mutex::new(None) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn image(&self) -> anyhow::Result<Arc<ImageEncoder>> {
        let mut g = self.image.lock();
        if let Some(e) = g.as_ref() {
            return Ok(e.clone());
        }
        let e = Arc::new(ImageEncoder::load(&self.dir, self.gpu)?);
        tracing::info!("image model loaded (graphics card: {})", e.on_gpu());
        *g = Some(e.clone());
        Ok(e)
    }

    pub fn text(&self) -> anyhow::Result<Arc<TextEncoder>> {
        let mut g = self.text.lock();
        if let Some(e) = g.as_ref() {
            return Ok(e.clone());
        }
        let e = Arc::new(TextEncoder::load(&self.dir)?);
        *g = Some(e.clone());
        Ok(e)
    }

    /// The embedding of a text, from the cache or the text tower.
    pub fn text_vec(&self, c: &Connection, text: &str) -> anyhow::Result<Vec<f32>> {
        let key = siglip::canonical(text);
        let hit: Option<Vec<u8>> = c
            .query_row("SELECT vec FROM text_embeddings WHERE model = ?1 AND text = ?2", params![MODEL_ID, key], |r| r.get(0))
            .optional()?;
        if let Some(b) = hit {
            return Ok(vectors::from_blob(&b));
        }
        let v = self.text()?.embed(&key)?;
        c.execute(
            "INSERT OR REPLACE INTO text_embeddings (model, text, vec, at) VALUES (?1, ?2, ?3, ?4)",
            params![MODEL_ID, key, vectors::to_blob(&v), now_ms()],
        )?;
        Ok(v)
    }
}

/// Category descriptions as embeddings: key → one vector per description.
pub type Prototypes = Vec<(String, Vec<(String, Vec<f32>)>)>;

/// The descriptions of every top-level category except Unsorted.
pub fn prototypes(c: &Connection, vision: &Vision) -> anyhow::Result<Prototypes> {
    let cats: Vec<(String, String, String)> = {
        let mut st =
            c.prepare("SELECT key, name, definition FROM categories WHERE parent_id IS NULL AND key != 'unsorted' ORDER BY sort, id")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?
    };
    let mut out = Vec::new();
    for (key, name, def) in cats {
        let mut v = Vec::new();
        for p in prompts(&key, &name, &def) {
            let e = vision.text_vec(c, &p)?;
            v.push((p, e));
        }
        out.push((key, v));
    }
    Ok(out)
}

/// Best description per category for one embedding, best category first:
/// (key, score, description).
pub fn zero_shot<'a>(protos: &'a Prototypes, e: &[f32]) -> Vec<(&'a str, f32, &'a str)> {
    let mut s: Vec<(&str, f32, &str)> = protos
        .iter()
        .filter(|(_, ps)| !ps.is_empty())
        .map(|(k, ps)| {
            let (p, sc) = ps.iter().map(|(p, v)| (p.as_str(), vectors::dot(e, v))).max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
            (k.as_str(), sc, p)
        })
        .collect();
    s.sort_by(|a, b| b.1.total_cmp(&a.1));
    s
}

/// What the owner's sorted items say about an embedding: the weighted
/// vote of the nearest owner-decided items (category, share of the vote,
/// similarity of the nearest one).
pub fn neighbours(owner: &Index, labels: &HashMap<i64, String>, e: &[f32], k: usize) -> Option<(String, f32, f32)> {
    let hits = owner.search(e, k, None);
    let top = hits.first()?.1;
    let mut votes: HashMap<&str, f32> = HashMap::new();
    let mut total = 0.0;
    for (id, s) in &hits {
        let Some(l) = labels.get(id) else { continue };
        let w = s.max(0.0);
        *votes.entry(l.as_str()).or_default() += w;
        total += w;
    }
    let (best, w) = votes.into_iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
    (total > 0.0).then(|| (best.to_string(), w / total, top))
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Outcome {
    pub decided: usize,
    pub proposed: usize,
}

/// Owner decisions need this many items before they count, so a handful
/// of early clicks does not outvote the descriptions.
const MIN_OWNER: usize = 20;
const K: usize = 9;

/// Sort undecided items that have an embedding (all of them when `ids` is
/// None).
pub fn sort(c: &Connection, protos: &Prototypes, ids: Option<&[i64]>) -> rusqlite::Result<Outcome> {
    let all = crate::embeddings::load(c, MODEL_ID)?;
    // The owner's decisions (file tags count: the owner put them there).
    let labels: HashMap<i64, String> = {
        let mut st = c.prepare(
            "SELECT a.id, cat.key FROM assets a JOIN categories cat ON cat.id = a.category_id
             WHERE a.review = 3 AND cat.parent_id IS NULL AND cat.key != 'unsorted'",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    let mut owner = Index::new(siglip::DIM);
    for id in labels.keys() {
        if let Some(v) = all.get(*id) {
            owner.insert(*id, v);
        }
    }
    let use_owner = owner.len() >= MIN_OWNER;
    let info: HashMap<String, (String, f32)> = {
        let mut st = c.prepare("SELECT key, name, coalesce(auto_threshold, 0.97) FROM categories")?;
        st.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get::<_, f64>(2)? as f32))))?.collect::<Result<_, _>>()?
    };
    let rows: Vec<(i64, i64, Option<String>, Option<f64>)> = {
        let filter = match ids {
            Some([]) => return Ok(Outcome::default()),
            Some(ids) => format!("AND a.id IN ({})", ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")),
            None => String::new(),
        };
        let mut st = c.prepare(&format!(
            "SELECT a.id, a.review, cat.key, a.category_conf FROM assets a LEFT JOIN categories cat ON cat.id = a.category_id
             WHERE a.missing = 0 AND a.trashed_at IS NULL AND a.review < 2 {filter}"
        ))?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?
    };
    let mut out = Outcome::default();
    for (id, review, current, current_conf) in rows {
        let Some(e) = all.get(id) else { continue };
        let zs = zero_shot(protos, e);
        let Some(&(best, s1, desc)) = zs.first() else { continue };
        let margin = s1 - zs.get(1).map_or(0.0, |x| x.1);
        let mut key = best.to_string();
        let mut conf = zero_shot_conf(margin);
        let mut because = format!("it looks like {desc}");
        if use_owner && let Some((k, share, top)) = neighbours(&owner, &labels, e, K) {
            // Near-identical sorted items are the strongest signal.
            let kconf = (share * (0.75 + 0.25 * ((top - 0.6) / 0.3).clamp(0.0, 1.0))).min(0.99);
            if k == key {
                conf = 1.0 - (1.0 - conf) * (1.0 - kconf);
                because = format!("{because}, like similar items you sorted");
            } else if kconf > conf {
                key = k;
                conf = kconf * (1.0 - conf * 0.5);
                because = "similar items you sorted are there".into();
            }
        }
        // Keep an equal or better proposal (a rule, the decision model).
        if review == 1 && current.is_some() && current_conf.unwrap_or(0.0) as f32 >= conf && current.as_deref() != Some(key.as_str()) {
            continue;
        }
        let Some((name, threshold)) = info.get(&key) else { continue };
        let certain = conf >= *threshold;
        let review_new = if certain { 2 } else { 1 };
        if current.as_deref() == Some(key.as_str()) && review >= review_new && current_conf.unwrap_or(0.0) as f32 >= conf {
            continue;
        }
        edits::set(
            c,
            None,
            id,
            &Change {
                field: "category",
                value: edits::category_value(Some(&key), "image-model", Some(conf), review_new),
                source: "image-model",
                source_ref: Some(MODEL_ID),
                conf: Some(conf),
                why: Some(format!("{name}, because {because} (image model, {:.0}%).", conf * 100.0)),
            },
        )?;
        if certain {
            out.decided += 1;
        } else {
            out.proposed += 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_is_monotonic() {
        let mut last = 0.0;
        for i in 0..60 {
            let c = zero_shot_conf(i as f32 * 0.001);
            assert!(c >= last && (0.0..=1.0).contains(&c));
            last = c;
        }
        assert!((zero_shot_conf(0.020) - 0.97).abs() < 1e-6);
        assert_eq!(prompts("receipts", "Receipts", "Shop receipts and bills."), vec!["a photo of receipts", "Shop receipts and bills."]);
    }

    #[test]
    fn neighbours_vote() {
        let mut ix = Index::new(3);
        let mut labels = HashMap::new();
        for (i, v, l) in [(1, [1.0, 0.0, 0.0], "a"), (2, [0.9, 0.1, 0.0], "a"), (3, [0.0, 1.0, 0.0], "b")] {
            let mut v = v.to_vec();
            vectors::normalize(&mut v);
            ix.insert(i, &v);
            labels.insert(i, l.to_string());
        }
        let (k, share, top) = neighbours(&ix, &labels, &[1.0, 0.0, 0.0], 3).unwrap();
        assert_eq!(k, "a");
        assert!(share > 0.9 && top > 0.99);
    }
}
