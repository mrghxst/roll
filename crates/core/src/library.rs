//! The library: one object that owns the databases, tools and jobs, and
//! answers everything the UI asks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use parking_lot::Mutex;
use roll_media::decode;
use roll_media::ffmpeg::Ffmpeg;
use roll_media::thumb;
use roll_meta::exiftool::ExifToolPool;
use rusqlite::{OptionalExtension, params, params_from_iter};

use crate::dates::SystemTz;
use crate::db::Db;
use crate::jobs::{JobCtx, JobSpec, Jobs};
use crate::model::*;
use crate::paths::{app_data_dir, join_rel};
use crate::query;
use crate::settings::Settings;
use crate::thumbs::{self, ThumbStore};
use crate::{groups, scan, tags, watch, why};

#[derive(Debug, Clone)]
pub enum LibEvent {
    Jobs(Vec<JobInfo>),
    /// Something the UI shows changed: "assets", "roots", "settings"…
    Changed(&'static str),
    Scanned(ScanReport),
}

pub type EventSink = Arc<dyn Fn(LibEvent) + Send + Sync>;

pub struct Library {
    pub data_dir: PathBuf,
    pub db: Arc<Db>,
    pub thumbs: ThumbStore,
    pub ffmpeg: Option<Ffmpeg>,
    pub exiftool: Option<ExifToolPool>,
    pub jobs: Jobs,
    pub tz: SystemTz,
    sink: EventSink,
    pool: rayon::ThreadPool,
    watcher: Mutex<Option<watch::RootWatcher>>,
}

impl Library {
    pub fn open_default(sink: EventSink) -> anyhow::Result<Arc<Self>> {
        Self::open(&app_data_dir(), sink)
    }

    pub fn open(data_dir: &Path, sink: EventSink) -> anyhow::Result<Arc<Self>> {
        std::fs::create_dir_all(data_dir)?;
        let db = Arc::new(Db::open(&data_dir.join("library.db"))?);
        let thumbs = ThumbStore::open(&data_dir.join("thumbs.db"))?;
        let settings = Settings::load(&db);
        let s2 = sink.clone();
        let jobs = Jobs::new(db.clone(), Arc::new(move |list| s2(LibEvent::Jobs(list))));
        let pool =
            rayon::ThreadPoolBuilder::new().num_threads(settings.thread_count()).thread_name(|i| format!("roll-work-{i}")).build()?;
        Ok(Arc::new(Self {
            data_dir: data_dir.to_path_buf(),
            db,
            thumbs,
            ffmpeg: Ffmpeg::locate(),
            exiftool: ExifToolPool::locate().ok(),
            jobs,
            tz: SystemTz,
            sink,
            pool,
            watcher: Mutex::new(None),
        }))
    }

    /// Start background work: the job worker, file watching, and a check of
    /// every root for changes made while Roll was closed.
    pub fn start(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        self.jobs.start(Arc::new(move |spec, ctx| match me.upgrade() {
            Some(lib) => lib.execute(spec, ctx),
            None => Ok(()),
        }));
        self.restart_watcher();
        for r in self.roots() {
            let _ = self.jobs.enqueue(JobSpec::Scan { root_id: r.id as i64 });
        }
        let _ = self.jobs.enqueue(JobSpec::Thumbs);
    }

    fn emit(&self, e: LibEvent) {
        (self.sink)(e)
    }

    fn restart_watcher(self: &Arc<Self>) {
        let roots: Vec<(i64, PathBuf)> = self.roots().into_iter().map(|r| (r.id as i64, PathBuf::from(r.path))).collect();
        let me = Arc::downgrade(self);
        let w = watch::RootWatcher::start(&roots, move |root_id| {
            if let Some(lib) = me.upgrade() {
                let _ = lib.jobs.enqueue(JobSpec::Scan { root_id });
            }
        });
        *self.watcher.lock() = w.ok();
    }

