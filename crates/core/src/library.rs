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

use crate::actions::EditAction;
use crate::datefix::{DateFix, DatePreview, DateSet};
use crate::dates::SystemTz;
use crate::db::Db;
use crate::import::{ImportInfo, ImportOptions, ImportPlan};
use crate::jobs::{JobCtx, JobSpec, Jobs};
use crate::model::*;
use crate::paths::{app_data_dir, join_rel};
use crate::places::{PlaceGroup, TrackInfo, ZoneProposal};
use crate::planner::{PlanOptions, PlanSummary};
use crate::query;
use crate::settings::Settings;
use crate::thumbs::{self, ThumbStore};
use crate::{actions, categories, classify, groups, journal, planner, scan, tags, watch, why};
use roll_geo::Place;
use roll_geo::gazetteer::{Gazetteer, Hit};
use roll_meta::write::WriteMode;

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
    /// Loaded on first use from `geodata/places.tsv`.
    gazetteer: Mutex<Option<Option<Arc<Gazetteer>>>>,
    /// The last computed place suggestions, by group key.
    place_cache: Mutex<crate::places::ByGroup>,
    /// Items in duplicate groups, from the last time they were computed.
    dupe_count: std::sync::atomic::AtomicI64,
    pub packs: crate::packs::Packs,
}

/// Whether the offline place names are installed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct PlacesInfo {
    pub installed: bool,
    pub count: i32,
}

impl Library {
    pub fn open_default(sink: EventSink) -> anyhow::Result<Arc<Self>> {
        Self::open(&app_data_dir(), sink)
    }

