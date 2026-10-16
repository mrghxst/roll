//! Offline map areas (PROMPT M7): the tiles of an area copied out of the
//! newest Protomaps build (OpenStreetMap data, ODbL) with range requests
//! and kept in `<data>/maps/<id>.db`. An area goes down to zoom 15, every
//! building and footpath, when that fits in [`MAX_TILES`], else as deep as
//! fits but at least to 13 (streets and larger buildings). Roll serves the
//! tiles to the map itself and enlarges an area's deepest tiles for the
//! zooms it lacks, up to 15; the map enlarges past that. Nothing is
//! fetched while the owner looks at the map.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use roll_geo::pmtiles::{self, Entry, Header};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const BUILDS: &str = "https://build-metadata.protomaps.dev/builds.json";
pub const BUILD_BASE: &str = "https://build.protomaps.com/";
/// The deepest zoom of the Protomaps builds.
pub const MAX_ZOOM: u8 = 15;
/// Areas too big for zoom 15 still go this deep.
pub const MIN_ZOOM: u8 = 13;
/// About Switzerland twice over at zoom 13 (about 1 GB); more would take
/// hours.
pub const MAX_TILES: u64 = 20_000;
/// Average stored bytes per tile, for estimates: 52 KB around Lake Garda,
/// 58 KB in central Lisbon down to zoom 15 (docs/benchmarks.md).
pub const BYTES_PER_TILE: u64 = 50_000;
pub const ATTRIBUTION: &str = "© OpenStreetMap contributors, Protomaps";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MapArea {
    pub id: String,
    pub name: String,
    /// West, south, east, north.
    pub bbox: Vec<f64>,
    pub max_zoom: u8,
    pub tiles: i32,
    pub bytes: f64,
    /// "ready", "downloading" or "failed".
    pub state: String,
    /// The Protomaps build it comes from (a date).
    pub build: String,
    pub error: Option<String>,
}