    fn execute(self: &Arc<Self>, spec: &JobSpec, ctx: &JobCtx) -> anyhow::Result<()> {
        match spec {
            JobSpec::Scan { root_id } => {
                let Some(root) = scan::root_path(&self.db.read(), *root_id)? else { return Ok(()) };
                if !root.is_dir() {
                    anyhow::bail!("The folder {} is not available. Connect the drive or remove the folder in Settings.", root.display());
                }
                let first =
                    self.db.read().query_row("SELECT count(*) = 0 FROM assets WHERE root_id = ?1", [root_id], |r| r.get::<_, bool>(0))?;
                // Show new items as chunks are committed, at most every 2 s.
                let last = Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(10));
                let progress = |phase: &str, done: usize, total: usize| {
                    ctx.progress(done, total);
                    let mut l = last.lock();
                    if phase == scan::SAVED && l.elapsed().as_secs() >= 2 {
                        *l = std::time::Instant::now();
                        self.emit(LibEvent::Changed("assets"));
                    }
                };
                let (report, _touched) = scan::scan_root(&self.db, *root_id, &root, &self.tz, !first, &progress)?;
                groups::rebuild(&self.db, *root_id)?;
                self.emit(LibEvent::Scanned(report));
                self.emit(LibEvent::Changed("assets"));
                self.jobs.enqueue(JobSpec::Thumbs)?;
                Ok(())
            }
            JobSpec::Thumbs => {
                let last_emit = Mutex::new(std::time::Instant::now());
                thumbs::run_pending(&self.db, &self.thumbs, self.ffmpeg.as_ref(), &self.pool, &|done, total| {
                    let mut last = last_emit.lock();
                    if last.elapsed().as_secs() >= 2 {
                        self.emit(LibEvent::Changed("thumbs"));
                        *last = std::time::Instant::now();
                    }
                    ctx.progress(done, total)
                })?;
                self.emit(LibEvent::Changed("thumbs"));
                Ok(())
            }
            JobSpec::Hash => self.run_hash(ctx),
        }
    }

    fn run_hash(&self, ctx: &JobCtx) -> anyhow::Result<()> {
        let todo: Vec<(i64, PathBuf)> = {
            let c = self.db.read();
            let mut st = c.prepare(
                "SELECT a.id, r.path, a.rel_path FROM assets a JOIN roots r ON r.id = a.root_id WHERE a.blake3 IS NULL AND a.missing = 0",
            )?;
            st.query_map([], |r| Ok((r.get(0)?, join_rel(Path::new(&r.get::<_, String>(1)?), &r.get::<_, String>(2)?))))?
                .collect::<Result<_, _>>()?
        };
        let total = todo.len();
        for (i, chunk) in todo.chunks(64).enumerate() {
            let hashed: Vec<(i64, String)> = self.pool.install(|| {
                use rayon::prelude::*;
                chunk.par_iter().filter_map(|(id, p)| roll_media::hash::blake3_file(p).ok().map(|h| (*id, h))).collect()
            });
            self.db.tx(|tx| {
                for (id, h) in &hashed {
                    tx.execute("UPDATE assets SET blake3 = ?2 WHERE id = ?1", params![id, h])?;
                }
                Ok(())
            })?;
            if !ctx.progress(((i + 1) * 64).min(total), total) {
                break;
            }
        }
        Ok(())
    }

    // Settings

    pub fn settings(&self) -> Settings {
        Settings::load(&self.db)
    }

    pub fn save_settings(&self, s: &Settings) -> anyhow::Result<()> {
        s.save(&self.db)?;
        self.emit(LibEvent::Changed("settings"));
        Ok(())
    }

    // Roots