    pub fn open(data_dir: &Path, sink: EventSink) -> anyhow::Result<Arc<Self>> {
        std::fs::create_dir_all(data_dir)?;
        let db = Arc::new(Db::open(&data_dir.join("library.db"))?);
        crate::rules::seed(&db.write())?;
        let thumbs = ThumbStore::open(&data_dir.join("thumbs.db"))?;
        let settings = Settings::load(&db);
        let s2 = sink.clone();
        let jobs = Jobs::new(db.clone(), Arc::new(move |list| s2(LibEvent::Jobs(list))));
        let s3 = sink.clone();
        let packs = crate::packs::Packs::new(
            data_dir.to_path_buf(),
            crate::packs::Catalog::bundled(),
            Arc::new(move || s3(LibEvent::Changed("packs"))),
        );
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
            gazetteer: Mutex::new(None),
            place_cache: Mutex::new(Default::default()),
            dupe_count: std::sync::atomic::AtomicI64::new(-1),
            packs,
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
        let _ = self.jobs.enqueue(JobSpec::Recover);
        self.restart_watcher();
        for r in self.roots() {
            let _ = self.jobs.enqueue(JobSpec::Scan { root_id: r.id as i64 });
        }
        let _ = self.jobs.enqueue(JobSpec::Thumbs);
        // Build the search index once (after an upgrade, or a lost index).
        let (assets, indexed): (i64, i64) = self
            .db
            .read()
            .query_row("SELECT (SELECT count(*) FROM assets), (SELECT count(*) FROM fts)", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap_or((0, 0));
        if assets != indexed {
            let _ = self.jobs.enqueue(JobSpec::Index);
        }
    }

    pub fn tools(&self) -> journal::Tools<'_> {
        let mode = if self.settings().write_mode == "sidecar_only" { WriteMode::SidecarOnly } else { WriteMode::FileAndSidecar };
        journal::Tools { exiftool: self.exiftool.as_ref(), write_mode: mode }
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
                let (report, touched) = scan::scan_root(&self.db, *root_id, &root, &self.tz, !first, &progress)?;
                groups::rebuild(&self.db, *root_id)?;
                // An export being added: read what its chats and JSON say.
                let import: Option<i64> =
                    self.db.read().query_row("SELECT id FROM imports WHERE root_id = ?1", [root_id], |r| r.get(0)).optional()?;
                if let Some(i) = import {
                    self.db.tx_any(|tx| crate::import::prepare(tx, i, &self.tz))?;
                }
                classify::run(&self.db.write(), Some(&touched))?;
                crate::privacy::guard_descriptions(&self.db.write(), Some(&touched))?;
                self.reindex(&touched);
                self.emit(LibEvent::Scanned(report));
                self.emit(LibEvent::Changed("assets"));
                self.emit(LibEvent::Changed("imports"));
                self.jobs.enqueue(JobSpec::Thumbs)?;
                self.jobs.enqueue(JobSpec::Hash)?;
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
                // Junk rules need the pixel statistics the thumbnails bring.
                self.jobs.enqueue(JobSpec::Classify)?;
                Ok(())
            }
            JobSpec::Classify => {
                let out = classify::run(&self.db.write(), None)?;
                let marked = crate::privacy::guard_descriptions(&self.db.write(), None)?;
                if out != classify::Outcome::default() || marked > 0 {
                    self.emit(LibEvent::Changed("assets"));
                }
                Ok(())
            }
            JobSpec::Hash => self.run_hash(ctx),
            JobSpec::Index => {
                let gaz = self.gaz();
                let place = |la: f64, lo: f64| gaz.as_ref().and_then(|g| g.reverse(la, lo).map(|e| e.label()));
                self.db.tx(|tx| crate::search::reindex(tx, None, &place))?;
                Ok(())
            }
            JobSpec::Immich { remove } => {
                let client = self.immich_client()?;
                client.me()?;
                let report = crate::immich::sync(&self.db, &client, *remove, &|done, total| ctx.progress(done, total));
                let report = match report {
                    Ok(r) => r,
                    Err(e) => crate::immich::SyncReport { errors: vec![e.to_string()], ..Default::default() },
                };
                {
                    let c = self.db.write();
                    let label = crate::immich::report_label(&report);
                    c.execute(
                        "INSERT INTO batches (kind, label, state, created_at, applied_at, summary) VALUES ('immich', ?1, 'applied', ?2, ?2, ?3)",
                        params![label, crate::db::now_ms(), serde_json::to_string(&report)?],
                    )?;
                }
                self.emit(LibEvent::Changed("assets"));
                self.emit(LibEvent::Changed("history"));
                self.emit(LibEvent::Changed("immich"));
                if let Some(first) = report.errors.first() {
                    anyhow::bail!("{} problems while syncing with Immich. First: {first}", report.errors.len());
                }
                Ok(())
            }
            JobSpec::Recover => {
                if journal::recover(&self.db, &self.tools())? > 0 {
                    self.emit(LibEvent::Changed("assets"));
                }
                Ok(())
            }
            JobSpec::Apply { batch } => {
                let report = journal::apply(&self.db, &self.tools(), *batch, &|done, total| ctx.progress(done, total))?;
                self.emit(LibEvent::Changed("assets"));
                self.emit(LibEvent::Changed("history"));
                self.emit(LibEvent::Changed("imports"));
                if !report.errors.is_empty() {
                    anyhow::bail!(
                        "{} of {} changes could not be made. First: {}",
                        report.failed,
                        report.failed + report.done,
                        report.errors[0]
                    );
                }
                Ok(())
            }
            JobSpec::Undo { batch } => {
                let report = journal::undo(&self.db, &self.tools(), *batch, &|done, total| ctx.progress(done, total))?;
                self.emit(LibEvent::Changed("assets"));
                self.emit(LibEvent::Changed("history"));
                self.emit(LibEvent::Changed("imports"));
                if !report.errors.is_empty() {
                    anyhow::bail!("{} changes could not be undone. First: {}", report.failed, report.errors[0]);
                }
                Ok(())
            }
        }
    }

    fn run_hash(&self, ctx: &JobCtx) -> anyhow::Result<()> {
        let todo: Vec<(i64, PathBuf)> = {
            let c = self.db.read();
            let mut st = c.prepare(
                // Exact copies share size and quick hash: only those need a
                // full read.
                "SELECT a.id, r.path, a.rel_path FROM assets a JOIN roots r ON r.id = a.root_id
                 WHERE a.blake3 IS NULL AND a.missing != 1
                   AND EXISTS (SELECT 1 FROM assets b WHERE b.size = a.size AND b.quick_hash = a.quick_hash AND b.id != a.id AND b.missing != 1)",
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

    /// The query with the words the search parser understands turned into
    /// filters.
    fn expand(&self, q: &Query) -> Query {
        if q.text.as_deref().is_none_or(|t| t.trim().is_empty()) {
            return q.clone();
        }
        let gaz = self.gaz();
        crate::search::expand(&self.db.read(), q, gaz.as_deref(), chrono::Local::now().date_naive())
    }

    pub fn parse_search(&self, text: &str) -> crate::search::ParsedSearch {
        let gaz = self.gaz();
        crate::search::parse(&self.db.read(), text, gaz.as_deref(), chrono::Local::now().date_naive())
    }

    /// Refresh the search index rows of items that changed.
    fn reindex(&self, ids: &[i64]) {
        // Big changes (a first scan) are cheaper as one rebuild in the background.
        if ids.len() > 2000 {
            let _ = self.jobs.enqueue(JobSpec::Index);
            return;
        }
        let gaz = self.gaz();
        let place = |la: f64, lo: f64| gaz.as_ref().and_then(|g| g.reverse(la, lo).map(|e| e.label()));
        let all = crate::trash::with_groups(&self.db.read(), ids).unwrap_or_else(|_| ids.to_vec());
        if let Err(e) = crate::search::reindex(&self.db.write(), Some(&all), &place) {
            tracing::warn!("search index: {e}");
        }
    }

    pub fn timeline(&self, q: &Query) -> anyhow::Result<Vec<u8>> {
        let q = self.expand(q);
        Ok(query::timeline(&self.db.read(), &q)?)
    }

    pub fn ids(&self, q: &Query) -> anyhow::Result<Vec<i64>> {
        let q = self.expand(q);
        Ok(query::ids(&self.db.read(), &q)?)
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
        drop(st);
        drop(c);
        if let (Some(lat), Some(lon)) = (d.lat, d.lon) {
            d.place = self.place_name(lat, lon);
        }
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
            duplicates: match self.dupe_count.load(std::sync::atomic::Ordering::Relaxed) {
                n if n >= 0 => n as i32,
                _ => one(&format!(
                    "SELECT count(*) FROM assets WHERE {live} AND blake3 IS NOT NULL AND blake3 IN (SELECT blake3 FROM assets WHERE missing = 0 AND trashed_at IS NULL AND blake3 IS NOT NULL GROUP BY blake3 HAVING count(*) > 1)"
                ))?,
            },
        })
    }

    pub fn facets(&self, q: &Query) -> anyhow::Result<Facets> {
        let q = &self.expand(q);
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

impl Library {
    // Edits, plans and history

    pub fn edit(&self, ids: &[i64], action: &EditAction) -> anyhow::Result<usize> {
        let (_, n) = self.db.tx(|tx| {
            let out = actions::apply(tx, ids, action)?;
            if matches!(action, EditAction::Category { .. } | EditAction::Accept) {
                crate::review::record_audits(tx, ids)?;
            }
            Ok(out)
        })?;
        self.reindex(ids);
        if matches!(action, EditAction::Rotate { .. }) && n > 0 {
            self.jobs.enqueue(JobSpec::Thumbs)?;
        }
        self.emit(LibEvent::Changed("assets"));
        Ok(n)
    }

    pub fn set_trashed(&self, ids: &[i64], trashed: bool) -> anyhow::Result<()> {
        self.db.tx(|tx| crate::trash::set_trashed(tx, ids, trashed))?;
        self.emit(LibEvent::Changed("assets"));
        Ok(())
    }

    /// Send trashed items to the system Recycle Bin (all when `ids` is None).
    pub fn empty_trash(&self, ids: Option<&[i64]>) -> anyhow::Result<usize> {
        let n = crate::trash::empty(&self.db.write(), ids)?;
        for id in ids.unwrap_or(&[]) {
            let _ = self.thumbs.remove(*id);
        }
        self.emit(LibEvent::Changed("assets"));
        Ok(n)
    }

    pub fn plan(&self, opts: &PlanOptions) -> anyhow::Result<PlanSummary> {
        let s = planner::plan(&self.db.write(), opts)?;
        self.emit(LibEvent::Changed("history"));
        Ok(s)
    }

    pub fn plan_csv(&self, batch: i64) -> anyhow::Result<String> {
        Ok(planner::csv(&self.db.read(), batch)?)
    }

    pub fn apply(&self, batch: i64) -> anyhow::Result<()> {
        if self.exiftool.is_none() {
            let writes: i64 =
                self.db.read().query_row("SELECT count(*) FROM ops WHERE batch_id = ?1 AND kind = 'write_meta'", [batch], |r| r.get(0))?;
            if writes > 0 {
                anyhow::bail!(
                    "ExifTool is needed to save metadata, and it was not found. Install it or turn off saving metadata for this plan."
                );
            }
        }
        self.jobs.enqueue(JobSpec::Apply { batch })?;
        Ok(())
    }

    /// Undo a batch: edits at once, file changes as a job.
    pub fn undo(&self, batch: i64) -> anyhow::Result<()> {
        let (kind, state): (String, String) =
            self.db.read().query_row("SELECT kind, state FROM batches WHERE id = ?1", [batch], |r| Ok((r.get(0)?, r.get(1)?)))?;
        if state != "applied" {
            anyhow::bail!("Only applied changes can be undone.");
        }
        match kind.as_str() {
            "organize" | "import" => {
                self.jobs.enqueue(JobSpec::Undo { batch })?;
            }
            "empty_trash" => anyhow::bail!("Items sent to the Recycle Bin can be restored from the Recycle Bin."),
            "immich" => anyhow::bail!("A sync is not undone from History. Change what goes to Immich, then sync again."),
            _ => {
                journal::undo(&self.db, &self.tools(), batch, &|_, _| true)?;
                // An undone rotation needs its thumbnails again.
                self.jobs.enqueue(JobSpec::Thumbs)?;
                self.emit(LibEvent::Changed("assets"));
                self.emit(LibEvent::Changed("history"));
            }
        }
        Ok(())
    }

    // Dates

    pub fn date_sets(&self) -> anyhow::Result<Vec<DateSet>> {
        Ok(crate::datefix::sets(&self.db.read(), crate::db::now_ms())?)
    }

    pub fn date_preview(&self, fix: &DateFix) -> anyhow::Result<DatePreview> {
        crate::datefix::preview(&self.db.read(), fix, &self.tz)
    }

    /// Apply a date fix. Returns how many items changed.
    pub fn date_apply(&self, fix: &DateFix) -> anyhow::Result<usize> {
        let (_, n) = self.db.tx_any(|tx| crate::datefix::apply(tx, fix, &self.tz))?;
        self.reindex(&fix.ids);
        self.emit(LibEvent::Changed("assets"));
        self.emit(LibEvent::Changed("history"));
        Ok(n)
    }

    pub fn date_candidates(&self, ids: &[i64]) -> anyhow::Result<Vec<crate::datefix::Candidate>> {
        Ok(crate::datefix::candidates(&self.db.read(), ids)?)
    }

    /// Hide a suggestion ("place") or a date set ("date").
    pub fn dismiss(&self, kind: &str, key: &str) -> anyhow::Result<()> {
        crate::places::dismiss(&self.db.write(), kind, key)?;
        Ok(())
    }

    // Places

    fn gaz(&self) -> Option<Arc<Gazetteer>> {
        let mut g = self.gazetteer.lock();
        if g.is_none() {
            let path = self.data_dir.join("geodata").join("places.tsv");
            *g = Some(Gazetteer::load(&path).ok().map(Arc::new));
        }
        g.clone().flatten()
    }

    pub fn places_info(&self) -> PlacesInfo {
        match self.gaz() {
            Some(g) => PlacesInfo { installed: true, count: g.len() as i32 },
            None => PlacesInfo { installed: false, count: 0 },
        }
    }

    /// Build the place names from GeoNames files in `dir` (`cities500.txt`
    /// or `cities500.zip`, `admin1CodesASCII.txt`, `countryInfo.txt`).
    pub fn install_places(&self, dir: &Path) -> anyhow::Result<PlacesInfo> {
        use std::io::{BufReader, Read};
        let open = |n: &str| -> anyhow::Result<BufReader<std::fs::File>> {
            Ok(BufReader::new(std::fs::File::open(dir.join(n)).map_err(|e| anyhow::anyhow!("{n}: {e}"))?))
        };
        let cities: Box<dyn std::io::BufRead> = if dir.join("cities500.txt").exists() {
            Box::new(open("cities500.txt")?)
        } else {
            let mut zip = zip::ZipArchive::new(open("cities500.zip")?).map_err(|e| anyhow::anyhow!("cities500.zip: {e}"))?;
            let mut text = Vec::new();
            zip.by_name("cities500.txt").map_err(|e| anyhow::anyhow!("cities500.zip: {e}"))?.read_to_end(&mut text)?;
            Box::new(std::io::Cursor::new(text))
        };
        let g = roll_geo::gazetteer::build(cities, open("admin1CodesASCII.txt")?, open("countryInfo.txt")?)?;
        anyhow::ensure!(!g.is_empty(), "These files hold no places; expected the GeoNames cities500 list");
        let out = self.data_dir.join("geodata");
        std::fs::create_dir_all(&out)?;
        g.save(&out.join("places.tsv"))?;
        let n = g.len();
        *self.gazetteer.lock() = Some(Some(Arc::new(g)));
        self.place_cache.lock().clear();
        self.emit(LibEvent::Changed("places"));
        // Place names are part of the search index.
        let _ = self.jobs.enqueue(JobSpec::Index);
        Ok(PlacesInfo { installed: true, count: n as i32 })
    }

    pub fn packs(&self) -> Vec<crate::packs::PackStatus> {
        self.packs.list()
    }

    /// Download a pack. The owner has confirmed it in the UI; downloads must
    /// also be on in Settings › Network.
    pub fn download_pack(self: &Arc<Self>, id: &str) -> anyhow::Result<()> {
        anyhow::ensure!(self.settings().network.downloads, "Downloads are off. Turn them on in Settings › Network.");
        let def = self.packs.catalog().get(id).ok_or_else(|| anyhow::anyhow!("Unknown pack {id}"))?;
        let after: Option<crate::packs::AfterStep> = match def.after.as_deref() {
            Some("places") => {
                let me = Arc::downgrade(self);
                Some(Box::new(move |dir: &Path| match me.upgrade() {
                    Some(lib) => lib.install_places(dir).map(|_| ()),
                    None => Ok(()),
                }))
            }
            _ => None,
        };
        self.packs.download(id, after)
    }

    pub fn cancel_pack(&self, id: &str) {
        self.packs.cancel(id);
    }

    pub fn remove_pack(&self, id: &str) -> anyhow::Result<()> {
        self.packs.remove(id)?;
        if self.packs.catalog().get(id).and_then(|d| d.after.as_deref()) == Some("places") {
            let _ = std::fs::remove_file(self.data_dir.join("geodata").join("places.tsv"));
            *self.gazetteer.lock() = Some(None);
            self.place_cache.lock().clear();
            self.emit(LibEvent::Changed("places"));
            let _ = self.jobs.enqueue(JobSpec::Index);
        }
        Ok(())
    }

    pub fn place_search(&self, q: &str) -> Vec<Hit> {
        let mut out = Vec::new();
        if let Some((lat, lon)) = roll_geo::coords::parse(q) {
            let label = self.gaz().and_then(|g| g.reverse(lat, lon).map(|e| e.label())).unwrap_or_else(|| format!("{lat:.5}, {lon:.5}"));
            out.push(Hit {
                label,
                name: format!("{lat:.5}, {lon:.5}"),
                kind: roll_geo::gazetteer::Kind::City,
                lat,
                lon,
                accuracy_m: 10.0,
                cc: String::new(),
            });
        }
        if let Some(g) = self.gaz() {
            out.extend(g.search(q, 12));
        }
        out
    }

    /// The place name at a point, if the gazetteer knows one.
    pub fn place_name(&self, lat: f64, lon: f64) -> Option<String> {
        self.gaz().and_then(|g| g.reverse(lat, lon).map(|e| e.label()))
    }

    pub fn tracks(&self) -> anyhow::Result<Vec<TrackInfo>> {
        Ok(crate::places::tracks(&self.db.read())?)
    }

    pub fn import_track(&self, path: &Path) -> anyhow::Result<TrackInfo> {
        let t = crate::places::import_track(&self.db.write(), path)?;
        self.emit(LibEvent::Changed("places"));
        Ok(t)
    }

    pub fn delete_track(&self, id: i64) -> anyhow::Result<()> {
        crate::places::delete_track(&self.db.write(), id)?;
        self.emit(LibEvent::Changed("places"));
        Ok(())
    }

    pub fn place_groups(&self) -> anyhow::Result<Vec<PlaceGroup>> {
        let window = self.settings().interpolation_minutes as i64;
        let gaz = self.gaz();
        let (groups, by_key) = crate::places::suggest(&self.db.read(), gaz.as_deref(), window)?;
        *self.place_cache.lock() = by_key;
        Ok(groups)
    }

    /// Accept a suggestion group. Returns how many items got a place and the
    /// time zones to offer for those without one.
    pub fn accept_places(&self, key: &str) -> anyhow::Result<(usize, Vec<ZoneProposal>)> {
        let items = self
            .place_cache
            .lock()
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("This suggestion is out of date. Refresh the list."))?;
        let ids: Vec<i64> = items.iter().map(|s| s.id).collect();
        let source = match items.first().map(|s| s.source) {
            Some("track") => "from location history",
            Some("neighbors") => "from your photos",
            Some("event") => "from the event",
            _ => "from the folder name",
        };
        let label = format!("Set the location of {} {source}", plural(items.len()));
        self.db.tx(|tx| crate::places::accept(tx, &items, &label))?;
        self.place_cache.lock().remove(key);
        self.reindex(&ids);
        let zones = crate::places::zone_proposals(&self.db.read(), &ids)?;
        self.emit(LibEvent::Changed("assets"));
        self.emit(LibEvent::Changed("history"));
        Ok((ids.len(), zones))
    }

    /// Set a place by hand on a selection; `None` clears it.
    pub fn set_location(&self, ids: &[i64], place: Option<Place>, name: Option<&str>) -> anyhow::Result<(usize, Vec<ZoneProposal>)> {
        let items = plural(ids.len());
        let label = match (&place, name) {
            (None, _) => format!("Clear the location of {items}"),
            (Some(_), Some(n)) => format!("Set the location of {items} to {n}"),
            (Some(p), None) => format!("Set the location of {items} to {:.4}, {:.4}", p.lat, p.lon),
        };
        let why = name.map(|n| format!("{n}, set by you."));
        let (_, n) = self.db.tx(|tx| crate::places::set_location(tx, ids, place, &label, why.clone()))?;
        self.reindex(ids);
        let zones = if place.is_some() { crate::places::zone_proposals(&self.db.read(), ids)? } else { Vec::new() };
        self.emit(LibEvent::Changed("assets"));
        self.emit(LibEvent::Changed("history"));
        Ok((n, zones))
    }

    /// State a time zone offset for items that had none.
    pub fn apply_zone(&self, ids: &[i64], offset_min: i32, zone: &str) -> anyhow::Result<usize> {
        let label = format!("Set the time zone of {} to {zone}", plural(ids.len()));
        let why = format!("Time zone {zone} ({}) from the place.", crate::places::offset_label(offset_min));
        let n = self.db.tx(|tx| {
            let batch = crate::edits::begin_batch(tx, "edit", &label)?;
            let mut n = 0;
            for id in ids {
                if crate::places::set_offset(tx, batch, *id, offset_min, &why)? {
                    n += 1;
                }
            }
            Ok(n)
        })?;
        self.emit(LibEvent::Changed("assets"));
        self.emit(LibEvent::Changed("history"));
        Ok(n)
    }

    // Review

    pub fn review_summary(&self) -> anyhow::Result<crate::review::ReviewSummary> {
        Ok(crate::review::summary(&self.db.write())?)
    }

    pub fn review_items(&self, key: &str, limit: i64) -> anyhow::Result<Vec<crate::review::ReviewItem>> {
        Ok(crate::review::items(&self.db.read(), key, limit)?)
    }

    pub fn accept_above(&self, key: &str, min: f32) -> anyhow::Result<usize> {
        let n = self.db.tx(|tx| crate::review::accept_above(tx, key, min))?;
        self.emit(LibEvent::Changed("assets"));
        self.emit(LibEvent::Changed("history"));
        Ok(n)
    }

    // Discover

    pub fn discover(&self, split: Option<&str>) -> anyhow::Result<Vec<crate::discover::Cluster>> {
        Ok(crate::discover::clusters(&self.db.read(), split)?)
    }

    pub fn discover_ignore(&self, key: &str) -> anyhow::Result<()> {
        crate::discover::ignore(&self.db.write(), key)?;
        Ok(())
    }

    // Events

    /// Detect again, then list proposed and confirmed events.
    pub fn events(&self) -> anyhow::Result<Vec<crate::events::EventInfo>> {
        let home = self.settings().home.filter(|h| h.len() == 2).map(|h| (h[0], h[1]));
        let gaz = self.gaz();
        let place = |pts: &[(f64, f64)]| gaz.as_deref().and_then(|g| crate::events::common_place(pts, g));
        self.db.tx(|tx| crate::events::refresh(tx, home, &place))?;
        Ok(crate::events::list(&self.db.read())?)
    }

    /// One event action as an undoable batch; refreshes what shows tags.
    fn event_action<T>(&self, f: impl FnOnce(&rusqlite::Transaction) -> rusqlite::Result<T>, ids: &[i64]) -> anyhow::Result<T> {
        let out = self.db.tx_any(|tx| f(tx).map_err(anyhow::Error::from))?;
        self.reindex(ids);
        self.emit(LibEvent::Changed("assets"));
        self.emit(LibEvent::Changed("history"));
        Ok(out)
    }

    fn event_items(&self, ids: &[i64]) -> Vec<i64> {
        let c = self.db.read();
        ids.iter().flat_map(|id| crate::events::assets_of(&c, *id).unwrap_or_default()).collect()
    }

    pub fn event_confirm(&self, id: i64, name: Option<&str>) -> anyhow::Result<()> {
        let items = self.event_items(&[id]);
        self.event_action(|tx| crate::events::confirm(tx, id, name), &items)?;
        Ok(())
    }

    pub fn event_rename(&self, id: i64, name: &str) -> anyhow::Result<()> {
        anyhow::ensure!(!name.trim().is_empty(), "An event needs a name");
        let items = self.event_items(&[id]);
        self.event_action(|tx| crate::events::rename(tx, id, name), &items)?;
        Ok(())
    }

    pub fn event_dismiss(&self, id: i64) -> anyhow::Result<()> {
        let items = self.event_items(&[id]);
        self.event_action(|tx| crate::events::dismiss(tx, id), &items)?;
        Ok(())
    }

    pub fn event_merge(&self, ids: &[i64]) -> anyhow::Result<()> {
        anyhow::ensure!(ids.len() >= 2, "Choose at least two events to merge");
        let items = self.event_items(ids);
        self.event_action(|tx| crate::events::merge(tx, ids), &items)?;
        Ok(())
    }

    /// Returns false when `at` is the first item (nothing to split off).
    pub fn event_split(&self, id: i64, at: i64, name: &str) -> anyhow::Result<bool> {
        let items = self.event_items(&[id]);
        Ok(self.event_action(|tx| crate::events::split(tx, id, at, name), &items)?.is_some())
    }

    pub fn event_own_folder(&self, id: i64, on: bool) -> anyhow::Result<()> {
        crate::events::set_own_folder(&self.db.write(), id, on)?;
        Ok(())
    }

    // Add photos

    /// Start (or reopen) an import of a folder outside the library. It is
    /// indexed read-only while the owner checks it.
    pub fn import_start(&self, path: &Path) -> anyhow::Result<i64> {
        let path = std::path::absolute(path)?;
        anyhow::ensure!(path.is_dir(), "{} is not a folder.", path.display());
        let (id, root) = crate::import::start(&self.db.write(), &path)?;
        self.jobs.enqueue(JobSpec::Scan { root_id: root })?;
        self.emit(LibEvent::Changed("imports"));
        Ok(id)
    }

    pub fn imports(&self) -> anyhow::Result<Vec<ImportInfo>> {
        let ids = crate::import::list_ids(&self.db.read())?;
        ids.into_iter().map(|id| self.import_info(id)).collect()
    }

    pub fn import_info(&self, id: i64) -> anyhow::Result<ImportInfo> {
        let gaz = self.gaz();
        let place = |pts: &[(f64, f64)]| gaz.as_deref().and_then(|g| crate::events::common_place(pts, g));
        // The write connection: missing BLAKE3 hashes are stored on the way.
        crate::import::info(&self.db.write(), id, crate::db::now_ms(), &place)
    }

    pub fn import_name(&self, id: i64, name: Option<&str>) -> anyhow::Result<()> {
        crate::import::set_name(&self.db.write(), id, name)?;
        self.emit(LibEvent::Changed("imports"));
        Ok(())
    }

    pub fn import_mark(&self, id: i64, dates_ok: Option<bool>, places_ok: Option<bool>) -> anyhow::Result<()> {
        crate::import::mark(&self.db.write(), id, dates_ok, places_ok)?;
        self.emit(LibEvent::Changed("imports"));
        Ok(())
    }

    pub fn import_leave_out(&self, id: i64, ids: &[i64], out: bool) -> anyhow::Result<()> {
        crate::import::leave_out(&self.db.write(), id, ids, out)?;
        self.emit(LibEvent::Changed("imports"));
        Ok(())
    }

    /// Place suggestions for an import's items; accept them with
    /// `accept_places` like any other group.
    pub fn import_places(&self, id: i64) -> anyhow::Result<Vec<PlaceGroup>> {
        let root: i64 = self.db.read().query_row("SELECT root_id FROM imports WHERE id = ?1", [id], |r| r.get(0))?;
        let window = self.settings().interpolation_minutes as i64;
        let gaz = self.gaz();
        let (groups, by_key) = crate::places::suggest_in(&self.db.read(), gaz.as_deref(), window, crate::model::Scope::Staged(root))?;
        self.place_cache.lock().extend(by_key);
        Ok(groups)
    }

    pub fn import_plan(&self, id: i64, opts: &ImportOptions) -> anyhow::Result<ImportPlan> {
        let info = self.import_info(id)?;
        crate::import::preview(&self.db.read(), id, opts, &info.name, self.exiftool.is_some())
    }

    /// Copy or move the import into the archive as one undoable batch.
    pub fn import_apply(&self, id: i64, opts: &ImportOptions) -> anyhow::Result<i64> {
        let info = self.import_info(id)?;
        let batch = self.db.tx_any(|tx| crate::import::begin(tx, id, opts, &info.name, info.place.as_deref(), self.exiftool.is_some()))?;
        self.jobs.enqueue(JobSpec::Apply { batch })?;
        self.emit(LibEvent::Changed("imports"));
        self.emit(LibEvent::Changed("history"));
        Ok(batch)
    }

    /// Close an import; the files in the source stay where they are.
    pub fn import_close(&self, id: i64) -> anyhow::Result<()> {
        let ids = crate::import::close(&self.db.write(), id)?;
        for a in ids {
            let _ = self.thumbs.remove(a);
        }
        self.emit(LibEvent::Changed("imports"));
        Ok(())
    }

    // Immich

    fn immich_client(&self) -> anyhow::Result<crate::immich::Client> {
        let s = self.settings();
        anyhow::ensure!(s.network.immich, "Immich is off. Turn it on in Settings › Network.");
        let url = s
            .immich_url
            .filter(|u| !u.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("Add your Immich server address in Settings › Immich."))?;
        let key = crate::secrets::get("immich").ok_or_else(|| anyhow::anyhow!("Add an Immich API key in Settings › Immich."))?;
        Ok(crate::immich::Client::new(&url, &key))
    }

    /// Whether Immich is set up, and (when it is on) what the server says.
    pub fn immich_status(&self) -> crate::immich::ImmichStatus {
        let s = self.settings();
        let mut st = crate::immich::ImmichStatus {
            on: s.network.immich,
            url: s.immich_url.clone(),
            has_key: crate::secrets::has("immich"),
            server: None,
            user: None,
            error: None,
        };
        if st.on && st.url.is_some() && st.has_key {
            match self.immich_client().and_then(|c| Ok((c.version()?, c.me()?))) {
                Ok((v, me)) => {
                    st.server = Some(v);
                    st.user = Some(me.name);
                }
                Err(e) => st.error = Some(e.to_string()),
            }
        }
        st
    }

    pub fn immich_plan(&self) -> anyhow::Result<crate::immich::ImmichPlan> {
        Ok(crate::immich::plan(&self.db.read())?)
    }

    /// Start a sync. The owner has seen the plan; `remove` confirms moving
    /// items to Immich's trash.
    pub fn immich_sync(&self, remove: bool) -> anyhow::Result<()> {
        self.immich_client()?;
        self.jobs.enqueue(JobSpec::Immich { remove })?;
        Ok(())
    }

    pub fn immich_users(&self) -> anyhow::Result<Vec<crate::immich::ImmichUser>> {
        let c = self.immich_client()?;
        let me = c.me()?;
        Ok(c.users()?.into_iter().filter(|u| u.id != me.id).collect())
    }

    pub fn immich_albums(&self) -> anyhow::Result<Vec<crate::immich::AlbumInfo>> {
        Ok(crate::immich::album_list(&self.db.read())?)
    }

    /// Whether a confirmed event becomes an album, and who sees it.
    pub fn immich_album(&self, event: i64, send: bool, share: &[String]) -> anyhow::Result<()> {
        self.db.write().execute(
            "UPDATE events SET immich = ?2, immich_share = ?3 WHERE id = ?1",
            params![event, if send { "default" } else { "local" }, serde_json::to_string(share)?],
        )?;
        self.emit(LibEvent::Changed("immich"));
        Ok(())
    }

    /// An Immich CLI script for the selection, for when the API sync is not
    /// an option.
    pub fn immich_script(&self, windows: bool) -> anyhow::Result<String> {
        Ok(crate::immich::cli_script(&self.db.read(), windows)?)
    }

    // Duplicates

    pub fn dupe_groups(&self) -> anyhow::Result<Vec<crate::dupes::DupeGroup>> {
        let g = crate::dupes::groups(&self.db.read())?;
        let n: usize = g.iter().map(|g| g.ids.len()).sum();
        self.dupe_count.store(n as i64, std::sync::atomic::Ordering::Relaxed);
        Ok(g)
    }

    /// Keep one item, send the others to Roll's trash. Returns how many.
    pub fn merge_dupes(&self, keep: i64, others: &[i64]) -> anyhow::Result<usize> {
        let (_, n) = self.db.tx(|tx| crate::dupes::merge(tx, keep, others))?;
        self.reindex(&[keep]);
        self.emit(LibEvent::Changed("assets"));
        self.emit(LibEvent::Changed("history"));
        Ok(n)
    }

    pub fn map_points(&self, q: &Query) -> anyhow::Result<Vec<u8>> {
        let q = self.expand(q);
        Ok(query::map_points(&self.db.read(), &q)?)
    }

    /// Undo the most recent applied batch. Returns its label.
    pub fn undo_last(&self) -> anyhow::Result<Option<String>> {
        let last: Option<(i64, String)> = self
            .db
            .read()
            .query_row(
                "SELECT id, label FROM batches WHERE state = 'applied' AND kind != 'empty_trash'
                 ORDER BY coalesce(applied_at, created_at) DESC, id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match last {
            Some((id, label)) => {
                self.undo(id)?;
                Ok(Some(label))
            }
            None => Ok(None),
        }
    }

    pub fn history(&self, limit: i64) -> anyhow::Result<Vec<BatchInfo>> {
        let c = self.db.read();
        let mut st = c.prepare(
            "SELECT b.id, b.kind, b.label, b.state, b.created_at, b.applied_at, b.summary,
                    (SELECT count(*) FROM ops o WHERE o.batch_id = b.id) + (SELECT count(*) FROM field_history h WHERE h.batch_id = b.id),
                    (SELECT count(*) FROM ops o WHERE o.batch_id = b.id AND o.state = 'failed')
             FROM batches b WHERE b.state != 'planned' ORDER BY b.id DESC LIMIT ?1",
        )?;
        let rows = st.query_map([limit], |r| {
            Ok(BatchInfo {
                id: r.get(0)?,
                kind: r.get(1)?,
                label: r.get(2)?,
                state: r.get(3)?,
                created_at: r.get(4)?,
                applied_at: r.get(5)?,
                summary: r.get(6)?,
                ops: r.get(7)?,
                failed: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Items with changes not yet written to their files.
    pub fn unsaved(&self) -> anyhow::Result<i64> {
        Ok(self.db.read().query_row(
            "SELECT count(*) FROM assets WHERE meta_dirty = 1 AND missing = 0 AND trashed_at IS NULL",
            [],
            |r| r.get(0),
        )?)
    }

    // Categories and rules

    pub fn add_category(&self, name: &str, parent: Option<&str>, definition: &str) -> anyhow::Result<String> {
        let key = categories::add(&self.db.write(), name, parent, definition)?;
        self.emit(LibEvent::Changed("categories"));
        Ok(key)
    }

    pub fn update_category(&self, key: &str, patch: &categories::CategoryPatch) -> anyhow::Result<()> {
        categories::update(&self.db.write(), key, patch)?;
        self.emit(LibEvent::Changed("categories"));
        Ok(())
    }

    pub fn delete_category(&self, key: &str) -> anyhow::Result<()> {
        categories::delete(&self.db.write(), key)?;
        self.emit(LibEvent::Changed("categories"));
        Ok(())
    }

    pub fn rules(&self) -> anyhow::Result<Vec<crate::rules::Rule>> {
        Ok(crate::rules::load(&self.db.read())?)
    }

    pub fn save_rule(&self, rule: &crate::rules::Rule) -> anyhow::Result<i64> {
        crate::rules::validate(&rule.def)?;
        let def = serde_json::to_string(&rule.def)?;
        let id = {
            let c = self.db.write();
            if rule.id > 0 {
                c.execute(
                    "UPDATE rules SET name = ?2, enabled = ?3, priority = ?4, def = ?5 WHERE id = ?1",
                    params![rule.id, rule.name, rule.enabled, rule.priority, def],
                )?;
                rule.id as i64
            } else {
                c.execute(
                    "INSERT INTO rules (name, enabled, priority, def) VALUES (?1, ?2, ?3, ?4)",
                    params![rule.name, rule.enabled, rule.priority, def],
                )?;
                c.last_insert_rowid()
            }
        };
        self.jobs.enqueue(JobSpec::Classify)?;
        Ok(id)
    }

    pub fn delete_rule(&self, id: i64) -> anyhow::Result<()> {
        let builtin: bool =
            self.db.read().query_row("SELECT builtin FROM rules WHERE id = ?1", [id], |r| r.get::<_, i64>(0).map(|b| b != 0))?;
        if builtin {
            anyhow::bail!("Built-in rules can be turned off but not deleted.");
        }
        self.db.write().execute("DELETE FROM rules WHERE id = ?1", [id])?;
        self.jobs.enqueue(JobSpec::Classify)?;
        Ok(())
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

fn plural(n: usize) -> String {
    if n == 1 { "1 item".into() } else { format!("{n} items") }
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