/// What the owner asked for, kept next to the download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub name: String,
    pub bbox: [f64; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MapEstimate {
    pub tiles: i32,
    pub bytes: f64,
    pub too_big: bool,
    /// How deep the area goes: 15 has every building.
    pub max_zoom: u8,
}

/// The deepest zoom an area fits in at, from [`MIN_ZOOM`] to [`MAX_ZOOM`].
pub fn zoom_for(bbox: [f64; 4]) -> u8 {
    (MIN_ZOOM..=MAX_ZOOM).rev().find(|&z| pmtiles::tile_count(bbox, z) <= MAX_TILES).unwrap_or(MIN_ZOOM)
}

pub fn estimate(bbox: [f64; 4]) -> MapEstimate {
    let z = zoom_for(bbox);
    let tiles = pmtiles::tile_count(bbox, z);
    MapEstimate {
        tiles: tiles.min(i32::MAX as u64) as i32,
        bytes: (tiles * BYTES_PER_TILE) as f64,
        too_big: tiles > MAX_TILES,
        max_zoom: z,
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_body(Some(Duration::from_secs(120)))
        .user_agent(concat!("Roll/", env!("CARGO_PKG_VERSION"), " (offline map areas)"))
        .build()
        .into()
}

/// The newest build's file name, like `20261015.pmtiles`.
pub fn latest_build() -> anyhow::Result<String> {
    let mut r = agent().get(BUILDS).call()?;
    anyhow::ensure!(r.status() == 200, "The map server answered {}", r.status());
    let builds: Vec<serde_json::Value> = r.body_mut().read_json()?;
    builds
        .iter()
        .filter_map(|b| b.get("key").and_then(|k| k.as_str()))
        .filter(|k| k.ends_with(".pmtiles"))
        .max()
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("The map server lists no builds"))
}

/// A remote archive read with range requests; directories are cached.
struct Remote {
    agent: ureq::Agent,
    url: String,
    header: Header,
    root: Vec<Entry>,
    leaves: HashMap<u64, Vec<Entry>>,
    fetched: u64,
}

impl Remote {
    fn open(url: &str) -> anyhow::Result<Self> {
        let agent = agent();
        let first = range(&agent, url, 0, 16_384)?;
        let header = pmtiles::parse_header(&first)?;
        anyhow::ensure!(header.tile_type == 1, "The map build does not hold vector tiles");
        let root_bytes = if header.root_offset + header.root_length <= first.len() as u64 {
            first[header.root_offset as usize..(header.root_offset + header.root_length) as usize].to_vec()
        } else {
            range(&agent, url, header.root_offset, header.root_length)?
        };
        let root = pmtiles::parse_directory(&pmtiles::decompress(&root_bytes, header.internal_compression)?)?;
        Ok(Remote { agent, url: url.into(), header, root, leaves: HashMap::new(), fetched: first.len() as u64 })
    }

    /// Where a tile's bytes are (absolute offset and length), if it has any.
    fn locate(&mut self, id: u64) -> anyhow::Result<Option<(u64, u32)>> {
        let mut dir = self.root.clone();
        for _ in 0..4 {
            let Some(e) = pmtiles::find(&dir, id) else { return Ok(None) };
            if e.run_length > 0 {
                return Ok(Some((self.header.data_offset + e.offset, e.length)));
            }
            let at = self.header.leaf_offset + e.offset;
            if !self.leaves.contains_key(&at) {
                let raw = range(&self.agent, &self.url, at, u64::from(e.length))?;
                self.fetched += raw.len() as u64;
                let leaf = pmtiles::parse_directory(&pmtiles::decompress(&raw, self.header.internal_compression)?)?;
                self.leaves.insert(at, leaf);
            }
            dir = self.leaves[&at].clone();
        }
        Ok(None)
    }
}

fn range(agent: &ureq::Agent, url: &str, offset: u64, len: u64) -> anyhow::Result<Vec<u8>> {
    let mut tries = 0;
    loop {
        let r = agent.get(url).header("Range", &format!("bytes={}-{}", offset, offset + len - 1)).call();
        match r {
            Ok(mut r) if r.status() == 206 || r.status() == 200 => {
                let body = r.body_mut().with_config().limit(64 * 1024 * 1024).read_to_vec()?;
                // A server that ignores ranges sends everything: take the part.
                if r.status() == 200 && body.len() as u64 > len {
                    return Ok(body[offset as usize..(offset + len) as usize].to_vec());
                }
                return Ok(body);
            }
            Ok(r) if tries < 3 && r.status().as_u16() >= 500 => {}
            Ok(r) => anyhow::bail!("The map server answered {}", r.status()),
            Err(_) if tries < 3 => {}
            Err(e) => return Err(e.into()),
        }
        tries += 1;
        std::thread::sleep(Duration::from_secs(2u64.pow(tries)));
    }
}

fn open_db(path: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(path)?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS tiles (z INTEGER NOT NULL, x INTEGER NOT NULL, y INTEGER NOT NULL, data BLOB NOT NULL, PRIMARY KEY (z, x, y)) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    Ok(c)
}

/// Copy an area's tiles into `<dir>/<id>.db`. `progress` gets tiles done
/// and in all, and stops the download when it returns false (a later call
/// starts over). Returns the stored bytes.
pub fn download(dir: &Path, id: &str, req: &Request, progress: &dyn Fn(usize, usize) -> bool) -> anyhow::Result<u64> {
    std::fs::create_dir_all(dir)?;
    let build = latest_build()?;
    let mut remote = Remote::open(&format!("{BUILD_BASE}{build}"))?;
    let max_zoom = zoom_for(req.bbox).min(remote.header.max_zoom);
    // Where every tile of the area is in the archive.
    let mut wanted: Vec<(u8, u32, u32, u64, u32)> = Vec::new();
    for z in 0..=max_zoom {
        let ((x0, x1), (y0, y1)) = pmtiles::tile_range(req.bbox, z);
        for x in x0..=x1 {
            for y in y0..=y1 {
                if let Some((off, len)) = remote.locate(pmtiles::tile_id(z, x, y))? {
                    wanted.push((z, x, y, off, len));
                }
            }
        }
    }
    wanted.sort_by_key(|w| w.3);
    let part = dir.join(format!("{id}.db.part"));
    let _ = std::fs::remove_file(&part);
    let mut c = open_db(&part)?;
    let total = wanted.len();
    // Neighbouring tiles in one request: gaps under 512 KB (cheaper than
    // another round trip), up to 8 MB.
    let mut chunks: Vec<(usize, usize, u64, u64)> = Vec::new();
    let mut i = 0;
    while i < wanted.len() {
        let start = wanted[i].3;
        let mut j = i + 1;
        let mut end = start + u64::from(wanted[i].4);
        while j < wanted.len() && wanted[j].3 <= end + (512 << 10) && wanted[j].3 + u64::from(wanted[j].4) - start <= 8 << 20 {
            end = end.max(wanted[j].3 + u64::from(wanted[j].4));
            j += 1;
        }
        chunks.push((i, j, start, end));
        i = j;
    }
    // Six requests at a time; the tiles are written here as they arrive.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let stop = std::sync::atomic::AtomicBool::new(false);
    let (tx_chunk, rx_chunk) = std::sync::mpsc::channel::<anyhow::Result<(usize, Vec<u8>)>>();
    let (agent, url) = (remote.agent.clone(), remote.url.clone());
    let mut done = 0;
    let mut bytes = 0u64;
    let mut fetched = 0u64;
    let outcome: anyhow::Result<()> = std::thread::scope(|sc| {
        for _ in 0..6 {
            let (tx_chunk, next, stop, chunks, agent, url) = (tx_chunk.clone(), &next, &stop, &chunks, &agent, &url);
            sc.spawn(move || {
                loop {
                    if stop.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&(_, _, start, end)) = chunks.get(k) else { return };
                    let r = range(agent, url, start, end - start).map(|b| (k, b));
                    let failed = r.is_err();
                    if tx_chunk.send(r).is_err() || failed {
                        return;
                    }
                }
            });
        }
        drop(tx_chunk);
        let mut result = Ok(());
        for got in rx_chunk {
            let (k, chunk) = match got {
                Ok(v) => v,
                Err(e) => {
                    stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    result = Err(e);
                    break;
                }
            };
            let (a, b, start, _) = chunks[k];
            fetched += chunk.len() as u64;
            let step = (|| -> anyhow::Result<()> {
                let tx = c.transaction()?;
                {
                    let mut ins = tx.prepare_cached("INSERT OR REPLACE INTO tiles (z, x, y, data) VALUES (?1, ?2, ?3, ?4)")?;
                    for &(z, x, y, off, len) in &wanted[a..b] {
                        let at = (off - start) as usize;
                        let raw = chunk.get(at..at + len as usize).ok_or_else(|| anyhow::anyhow!("The map server sent less than asked"))?;
                        let tile = pmtiles::decompress(raw, remote.header.tile_compression)?;
                        bytes += tile.len() as u64;
                        ins.execute(params![z, x, y, tile])?;
                    }
                }
                tx.commit()?;
                Ok(())
            })();
            if let Err(e) = step {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                result = Err(e);
                break;
            }
            done += b - a;
            if !progress(done, total) {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                result = Err(anyhow::anyhow!("stopped"));
                break;
            }
        }
        result
    });
    remote.fetched += fetched;
    if let Err(e) = outcome {
        drop(c);
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    let meta = [
        ("name", req.name.clone()),
        ("bbox", serde_json::to_string(&req.bbox)?),
        ("max_zoom", max_zoom.to_string()),
        ("build", build.trim_end_matches(".pmtiles").to_string()),
        ("tiles", total.to_string()),
        ("bytes", bytes.to_string()),
        ("fetched", remote.fetched.to_string()),
        ("attribution", ATTRIBUTION.into()),
    ];
    for (k, v) in meta {
        c.execute("INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)", params![k, v])?;
    }
    drop(c);
    std::fs::rename(&part, dir.join(format!("{id}.db")))?;
    tracing::info!("map area {id}: {total} tiles, {bytes} bytes, {} fetched", remote.fetched);
    Ok(bytes)
}

/// The areas in `dir`, ready ones from their files, pending ones from
/// their requests.
pub fn list(dir: &Path, running: &[String], failed: &HashMap<String, String>) -> Vec<MapArea> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    files.sort();
    for p in files {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if let Some(id) = name.strip_suffix(".db") {
            if let Some(a) = read_area(&p, id) {
                out.push(a);
            }
        } else if let Some(id) = name.strip_suffix(".json")
            && !dir.join(format!("{id}.db")).exists()
            && let Some(req) = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<Request>(&b).ok())
        {
            let est = estimate(req.bbox);
            let state = if running.iter().any(|r| r == id) { "downloading" } else { "failed" };
            out.push(MapArea {
                id: id.into(),
                name: req.name,
                bbox: req.bbox.to_vec(),
                max_zoom: est.max_zoom,
                tiles: est.tiles,
                bytes: est.bytes,
                state: state.into(),
                build: String::new(),
                error: failed.get(id).cloned().or_else(|| (state == "failed").then(|| "The download stopped. Start it again.".into())),
            });
        }
    }
    out
}