    pub fn roots(&self) -> Vec<RootInfo> {
        let c = self.db.read();
        let Ok(mut st) = c.prepare(
            "SELECT r.id, r.path, r.kind, (SELECT count(*) FROM assets a WHERE a.root_id = r.id AND a.missing = 0 AND a.trashed_at IS NULL)
             FROM roots r WHERE r.kind = 'library' ORDER BY r.id",
        ) else {
            return Vec::new();
        };
        st.query_map([], |r| Ok(RootInfo { id: r.get(0)?, path: r.get(1)?, kind: r.get(2)?, count: r.get(3)? }))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    pub fn add_root(self: &Arc<Self>, path: &Path) -> anyhow::Result<RootInfo> {
        let path = std::path::absolute(path)?;
        if !path.is_dir() {
            anyhow::bail!("{} is not a folder.", path.display());
        }
        let p = path.to_string_lossy().into_owned();
        for r in self.roots() {
            let existing = Path::new(&r.path);
            if path.starts_with(existing) || existing.starts_with(&path) {
                anyhow::bail!("This folder overlaps with {}, which is already in the library.", r.path);
            }
        }
        let id = {
            let c = self.db.write();
            c.execute("INSERT INTO roots (path, kind, added_at) VALUES (?1, 'library', ?2)", params![p, crate::db::now_ms()])?;
            c.last_insert_rowid()
        };
        self.jobs.enqueue(JobSpec::Scan { root_id: id })?;
        self.restart_watcher();
        self.emit(LibEvent::Changed("roots"));
        Ok(RootInfo { id: id as i32, path: p, kind: "library".into(), count: 0 })
    }

    /// Forget a root. Files are not touched.
    pub fn remove_root(self: &Arc<Self>, id: i64) -> anyhow::Result<()> {
        let ids: Vec<i64> = {
            let c = self.db.read();
            let mut st = c.prepare("SELECT id FROM assets WHERE root_id = ?1")?;
            st.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        self.db.write().execute("DELETE FROM roots WHERE id = ?1", [id])?;
        for a in ids {
            let _ = self.thumbs.remove(a);
        }
        self.restart_watcher();
        self.emit(LibEvent::Changed("roots"));
        Ok(())
    }

    pub fn rescan(&self) -> anyhow::Result<()> {
        for r in self.roots() {
            self.jobs.enqueue(JobSpec::Scan { root_id: r.id as i64 })?;
        }
        Ok(())
    }

    // Browsing

    pub fn timeline(&self, q: &Query) -> anyhow::Result<Vec<u8>> {
        Ok(query::timeline(&self.db.read(), q)?)
    }

    pub fn ids(&self, q: &Query) -> anyhow::Result<Vec<i64>> {
        Ok(query::ids(&self.db.read(), q)?)
    }

    pub fn tiles(&self, ids: &[i64]) -> anyhow::Result<Vec<TileInfo>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let c = self.db.read();
        let list = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        let mut st =
            c.prepare(&format!("SELECT id, name, thumb_v, thumbhash, duration_ms, description FROM assets WHERE id IN ({list})"))?;
        let rows = st.query_map([], |r| {
            let hash: Option<Vec<u8>> = r.get(3)?;
            Ok(TileInfo {
                id: r.get(0)?,
                name: r.get(1)?,
                thumb_v: r.get(2)?,
                thumbhash: hash.map(|h| base64::engine::general_purpose::STANDARD.encode(h)),
                duration_ms: r.get(4)?,
                caption: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn detail(&self, id: i64) -> anyhow::Result<Option<AssetDetail>> {
        let c = self.db.read();
        let row = c
            .query_row(
                "SELECT a.id, a.root_id, r.path, a.rel_path, a.name, a.kind, a.format, a.size, a.width, a.height, a.duration_ms, a.fps,
                        a.codec, a.has_audio, a.taken_utc, a.taken_local, a.tz_offset, a.date_source, a.lat, a.lon, a.loc_acc,
                        a.loc_source, a.make, a.model, a.lens, a.software, c.key, c.name, a.category_conf, a.review, a.rating,
                        a.favorite, a.description, a.private, a.immich, a.immich_id, g.kind, a.group_role, a.group_id,
                        a.problems, a.thumb_v, a.category_source, a.missing, a.trashed_at
                 FROM assets a JOIN roots r ON r.id = a.root_id LEFT JOIN categories c ON c.id = a.category_id
                 LEFT JOIN groups g ON g.id = a.group_id WHERE a.id = ?1",
                [id],
                |r| {
                    let root: String = r.get(2)?;
                    let rel: String = r.get(3)?;
                    Ok((
                        AssetDetail {
                            id: r.get(0)?,
                            root_id: r.get(1)?,
                            path: join_rel(Path::new(&root), &rel).to_string_lossy().into_owned(),
                            rel_path: rel,
                            name: r.get(4)?,
                            kind: if r.get::<_, i64>(5)? == 1 { "video".into() } else { "photo".into() },
                            format: r.get(6)?,
                            size: r.get(7)?,
                            width: r.get(8)?,
                            height: r.get(9)?,
                            duration_ms: r.get(10)?,
                            fps: r.get(11)?,
                            codec: r.get(12)?,
                            has_audio: r.get(13)?,
                            taken_utc: r.get(14)?,
                            taken_local: r.get(15)?,
                            tz_offset: r.get(16)?,
                            date_source: r.get(17)?,
                            lat: r.get(18)?,
                            lon: r.get(19)?,
                            loc_acc: r.get(20)?,
                            loc_source: r.get(21)?,
                            place: None,
                            make: r.get(22)?,
                            model: r.get(23)?,
                            lens: r.get(24)?,
                            software: r.get(25)?,
                            category: r.get(26)?,
                            category_name: r.get(27)?,
                            category_conf: r.get(28)?,
                            review: r.get(29)?,
                            tags: Vec::new(),
                            rating: r.get(30)?,
                            favorite: r.get::<_, i64>(31)? != 0,
                            description: r.get(32)?,
                            private: r.get::<_, i64>(33)? != 0,
                            immich: r.get(34)?,
                            immich_id: r.get(35)?,
                            group_kind: r.get(36)?,
                            group_role: r.get(37)?,
                            group_members: Vec::new(),
                            problems: r.get(39)?,
                            thumb_v: r.get(40)?,
                            why: Vec::new(),
                            sidecars: Vec::new(),
                            missing: r.get::<_, i64>(42)? != 0,
                            trashed: r.get::<_, Option<i64>>(43)?.is_some(),
                        },
                        r.get::<_, Option<i64>>(38)?,
                        r.get::<_, Option<String>>(41)?,
                    ))
                },
            )
            .optional()?;
        let Some((mut d, group_id, cat_source)) = row else { return Ok(None) };
        d.tags = tags::of_asset(&c, id)?;
        if let Some(g) = group_id {
            let mut st = c.prepare("SELECT id FROM assets WHERE group_id = ?1 ORDER BY group_role != 'primary', id")?;
            d.group_members = st.query_map([g], |r| r.get(0))?.collect::<Result<_, _>>()?;
        }
        let mut st = c.prepare("SELECT rel_path FROM sidecars WHERE asset_id = ?1")?;
        d.sidecars = st.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        d.why = why::explain(&c, id, d.date_source.as_deref(), d.loc_source.as_deref(), cat_source.as_deref(), d.category_name.as_deref())?;
        Ok(Some(d))
    }

    pub fn folders(&self) -> anyhow::Result<Vec<FolderNode>> {
        let c = self.db.read();
        let mut out = Vec::new();
        for r in self.roots() {
            let mut st = c.prepare("SELECT dir, count(*) FROM assets WHERE root_id = ?1 AND missing = 0 AND trashed_at IS NULL AND (group_role IS NULL OR group_role = 'primary') GROUP BY dir")?;
            let counts: Vec<(String, i32)> = st.query_map([r.id], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?;
            out.push(folder_tree(r.id, &r.path, &counts));
        }
        Ok(out)
    }

    pub fn categories(&self) -> anyhow::Result<Vec<Category>> {
        let c = self.db.read();
        let mut st = c.prepare(
            "SELECT c.id, c.key, c.name, c.parent_id, c.definition, c.folder_template, c.own_folder, c.immich_default, c.auto_threshold, c.builtin,
               (SELECT count(*) FROM assets a WHERE (a.category_id = c.id OR (c.key = 'unsorted' AND a.category_id IS NULL))
                  AND a.missing = 0 AND a.trashed_at IS NULL AND (a.group_role IS NULL OR a.group_role = 'primary'))
             FROM categories c ORDER BY coalesce(c.parent_id, c.id), c.parent_id IS NOT NULL, c.sort, c.name",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Category {
                id: r.get(0)?,
                key: r.get(1)?,
                name: r.get(2)?,
                parent_id: r.get(3)?,
                definition: r.get(4)?,
                folder_template: r.get(5)?,
                own_folder: r.get::<_, i64>(6)? != 0,
                immich_default: r.get::<_, i64>(7)? != 0,
                auto_threshold: r.get(8)?,
                builtin: r.get::<_, i64>(9)? != 0,
                count: r.get(10)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn tag_counts(&self) -> anyhow::Result<Vec<TagCount>> {
        let c = self.db.read();
        let mut st = c.prepare(
            "SELECT t.id, t.path, count(a.id) FROM tags t JOIN asset_tags at ON at.tag_id = t.id
             JOIN assets a ON a.id = at.asset_id AND a.missing = 0 AND a.trashed_at IS NULL
             WHERE t.path NOT LIKE 'Category/%' GROUP BY t.id ORDER BY t.path",
        )?;
        let rows = st.query_map([], |r| Ok(TagCount { id: r.get(0)?, path: r.get(1)?, count: r.get(2)? }))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn counts(&self) -> anyhow::Result<Counts> {
        let c = self.db.read();
        let one = |sql: &str| -> rusqlite::Result<i32> { c.query_row(sql, [], |r| r.get(0)) };
        let live = "missing = 0 AND trashed_at IS NULL AND (group_role IS NULL OR group_role = 'primary')";
        Ok(Counts {
            total: one(&format!("SELECT count(*) FROM assets WHERE {live}"))?,
            inbox: one(&format!("SELECT count(*) FROM assets WHERE {live} AND inbox > 0"))?,
            review: one(&format!("SELECT count(*) FROM assets WHERE {live} AND review = 1"))?,
            date_issues: one(&format!(
                "SELECT count(*) FROM assets WHERE {live} AND (problems & {}) != 0",
                problem::NO_DATE | problem::SUSPICIOUS_DATE
            ))?,
            no_location: one(&format!(
                "SELECT count(*) FROM assets WHERE {live} AND lat IS NULL AND (category_id IS NULL OR category_id = (SELECT id FROM categories WHERE key = 'photos'))"
            ))?,
            trash: one("SELECT count(*) FROM assets WHERE trashed_at IS NOT NULL AND missing = 0")?,
            duplicates: one(&format!(
                "SELECT count(*) FROM assets WHERE {live} AND blake3 IS NOT NULL AND blake3 IN (SELECT blake3 FROM assets WHERE missing = 0 AND trashed_at IS NULL AND blake3 IS NOT NULL GROUP BY blake3 HAVING count(*) > 1)"
            ))?,
        })
    }

    pub fn facets(&self, q: &Query) -> anyhow::Result<Facets> {
        let c = self.db.read();
        let sql = query::where_clause(q);
        let facet = |select: &str, group: &str, limit: usize| -> rusqlite::Result<Vec<Facet>> {
            let stmt = format!(
                "SELECT {select}, count(*) FROM assets a WHERE {} GROUP BY {group} ORDER BY count(*) DESC LIMIT {limit}",
                sql.clause
            );
            let mut st = c.prepare(&stmt)?;
            let rows = st.query_map(params_from_iter(sql.params.iter()), |r| {
                let key: Option<String> = r.get(0)?;
                let label: Option<String> = r.get(1)?;
                Ok(Facet { key: key.clone().unwrap_or_default(), label: label.or(key).unwrap_or_default(), count: r.get(2)? })
            })?;
            Ok(rows.filter_map(Result::ok).filter(|f| !f.key.is_empty()).collect())
        };
        let categories = facet(
            "coalesce((SELECT key FROM categories c WHERE c.id = a.category_id), 'unsorted'), coalesce((SELECT name FROM categories c WHERE c.id = a.category_id), 'Unsorted')",
            "1",
            50,
        )?;
        let media = facet("CASE a.kind WHEN 1 THEN 'video' ELSE 'photo' END, CASE a.kind WHEN 1 THEN 'Videos' ELSE 'Photos' END", "1", 5)?;
        let cameras = facet("coalesce(a.model, a.make), coalesce(a.model, a.make)", "1", 40)?;
        let lenses = facet("a.lens, a.lens", "1", 30)?;
        let formats = facet("a.format, upper(a.format)", "1", 20)?;
        let mut years = facet("substr(a.taken_local, 1, 4), substr(a.taken_local, 1, 4)", "1", 100)?;
        years.sort_by(|a, b| b.key.cmp(&a.key));
        let location = facet(
            "CASE WHEN a.lat IS NULL THEN 'none' WHEN a.loc_acc > 100 THEN 'approximate' ELSE 'exact' END,
             CASE WHEN a.lat IS NULL THEN 'No location' WHEN a.loc_acc > 100 THEN 'Approximate' ELSE 'Exact' END",
            "1",
            3,
        )?;
        let orientation = facet(
            "CASE WHEN a.width > a.height THEN 'landscape' WHEN a.width < a.height THEN 'portrait' ELSE 'square' END,
             CASE WHEN a.width > a.height THEN 'Landscape' WHEN a.width < a.height THEN 'Portrait' ELSE 'Square' END",
            "1",
            3,
        )?;
        let mut problems = Vec::new();
        for (bit, key, label) in [
            (problem::NO_DATE, "no_date", "No date"),
            (problem::SUSPICIOUS_DATE, "suspicious_date", "Suspicious date"),
            (problem::DATE_CHANGED, "date_changed", "Date changed"),
            (problem::NO_LOCATION, "no_location", "No location"),
        ] {
            let n: i32 = c.query_row(
                &format!("SELECT count(*) FROM assets a WHERE {} AND (a.problems & {bit}) != 0", sql.clause),
                params_from_iter(sql.params.iter()),
                |r| r.get(0),
            )?;
            if n > 0 {
                problems.push(Facet { key: key.into(), label: label.into(), count: n });
            }
        }
        let tags = {
            let stmt = format!(
                "SELECT t.path, count(*) FROM assets a JOIN asset_tags at ON at.asset_id = a.id JOIN tags t ON t.id = at.tag_id
                 WHERE {} AND t.path NOT LIKE 'Category/%' GROUP BY t.id ORDER BY count(*) DESC LIMIT 40",
                sql.clause
            );
            let mut st = c.prepare(&stmt)?;
            let rows =
                st.query_map(params_from_iter(sql.params.iter()), |r| Ok(Facet { key: r.get(0)?, label: r.get(0)?, count: r.get(1)? }))?;
            rows.filter_map(Result::ok).collect()
        };
        let immich = facet(
            &format!(
                "CASE WHEN a.immich_id IS NOT NULL THEN 'sent' WHEN {sel} = 1 THEN 'pending' ELSE 'local' END,
                      CASE WHEN a.immich_id IS NOT NULL THEN 'Sent' WHEN {sel} = 1 THEN 'Waiting for the next sync' ELSE 'Local only' END",
                sel = query::IMMICH_SELECTED
            ),
            "1",
            3,
        )?;
        Ok(Facets { categories, media, cameras, lenses, formats, years, location, problems, tags, immich, orientation })
    }

    // Images

    /// The 256 px thumbnail, made now if missing (visible tiles come first).
    pub fn thumb(&self, id: i64) -> anyhow::Result<Option<Vec<u8>>> {
        if let Some((_, data)) = self.thumbs.get(id, thumbs::SMALL)? {
            return Ok(Some(data));
        }
        let Some(input) = thumbs::load_input(&self.db, id)? else { return Ok(None) };
        let made = thumbs::make_one(&input, self.ffmpeg.as_ref());
        let data = made.result.as_ref().ok().map(|t| t.webp.clone());
        thumbs::save(&self.db, &self.thumbs, &[(made, input.width.zip(input.height))])?;
        Ok(data)
    }

    /// The 1024 px preview, from the cache or made now.
    pub fn preview(&self, id: i64) -> anyhow::Result<Option<Vec<u8>>> {
        if let Some((_, data)) = self.thumbs.get(id, thumbs::PREVIEW)? {
            self.thumbs.touch(id, thumbs::PREVIEW);
            return Ok(Some(data));
        }
        let Some(input) = thumbs::load_input(&self.db, id)? else { return Ok(None) };
        let img = decode::decode(thumbs::source(&input), self.ffmpeg.as_ref(), Some(thumb::PREVIEW_LONG))?;
        let data = thumb::preview(&img);
        self.thumbs.put(id, thumbs::PREVIEW, input.thumb_v, &data)?;
        let budget = (self.settings().preview_cache_gb as f64 * 1e9) as i64;
        let _ = self.thumbs.evict_previews(budget);
        Ok(Some(data))
    }

    pub fn asset_file(&self, id: i64) -> anyhow::Result<Option<(PathBuf, String, bool)>> {
        Ok(thumbs::load_input(&self.db, id)?.map(|i| (i.path, i.format, i.video)))
    }

    /// The full image as the WebView can show it: the original for JPEG,
    /// PNG, GIF and WebP; a cached full-size JPEG for HEIC, RAW and others.
    pub fn full_image(&self, id: i64) -> anyhow::Result<Option<(PathBuf, &'static str)>> {
        let Some(input) = thumbs::load_input(&self.db, id)? else { return Ok(None) };
        if decode::browser_native(&input.format) {
            let mime = roll_meta::detect::mime_of(&input.format);
            return Ok(Some((input.path, mime)));
        }
        let dir = self.data_dir.join("cache").join("full");
        std::fs::create_dir_all(&dir)?;
        let out = dir.join(format!("{id}-{}.jpg", input.thumb_v));
        if !out.exists() {
            let img = decode::decode(thumbs::source(&input), self.ffmpeg.as_ref(), None)?;
            let tmp = out.with_extension("tmp");
            std::fs::write(&tmp, thumb::full_jpeg(&img))?;
            std::fs::rename(&tmp, &out)?;
            prune_dir(&dir, 200);
        }
        Ok(Some((out, "image/jpeg")))
    }

    /// A playable video: the original when the WebView supports its codec,
    /// else a cached proxy.
    pub fn playable_video(&self, id: i64, native_ok: bool) -> anyhow::Result<Option<(PathBuf, &'static str)>> {
        let Some(input) = thumbs::load_input(&self.db, id)? else { return Ok(None) };
        if native_ok {
            let mime = roll_meta::detect::mime_of(&input.format);
            return Ok(Some((input.path, if mime == "video/quicktime" { "video/mp4" } else { mime })));
        }
        let ff = self.ffmpeg.as_ref().ok_or_else(|| anyhow::anyhow!("FFmpeg is needed to play this video."))?;
        let dir = self.data_dir.join("cache").join("proxy");
        std::fs::create_dir_all(&dir)?;
        for (ext, mime) in [("mp4", "video/mp4"), ("webm", "video/webm")] {
            let p = dir.join(format!("{id}.{ext}"));
            if p.exists() {
                return Ok(Some((p, mime)));
            }
        }
        let tmp = dir.join(format!("{id}.part"));
        let mime = ff.proxy(&input.path, &tmp)?;
        let out = dir.join(format!("{id}.{}", if mime == "video/mp4" { "mp4" } else { "webm" }));
        std::fs::rename(&tmp, &out)?;
        prune_dir(&dir, 50);
        Ok(Some((out, mime)))
    }

    pub fn codec(&self, id: i64) -> anyhow::Result<Option<String>> {
        Ok(self.db.read().query_row("SELECT codec FROM assets WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten())
    }
}

/// Keep at most `keep` files in a cache folder, dropping the oldest.
fn prune_dir(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> =
        rd.flatten().filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()).map(|t| (t, e.path()))).collect();
    if files.len() <= keep {
        return;
    }
    files.sort();
    for (_, p) in files.iter().take(files.len() - keep) {
        let _ = std::fs::remove_file(p);
    }
}

/// Build a folder tree with direct and total counts.
pub fn folder_tree(root_id: i32, root_path: &str, counts: &[(String, i32)]) -> FolderNode {
    #[derive(Default)]
    struct N {
        count: i32,
        children: BTreeMap<String, N>,
    }
    let mut top = N::default();
    for (dir, n) in counts {
        let mut node = &mut top;
        for part in dir.split('/').filter(|p| !p.is_empty()) {
            node = node.children.entry(part.to_string()).or_default();
        }
        node.count += n;
    }
    fn build(root_id: i32, path: String, name: String, n: N) -> FolderNode {
        let children: Vec<FolderNode> = n
            .children
            .into_iter()
            .map(|(k, v)| {
                let p = if path.is_empty() { k.clone() } else { format!("{path}/{k}") };
                build(root_id, p, k, v)
            })
            .collect();
        let total = n.count + children.iter().map(|c| c.total).sum::<i32>();
        FolderNode { root_id, path, name, count: n.count, total, children }
    }
    let name = Path::new(root_path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| root_path.to_string());
    build(root_id, String::new(), name, top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_counts() {
        let t = folder_tree(1, "C:/Photos", &[("".into(), 2), ("2019/Photos".into(), 5), ("2019".into(), 1), ("2020/Memes".into(), 3)]);
        assert_eq!(t.name, "Photos");
        assert_eq!(t.total, 11);
        assert_eq!(t.children[0].name, "2019");
        assert_eq!((t.children[0].count, t.children[0].total), (1, 6));
        assert_eq!(t.children[1].children[0].path, "2020/Memes");
    }
}
