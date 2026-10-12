//! Thumbnails and previews: generation from the original file and storage
//! in `thumbs.db`. 256 px thumbnails are kept for every asset; 1024 px
//! previews are an LRU cache with a size budget.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;
use roll_media::decode::{self, Source};
use roll_media::ffmpeg::Ffmpeg;
use roll_media::thumb;
use rusqlite::{OptionalExtension, params};

use crate::db::{Db, now_ms};
use crate::model::problem;
use crate::paths::join_rel;

const MIGRATIONS: &[&str] = &["
CREATE TABLE thumbs (
  asset_id INTEGER NOT NULL,
  size INTEGER NOT NULL,
  v INTEGER NOT NULL,
  data BLOB NOT NULL,
  accessed INTEGER NOT NULL,
  PRIMARY KEY (asset_id, size)
)"];

pub const SMALL: i64 = 256;
pub const PREVIEW: i64 = 1024;

pub struct ThumbStore {
    db: Db,
}

impl ThumbStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Ok(Self { db: Db::open_with(path, MIGRATIONS)? })
    }

    pub fn memory() -> rusqlite::Result<Self> {
        Ok(Self { db: Db::memory_with(MIGRATIONS)? })
    }

    pub fn get(&self, asset: i64, size: i64) -> rusqlite::Result<Option<(i64, Vec<u8>)>> {
        self.db
            .read()
            .query_row("SELECT v, data FROM thumbs WHERE asset_id = ?1 AND size = ?2", params![asset, size], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
    }

    pub fn put(&self, asset: i64, size: i64, v: i64, data: &[u8]) -> rusqlite::Result<()> {
        self.db.write().execute(
            "INSERT OR REPLACE INTO thumbs (asset_id, size, v, data, accessed) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![asset, size, v, data, now_ms()],
        )?;
        Ok(())
    }

    pub fn put_many(&self, rows: &[(i64, i64, i64, Vec<u8>)]) -> rusqlite::Result<()> {
        let now = now_ms();
        self.db.tx(|tx| {
            let mut st =
                tx.prepare_cached("INSERT OR REPLACE INTO thumbs (asset_id, size, v, data, accessed) VALUES (?1, ?2, ?3, ?4, ?5)")?;
            for (a, s, v, d) in rows {
                st.execute(params![a, s, v, d, now])?;
            }
            Ok(())
        })
    }

    pub fn touch(&self, asset: i64, size: i64) {
        let _ =
            self.db.write().execute("UPDATE thumbs SET accessed = ?3 WHERE asset_id = ?1 AND size = ?2", params![asset, size, now_ms()]);
    }

    pub fn remove(&self, asset: i64) -> rusqlite::Result<()> {
        self.db.write().execute("DELETE FROM thumbs WHERE asset_id = ?1", [asset])?;
        Ok(())
    }

    /// Drop the least recently used previews until they fit the budget.
    pub fn evict_previews(&self, budget_bytes: i64) -> rusqlite::Result<usize> {
        let c = self.db.write();
        let total: i64 = c.query_row("SELECT coalesce(sum(length(data)), 0) FROM thumbs WHERE size = ?1", [PREVIEW], |r| r.get(0))?;
        if total <= budget_bytes {
            return Ok(0);
        }
        let mut excess = total - budget_bytes;
        let mut st = c.prepare("SELECT asset_id, length(data) FROM thumbs WHERE size = ?1 ORDER BY accessed")?;
        let victims: Vec<i64> = st
            .query_map([PREVIEW], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
            .filter_map(Result::ok)
            .take_while(|(_, len)| {
                let keep_going = excess > 0;
                excess -= len;
                keep_going
            })
            .map(|(id, _)| id)
            .collect();
        for id in &victims {
            c.execute("DELETE FROM thumbs WHERE asset_id = ?1 AND size = ?2", params![id, PREVIEW])?;
        }
        Ok(victims.len())
    }

    pub fn bytes(&self) -> rusqlite::Result<(i64, i64)> {
        self.db.read().query_row(
            "SELECT coalesce(sum(CASE WHEN size = 256 THEN length(data) END), 0), coalesce(sum(CASE WHEN size = 1024 THEN length(data) END), 0) FROM thumbs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }
}

/// What thumbnailing needs to know about an asset.
#[derive(Debug, Clone)]
pub struct ThumbInput {
    pub id: i64,
    pub path: PathBuf,
    pub format: String,
    pub video: bool,
    pub orientation: Option<u16>,
    pub duration_ms: Option<u64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub thumb_v: i64,
}

pub fn load_input(db: &Db, id: i64) -> rusqlite::Result<Option<ThumbInput>> {
    db.read()
        .query_row(
            "SELECT a.id, r.path, a.rel_path, a.format, a.kind, a.orientation, a.duration_ms, a.width, a.height, a.thumb_v
             FROM assets a JOIN roots r ON r.id = a.root_id WHERE a.id = ?1",
            [id],
            row_input,
        )
        .optional()
}

fn row_input(r: &rusqlite::Row) -> rusqlite::Result<ThumbInput> {
    let root: String = r.get(1)?;
    let rel: String = r.get(2)?;
    Ok(ThumbInput {
        id: r.get(0)?,
        path: join_rel(Path::new(&root), &rel),
        format: r.get(3)?,
        video: r.get::<_, i64>(4)? == 1,
        orientation: r.get::<_, Option<i64>>(5)?.map(|o| o as u16),
        duration_ms: r.get::<_, Option<i64>>(6)?.map(|d| d as u64),
        width: r.get(7)?,
        height: r.get(8)?,
        thumb_v: r.get(9)?,
    })
}

pub struct Made {
    pub id: i64,
    pub v: i64,
    pub result: Result<thumb::Thumbs, String>,
}

pub fn source(i: &ThumbInput) -> Source<'_> {
    Source { path: &i.path, format: &i.format, video: i.video, orientation: i.orientation, duration_ms: i.duration_ms }
}

pub fn make_one(i: &ThumbInput, ff: Option<&Ffmpeg>) -> Made {
    let result = decode::decode(source(i), ff, Some(1024)).map(|img| thumb::make(&img)).map_err(|e| e.to_string());
    Made { id: i.id, v: i.thumb_v + 1, result }
}

/// Store results: blobs in `thumbs.db`, derived values on the asset.
pub fn save(db: &Db, store: &ThumbStore, made: &[(Made, Option<(i64, i64)>)]) -> anyhow::Result<()> {
    let blobs: Vec<(i64, i64, i64, Vec<u8>)> =
        made.iter().filter_map(|(m, _)| m.result.as_ref().ok().map(|t| (m.id, SMALL, m.v, t.webp.clone()))).collect();
    store.put_many(&blobs)?;
    db.tx(|tx| {
        for (m, stored) in made {
            match &m.result {
                Ok(t) => {
                    // Trust the decoded image for which side is longer: it
                    // has orientation and HEIF transforms applied.
                    let (mut w, mut h) = stored.unwrap_or((t.source_w as i64, t.source_h as i64));
                    if (w > h) != (t.source_w > t.source_h) && w != h && t.source_w != t.source_h {
                        std::mem::swap(&mut w, &mut h);
                    }
                    tx.execute(
                        "UPDATE assets SET thumb = 1, thumb_v = ?2, avg_color = ?3, thumbhash = ?4, stats = ?5, phash = ?6,
                           width = ?7, height = ?8 WHERE id = ?1",
                        params![
                            m.id,
                            m.v,
                            t.avg_rgb,
                            t.thumbhash,
                            serde_json::to_string(&t.stats).unwrap_or_default(),
                            t.dhash as i64,
                            w,
                            h
                        ],
                    )?;
                }
                Err(_) => {
                    tx.execute("UPDATE assets SET thumb = 2, problems = problems | ?2 WHERE id = ?1", params![m.id, problem::BROKEN])?;
                }
            }
        }
        Ok(())
    })?;
    Ok(())
}

/// Thumbnail every asset that has none, newest first. Resumable: it simply
/// continues with what is left.
pub fn run_pending(
    db: &Db,
    store: &ThumbStore,
    ff: Option<&Ffmpeg>,
    pool: &rayon::ThreadPool,
    check: &dyn Fn(usize, usize) -> bool,
) -> anyhow::Result<usize> {
    let total: i64 = db.read().query_row("SELECT count(*) FROM assets WHERE thumb = 0 AND missing != 1", [], |r| r.get(0))?;
    let done = AtomicUsize::new(0);
    loop {
        let batch: Vec<ThumbInput> = {
            let c = db.read();
            let mut st = c.prepare(
                "SELECT a.id, r.path, a.rel_path, a.format, a.kind, a.orientation, a.duration_ms, a.width, a.height, a.thumb_v
                 FROM assets a JOIN roots r ON r.id = a.root_id WHERE a.thumb = 0 AND a.missing != 1
                 ORDER BY a.missing = 2 DESC, a.taken_utc DESC LIMIT 128",
            )?;
            st.query_map([], row_input)?.collect::<Result<_, _>>()?
        };
        if batch.is_empty() {
            break;
        }
        let made: Vec<(Made, Option<(i64, i64)>)> = pool.install(|| {
            batch
                .par_iter()
                .map(|i| {
                    let m = make_one(i, ff);
                    done.fetch_add(1, Ordering::Relaxed);
                    (m, i.width.zip(i.height))
                })
                .collect()
        });
        save(db, store, &made)?;
        if !check(done.load(Ordering::Relaxed), total as usize) {
            break;
        }
    }
    Ok(done.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_evict() {
        let s = ThumbStore::memory().unwrap();
        s.put(1, SMALL, 1, b"small").unwrap();
        for id in 1..=5 {
            s.put(id, PREVIEW, 1, &vec![0u8; 1000]).unwrap();
        }
        assert_eq!(s.get(1, SMALL).unwrap().unwrap().1, b"small");
        assert_eq!(s.evict_previews(2500).unwrap(), 3);
        assert_eq!(s.bytes().unwrap(), (5, 2000));
    }
}