fn read_area(path: &Path, id: &str) -> Option<MapArea> {
    let c = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let get = |k: &str| c.query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| r.get::<_, String>(0)).optional().ok().flatten();
    Some(MapArea {
        id: id.into(),
        name: get("name")?,
        bbox: serde_json::from_str(&get("bbox")?).ok()?,
        max_zoom: get("max_zoom")?.parse().ok()?,
        tiles: get("tiles")?.parse().ok()?,
        bytes: std::fs::metadata(path).map(|m| m.len() as f64).unwrap_or(0.0),
        state: "ready".into(),
        build: get("build").unwrap_or_default(),
        error: None,
    })
}

/// Glyphs for place names (Noto Sans, OFL), Latin, Greek and Cyrillic,
/// from the Protomaps assets; fetched with the first area.
const FONT_BASE: &str = "https://raw.githubusercontent.com/protomaps/basemaps-assets/028c18f713baecad011301ff7a69acc39bcc2ae7/fonts/";
pub const FONT: &str = "Noto Sans Regular";
const FONT_FILES: [(&str, &str); 7] = [
    ("Noto Sans Regular/0-255.pbf", "62c6d49b15fa836eb6aa45e259c7ca6762f44b011b09e47776efbe4a6db1b397"),
    ("Noto Sans Regular/256-511.pbf", "2eca7561f9f566bcacfda5dd04fb5880baec1328ec0f5484678289a13994de8a"),
    ("Noto Sans Regular/512-767.pbf", "6abc80badad0e823e228ab739515be257c8b33bc68bb077dd873030e9b2d143a"),
    ("Noto Sans Regular/768-1023.pbf", "537ccbee79180f4f8b3f6d2bdd27df979260ce1a0358c40d7aa7db505d9b03aa"),
    ("Noto Sans Regular/1024-1279.pbf", "302231023f7048d9694a7b3f3b737c8bc378f5af0175aed5bb4f2f3b6188919a"),
    ("Noto Sans Regular/8192-8447.pbf", "8ea977a587352fe31b4159ffdbc9a40be79056f2472017c742ea1e4a931864b9"),
    ("OFL.txt", "7713cfc8e3c36d5ec4aa3d6cffe7500a1b3310f8a86d914b8ea09c2a9dee7c2d"),
];

/// Fetch the glyphs unless they are there; each file is checked.
pub fn ensure_fonts(dir: &Path) -> anyhow::Result<()> {
    use sha2::Digest;
    let agent = agent();
    for (rel, sha) in FONT_FILES {
        let path = dir.join("fonts").join(rel);
        if path.exists() {
            continue;
        }
        let url = format!("{FONT_BASE}{}", rel.replace(' ', "%20"));
        let mut r = agent.get(&url).call()?;
        anyhow::ensure!(r.status() == 200, "The font server answered {}", r.status());
        let bytes = r.body_mut().read_to_vec()?;
        let got = hex(&sha2::Sha256::digest(&bytes));
        anyhow::ensure!(got == sha, "A map font did not match its checksum");
        std::fs::create_dir_all(path.parent().unwrap_or(dir))?;
        std::fs::write(&path, bytes)?;
    }
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A glyph range, if fetched (`fontstack` is the font's name).
pub fn glyphs(dir: &Path, fontstack: &str, range: &str) -> Option<Vec<u8>> {
    // The map asks for "a,b" stacks; Roll has one font.
    let font = fontstack.split(',').map(str::trim).find(|f| *f == FONT)?;
    let ok = range.len() < 16 && range.chars().all(|c| c.is_ascii_digit() || c == '-');
    if !ok {
        return None;
    }
    std::fs::read(dir.join("fonts").join(font).join(format!("{range}.pbf"))).ok()
}

/// Open tile stores, for serving.
#[derive(Default)]
pub struct Tiles {
    open: Vec<(String, [f64; 4], u8, Connection)>,
}

impl Tiles {
    pub fn load(dir: &Path) -> Self {
        let mut open = Vec::new();
        for a in list(dir, &[], &HashMap::new()).into_iter().filter(|a| a.state == "ready") {
            let Ok(c) = Connection::open_with_flags(dir.join(format!("{}.db", a.id)), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) else {
                continue;
            };
            let bbox = [a.bbox[0], a.bbox[1], a.bbox[2], a.bbox[3]];
            open.push((a.id, bbox, a.max_zoom, c));
        }
        Tiles { open }
    }

    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// A tile (uncompressed MVT) from the first area that has it, else
    /// enlarged from the deepest tile above it that an area has.
    pub fn get(&self, z: u8, x: u32, y: u32) -> Option<Vec<u8>> {
        let read = |c: &Connection, z: u8, x: u32, y: u32| -> Option<Vec<u8>> {
            c.query_row("SELECT data FROM tiles WHERE z = ?1 AND x = ?2 AND y = ?3", params![z, x, y], |r| r.get(0))
                .optional()
                .ok()
                .flatten()
        };
        if let Some(t) = self.open.iter().find_map(|(_, _, _, c)| read(c, z, x, y)) {
            return Some(t);
        }
        if z > MAX_ZOOM {
            return None;
        }
        let mut best: Option<(u8, Vec<u8>)> = None;
        for (_, _, max, c) in &self.open {
            if *max >= z || best.as_ref().is_some_and(|(b, _)| b >= max) {
                continue;
            }
            let dz = z - max;
            if let Some(t) = read(c, *max, x >> dz, y >> dz) {
                best = Some((*max, t));
            }
        }
        let (max, parent) = best?;
        let dz = z - max;
        let mask = (1u32 << dz) - 1;
        roll_geo::mvt::overzoom(&parent, dz, x & mask, y & mask).map_err(|e| tracing::warn!("map tile {z}/{x}/{y}: {e}")).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_and_lists() {
        let ch = estimate([5.9, 45.8, 10.5, 47.8]);
        assert!(!ch.too_big && ch.tiles > 5_000 && ch.max_zoom == 13);
        assert!(estimate([-125.0, 24.0, -66.0, 49.0]).too_big, "the whole United States is too much");
        // A city has every building; a region in between stops at 14.
        let zurich = estimate([8.45, 47.32, 8.62, 47.43]);
        assert_eq!((zurich.max_zoom, zurich.too_big), (15, false));
        assert!(zurich.bytes < 100e6, "{}", zurich.bytes);
        assert_eq!(estimate([10.5, 45.5, 11.0, 46.0]).max_zoom, 15);
        assert_eq!(estimate([8.0, 46.5, 9.5, 47.6]).max_zoom, 14);

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // A finished area and one that is still downloading.
        let c = open_db(&dir.join("ch.db")).unwrap();
        for (k, v) in [("name", "Switzerland"), ("bbox", "[5.9,45.8,10.5,47.8]"), ("max_zoom", "13"), ("tiles", "2"), ("build", "20261015")]
        {
            c.execute("INSERT INTO meta (key, value) VALUES (?1, ?2)", params![k, v]).unwrap();
        }
        c.execute("INSERT INTO tiles (z, x, y, data) VALUES (0, 0, 0, x'1a00')", []).unwrap();
        c.execute("INSERT INTO tiles (z, x, y, data) VALUES (13, 4264, 2881, x'1a00')", []).unwrap();
        drop(c);
        std::fs::write(
            dir.join("lis.json"),
            serde_json::to_vec(&Request { name: "Lisbon".into(), bbox: [-9.3, 38.6, -9.0, 38.8] }).unwrap(),
        )
        .unwrap();
        let areas = list(dir, &["lis".to_string()], &HashMap::new());
        assert_eq!(
            areas.iter().map(|a| (a.name.as_str(), a.state.as_str())).collect::<Vec<_>>(),
            vec![("Switzerland", "ready"), ("Lisbon", "downloading")]
        );
        let tiles = Tiles::load(dir);
        assert_eq!(tiles.get(0, 0, 0), Some(vec![0x1a, 0x00]));
        assert_eq!(tiles.get(1, 0, 0), None);
        // Zoom 14 and 15 come from the area's zoom 13 tile above them.
        assert_eq!(tiles.get(14, 8529, 5763), Some(vec![]));
        assert_eq!(tiles.get(15, 17059, 11527), Some(vec![]));
        assert_eq!(tiles.get(14, 8531, 5763), None);
        assert_eq!(tiles.get(16, 34118, 23054), None);
    }
}
