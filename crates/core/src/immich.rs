//! Immich (4.10): the curated part of the archive goes to the owner's
//! Immich server through its API (v3.x). What goes is decided per item
//! (`assets.immich`), by category default and by confirmed events. A sync
//! uploads new items once (checksums are checked first), pushes later
//! edits, puts events into albums and, after the owner confirms, moves
//! items switched to "Keep local only" to Immich's trash. It never touches
//! local files. Progress is kept per item (`immich_id`, `immich_synced`),
//! so an interrupted sync continues where it stopped.

use std::collections::{BTreeSet, HashMap};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use rusqlite::{Connection, OptionalExtension, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use crate::db::Db;
use crate::paths::join_rel;
use crate::query::IMMICH_SELECTED;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImmichUser {
    pub id: String,
    pub name: String,
    pub email: String,
}

/// A file to upload.
pub struct Upload<'a> {
    pub file: &'a Path,
    pub name: &'a str,
    pub sidecar: Option<&'a Path>,
    /// ISO 8601 instants.
    pub created: String,
    pub modified: String,
    pub favorite: bool,
    pub live_video: Option<&'a str>,
    pub visibility: Option<&'a str>,
    /// SHA-1, base64.
    pub checksum: &'a str,
}

/// The Immich API, authenticated with an API key.
pub struct Client {
    base: String,
    key: String,
    agent: ureq::Agent,
}

type Resp = ureq::http::Response<ureq::Body>;

impl Client {
    /// `url` is the server address as the owner types it; `/api` is added.
    pub fn new(url: &str, key: &str) -> Self {
        let mut base = url.trim().trim_end_matches('/').to_string();
        if !base.ends_with("/api") {
            base.push_str("/api");
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(600)))
            .build()
            .into();
        Client { base, key: key.trim().to_string(), agent }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn ok(r: Result<Resp, ureq::Error>) -> anyhow::Result<Resp> {
        let r = r.map_err(|e| anyhow::anyhow!("Immich could not be reached: {e}"))?;
        let status = r.status().as_u16();
        if status < 300 {
            return Ok(r);
        }
        let body = r.into_body().read_to_string().unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v.get("message").map(|m| m.as_str().map(str::to_string).unwrap_or_else(|| m.to_string())))
            .unwrap_or(body);
        match status {
            401 => anyhow::bail!("Immich did not accept the API key."),
            403 => anyhow::bail!("The API key lacks a permission Roll needs ({msg}). The Immich guide lists them."),
            _ => anyhow::bail!("Immich answered {status}: {msg}"),
        }
    }

    fn json<T: DeserializeOwned>(r: Resp) -> anyhow::Result<T> {
        Ok(r.into_body().read_json::<T>()?)
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        Self::json(Self::ok(self.agent.get(self.url(path)).header("x-api-key", &self.key).call())?)
    }

    fn send(&self, method: &str, path: &str, body: &Value) -> anyhow::Result<Resp> {
        let url = self.url(path);
        let r = match method {
            "POST" => self.agent.post(url).header("x-api-key", &self.key).send_json(body),
            "PUT" => self.agent.put(url).header("x-api-key", &self.key).send_json(body),
            "PATCH" => self.agent.patch(url).header("x-api-key", &self.key).send_json(body),
            _ => self.agent.delete(url).header("x-api-key", &self.key).force_send_body().send_json(body),
        };
        Self::ok(r)
    }

    /// "3.3.1".
    pub fn version(&self) -> anyhow::Result<String> {
        let v: Value = self.get("/server/version")?;
        Ok(format!("{}.{}.{}", v["major"], v["minor"], v["patch"]))
    }

    pub fn me(&self) -> anyhow::Result<ImmichUser> {
        self.get("/users/me")
    }

    pub fn users(&self) -> anyhow::Result<Vec<ImmichUser>> {
        self.get("/users")
    }

    /// Which files the server already has: client id → Immich asset id.
    /// Roll asks only about files it wants in Immich, so any of them in
    /// Immich's trash (an earlier "Keep local only") are restored.
    pub fn existing(&self, items: &[(String, String)]) -> anyhow::Result<HashMap<String, String>> {
        let assets: Vec<Value> = items.iter().map(|(id, sum)| json!({"id": id, "checksum": sum})).collect();
        let v: Value = Self::json(self.send("POST", "/assets/bulk-upload-check", &json!({ "assets": assets }))?)?;
        let dups: Vec<&Value> =
            v["results"].as_array().into_iter().flatten().filter(|r| r["action"] == "reject" && r["reason"] == "duplicate").collect();
        let trashed: Vec<&str> = dups.iter().filter(|r| r["isTrashed"] == true).filter_map(|r| r["assetId"].as_str()).collect();
        if !trashed.is_empty() {
            self.send("POST", "/trash/restore/assets", &json!({ "ids": trashed }))?;
        }
        Ok(dups.iter().filter_map(|r| Some((r["id"].as_str()?.to_string(), r["assetId"].as_str()?.to_string()))).collect())
    }

    /// Upload one file (with its sidecar). Returns the asset id.
    pub fn upload(&self, u: &Upload) -> anyhow::Result<String> {
        let boundary =
            format!("roll{:016x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
        let field = |name: &str, value: &str| format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n");
        let quoted = u.name.replace('"', "'");
        let mut head = String::new();
        head += &field("fileCreatedAt", &u.created);
        head += &field("fileModifiedAt", &u.modified);
        head += &field("filename", u.name);
        head += &field("isFavorite", if u.favorite { "true" } else { "false" });
        if let Some(v) = u.live_video {
            head += &field("livePhotoVideoId", v);
        }
        if let Some(v) = u.visibility {
            head += &field("visibility", v);
        }
        head += &format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"assetData\"; filename=\"{quoted}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        );
        let file = std::fs::File::open(u.file)?;
        let mut len = head.len() as u64 + file.metadata()?.len();
        let mut reader: Box<dyn Read + Send> = Box::new(Cursor::new(head.into_bytes()).chain(file));
        if let Some(side) = u.sidecar.filter(|s| s.exists()) {
            let part = format!(
                "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"sidecarData\"; filename=\"{quoted}.xmp\"\r\nContent-Type: application/xml\r\n\r\n"
            );
            let f = std::fs::File::open(side)?;
            len += part.len() as u64 + f.metadata()?.len();
            reader = Box::new(reader.chain(Cursor::new(part.into_bytes())).chain(f));
        }
        let tail = format!("\r\n--{boundary}--\r\n");
        len += tail.len() as u64;
        let reader = reader.chain(Cursor::new(tail.into_bytes()));
        let r = self
            .agent
            .post(self.url("/assets"))
            .header("x-api-key", &self.key)
            .header("x-immich-checksum", u.checksum)
            .header("content-type", format!("multipart/form-data; boundary={boundary}"))
            .header("content-length", len.to_string())
            .send(ureq::SendBody::from_owned_reader(reader));
        let v: Value = Self::json(Self::ok(r)?)?;
        v["id"].as_str().map(str::to_string).ok_or_else(|| anyhow::anyhow!("Immich returned no asset id"))
    }

    /// Change fields of assets. `PATCH /assets` replaces the `PUT` that v3
    /// deprecates; servers without it get the `PUT`.
    pub fn update(&self, ids: &[String], fields: &Value) -> anyhow::Result<()> {
        let mut body = fields.clone();
        body["ids"] = json!(ids);
        match self.send("PATCH", "/assets", &body) {
            Ok(_) => Ok(()),
            Err(e) if e.to_string().contains("answered 404") || e.to_string().contains("answered 405") => {
                self.send("PUT", "/assets", &body).map(|_| ())
            }
            Err(e) => Err(e),
        }
    }

    /// Create missing tags ("A/B" makes a hierarchy). Returns path → id.
    pub fn upsert_tags(&self, names: &[String]) -> anyhow::Result<HashMap<String, String>> {
        if names.is_empty() {
            return Ok(HashMap::new());
        }
        let v: Vec<Value> = Self::json(self.send("PUT", "/tags", &json!({ "tags": names }))?)?;
        Ok(v.iter().filter_map(|t| Some((t["value"].as_str()?.to_string(), t["id"].as_str()?.to_string()))).collect())
    }

    pub fn tag(&self, tag: &str, ids: &[String]) -> anyhow::Result<()> {
        self.send("PUT", &format!("/tags/{tag}/assets"), &json!({ "ids": ids })).map(|_| ())
    }

    pub fn untag(&self, tag: &str, ids: &[String]) -> anyhow::Result<()> {
        self.send("DELETE", &format!("/tags/{tag}/assets"), &json!({ "ids": ids })).map(|_| ())
    }

    /// A new album with its first assets, shared with `users` as viewers.
    pub fn create_album(&self, name: &str, ids: &[String], users: &[String]) -> anyhow::Result<String> {
        let album_users: Vec<Value> = users.iter().map(|u| json!({"userId": u, "role": "viewer"})).collect();
        let v: Value =
            Self::json(self.send("POST", "/albums", &json!({"albumName": name, "assetIds": ids, "albumUsers": album_users}))?)?;
        v["id"].as_str().map(str::to_string).ok_or_else(|| anyhow::anyhow!("Immich returned no album id"))
    }

    /// The users an album is shared with, or `None` when it is gone.
    pub fn album_users(&self, id: &str) -> anyhow::Result<Option<Vec<String>>> {
        let r = self.agent.get(self.url(&format!("/albums/{id}?withoutAssets=true"))).header("x-api-key", &self.key).call();
        if let Ok(resp) = &r
            && matches!(resp.status().as_u16(), 400 | 404)
        {
            return Ok(None);
        }
        let v: Value = Self::json(Self::ok(r)?)?;
        Ok(Some(v["albumUsers"].as_array().into_iter().flatten().filter_map(|u| u["user"]["id"].as_str().map(str::to_string)).collect()))
    }

    pub fn add_to_album(&self, id: &str, ids: &[String]) -> anyhow::Result<()> {
        self.send("PUT", &format!("/albums/{id}/assets"), &json!({ "ids": ids })).map(|_| ())
    }

    pub fn share_album(&self, id: &str, users: &[String]) -> anyhow::Result<()> {
        let list: Vec<Value> = users.iter().map(|u| json!({"userId": u, "role": "viewer"})).collect();
        self.send("PUT", &format!("/albums/{id}/users"), &json!({ "albumUsers": list })).map(|_| ())
    }

    /// Move assets to Immich's trash (not a permanent delete).
    pub fn trash(&self, ids: &[String]) -> anyhow::Result<()> {
        self.send("DELETE", "/assets", &json!({"ids": ids, "force": false})).map(|_| ())
    }
}

/// What Roll last pushed for an asset; a sync updates what differs.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Pushed {
    pub date: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub description: Option<String>,
    pub rating: i64,
    pub favorite: bool,
    pub visibility: String,
    pub tags: Vec<String>,
}

struct Wanted {
    id: i64,
    immich_id: Option<String>,
    synced: Option<Pushed>,
    /// As stored: compared as text, since parsed floats may differ in the
    /// last bit.
    synced_raw: Option<String>,
    state: Pushed,
}

impl Wanted {
    fn in_sync(&self) -> bool {
        self.synced_raw.as_deref() == serde_json::to_string(&self.state).ok().as_deref()
    }
}

/// `2022-05-20T08:07:35+01:00`: local time with its offset (stated, or the
/// one Roll assumed when it read the file).
fn date_string(local: Option<&str>, offset: Option<i64>, utc: Option<i64>) -> Option<String> {
    let local = local?;
    let l = chrono::NaiveDateTime::parse_from_str(local, "%Y-%m-%dT%H:%M:%S").ok()?;
    let off = offset.or_else(|| utc.map(|u| (l.and_utc().timestamp_millis() - u) / 60_000))?;
    let sign = if off < 0 { '-' } else { '+' };
    Some(format!("{local}{sign}{:02}:{:02}", off.abs() / 60, off.abs() % 60))
}

/// The items Immich should have (primary items), with their state.
fn wanted(c: &Connection) -> rusqlite::Result<Vec<Wanted>> {
    let mut st = c.prepare(&format!(
        "SELECT a.id, a.immich_id, a.immich_synced, a.taken_local, a.tz_offset, a.taken_utc, a.lat, a.lon, a.description, a.rating,
                a.favorite, a.private,
                (SELECT group_concat(t.path, char(31)) FROM asset_tags at JOIN tags t ON t.id = at.tag_id WHERE at.asset_id = a.id)
         FROM assets a
         WHERE a.missing = 0 AND a.trashed_at IS NULL AND (a.group_role IS NULL OR a.group_role = 'primary') AND {IMMICH_SELECTED} = 1
         ORDER BY a.taken_utc, a.id"
    ))?;
    st.query_map([], |r| {
        let mut tags: Vec<String> =
            r.get::<_, Option<String>>(12)?.map(|s| s.split('\u{1f}').map(str::to_string).collect()).unwrap_or_default();
        tags.sort();
        let raw: Option<String> = r.get(2)?;
        Ok(Wanted {
            id: r.get(0)?,
            immich_id: r.get(1)?,
            synced: raw.as_deref().and_then(|s| serde_json::from_str(s).ok()),
            synced_raw: raw,
            state: Pushed {
                date: date_string(r.get::<_, Option<String>>(3)?.as_deref(), r.get(4)?, r.get(5)?),
                lat: r.get(6)?,
                lon: r.get(7)?,
                description: r.get(8)?,
                rating: r.get(9)?,
                favorite: r.get::<_, i64>(10)? != 0,
                visibility: if r.get::<_, i64>(11)? != 0 { "locked".into() } else { "timeline".into() },
                tags,
            },
        })
    })?
    .collect()
}

/// Items in Immich that should no longer be: switched to "Keep local only",
/// or in Roll's trash.
fn unwanted(c: &Connection) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut st = c.prepare(&format!(
        "SELECT a.id, a.immich_id FROM assets a
         WHERE a.immich_id IS NOT NULL AND (a.group_role IS NULL OR a.group_role = 'primary')
           AND (a.missing != 0 OR a.trashed_at IS NOT NULL OR {IMMICH_SELECTED} = 0)"
    ))?;
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
}

/// Confirmed events that become albums: (event, name, album id, shared with, items in Immich or about to be).
type AlbumRow = (i64, String, Option<String>, Vec<String>, Vec<i64>);

fn albums(c: &Connection) -> rusqlite::Result<Vec<AlbumRow>> {
    let events: Vec<(i64, String, Option<String>, String)> = {
        let mut st = c.prepare(
            "SELECT id, name, immich_album_id, immich_share FROM events WHERE state = 'confirmed' AND immich != 'local' ORDER BY start_utc",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?
    };
    let mut out = Vec::new();
    for (id, name, album, share) in events {
        let mut st = c.prepare_cached(&format!(
            "SELECT a.id FROM event_assets ea JOIN assets a ON a.id = ea.asset_id
             WHERE ea.event_id = ?1 AND a.missing = 0 AND a.trashed_at IS NULL AND (a.group_role IS NULL OR a.group_role = 'primary')
               AND {IMMICH_SELECTED} = 1 ORDER BY a.taken_utc"
        ))?;
        let items: Vec<i64> = st.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        if !items.is_empty() {
            out.push((id, name, album, serde_json::from_str(&share).unwrap_or_default(), items));
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImmichPlan {
    pub uploads: i32,
    #[ts(type = "number")]
    pub upload_bytes: i64,
    pub updates: i32,
    /// Items that leave Immich (to its trash) when the owner confirms.
    pub removes: i32,
    pub new_albums: i32,
    /// Albums that get items.
    pub album_updates: i32,
    pub upload_samples: Vec<i32>,
    pub remove_samples: Vec<i32>,
    /// Items already in Immich and up to date.
    pub in_sync: i32,
}

/// What a sync would do, from the database alone.
pub fn plan(c: &Connection) -> rusqlite::Result<ImmichPlan> {
    let want = wanted(c)?;
    let mut p = ImmichPlan::default();
    for w in &want {
        match (&w.immich_id, &w.synced) {
            (None, _) => {
                p.uploads += 1;
                if p.upload_samples.len() < 12 {
                    p.upload_samples.push(w.id as i32);
                }
            }
            (Some(_), Some(_)) if w.in_sync() => p.in_sync += 1,
            (Some(_), _) => p.updates += 1,
        }
    }
    let up: Vec<String> = want.iter().filter(|w| w.immich_id.is_none()).map(|w| w.id.to_string()).collect();
    if !up.is_empty() {
        p.upload_bytes =
            c.query_row(&format!("SELECT coalesce(sum(size), 0) FROM assets WHERE id IN ({})", up.join(",")), [], |r| r.get(0))?;
    }
    let gone = unwanted(c)?;
    p.removes = gone.len() as i32;
    p.remove_samples = gone.iter().take(12).map(|g| g.0 as i32).collect();
    for (_, _, album, _, _) in albums(c)? {
        if album.is_none() {
            p.new_albums += 1;
        } else {
            p.album_updates += 1;
        }
    }
    Ok(p)
}

/// What a sync did.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncReport {
    pub uploaded: i32,
    /// Files the server already had (linked, not uploaded again).
    pub linked: i32,
    pub updated: i32,
    pub removed: i32,
    pub albums: i32,
    pub errors: Vec<String>,
}

fn sha1_base64(path: &Path) -> std::io::Result<String> {
    use sha1::Digest;
    let mut f = std::fs::File::open(path)?;
    let mut h = sha1::Sha1::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(h.finalize()))
}

fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

struct FileInfo {
    path: PathBuf,
    name: String,
    sidecar: Option<PathBuf>,
    utc: Option<i64>,
    mtime: i64,
    favorite: bool,
    private: bool,
    dirty: bool,
    motion: Option<i64>,
}

fn file_info(c: &Connection, id: i64) -> rusqlite::Result<FileInfo> {
    let (root, rel, name, utc, mtime, favorite, private, dirty, group): (
        String,
        String,
        String,
        Option<i64>,
        i64,
        i64,
        i64,
        i64,
        Option<i64>,
    ) = c.query_row(
        "SELECT r.path, a.rel_path, a.name, a.taken_utc, a.mtime, a.favorite, a.private, a.meta_dirty, a.group_id
             FROM assets a JOIN roots r ON r.id = a.root_id WHERE a.id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
    )?;
    let root = PathBuf::from(root);
    let sidecar: Option<String> = c
        .query_row("SELECT rel_path FROM sidecars WHERE asset_id = ?1 AND kind = 'xmp' ORDER BY length(rel_path) DESC LIMIT 1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    let motion: Option<i64> = match group {
        Some(g) => c
            .query_row(
                "SELECT a.id FROM assets a JOIN groups gr ON gr.id = a.group_id WHERE a.group_id = ?1 AND gr.kind = 'live' AND a.group_role = 'motion' AND a.missing = 0",
                [g],
                |r| r.get(0),
            )
            .optional()?,
        None => None,
    };
    Ok(FileInfo {
        path: join_rel(&root, &rel),
        name,
        sidecar: sidecar.map(|s| join_rel(&root, &s)),
        utc,
        mtime,
        favorite: favorite != 0,
        private: private != 0,
        dirty: dirty != 0,
        motion,
    })
}

/// Progress: (done, total); false stops between items (the next sync
/// continues).
pub type Check<'a> = &'a dyn Fn(usize, usize) -> bool;

/// Upload one asset (and its Live Photo video first). Returns its Immich id
/// and whether it was already on the server.
fn push_file(db: &Db, client: &Client, id: i64, known: &HashMap<String, String>) -> anyhow::Result<(String, bool)> {
    if let Some(existing) = known.get(&id.to_string()) {
        // The server has the still; note its Live Photo video too, so it
        // follows the still later (for example to Immich's trash).
        let f = file_info(&db.read(), id)?;
        if let Some(m) = f.motion {
            let have: Option<String> = db.read().query_row("SELECT immich_id FROM assets WHERE id = ?1", [m], |r| r.get(0))?;
            if have.is_none() {
                let msum = sha1_base64(&file_info(&db.read(), m)?.path)?;
                if let Some(v) = client.existing(&[(m.to_string(), msum)])?.get(&m.to_string()) {
                    db.write().execute("UPDATE assets SET immich_id = ?2 WHERE id = ?1", params![m, v])?;
                }
            }
        }
        return Ok((existing.clone(), true));
    }
    let f = file_info(&db.read(), id)?;
    let sum = sha1_base64(&f.path)?;
    let mut live = None;
    if let Some(m) = f.motion {
        let have: Option<String> = db.read().query_row("SELECT immich_id FROM assets WHERE id = ?1", [m], |r| r.get(0))?;
        let vid = match have {
            Some(v) => v,
            None => {
                let mf = file_info(&db.read(), m)?;
                let msum = sha1_base64(&mf.path)?;
                let found = client.existing(&[(m.to_string(), msum.clone())])?;
                let v = match found.get(&m.to_string()) {
                    Some(v) => v.clone(),
                    None => client.upload(&Upload {
                        file: &mf.path,
                        name: &mf.name,
                        sidecar: None,
                        created: iso(mf.utc.unwrap_or(mf.mtime)),
                        modified: iso(mf.mtime),
                        favorite: false,
                        live_video: None,
                        visibility: Some("hidden"),
                        checksum: &msum,
                    })?,
                };
                db.write().execute("UPDATE assets SET immich_id = ?2 WHERE id = ?1", params![m, v])?;
                v
            }
        };
        live = Some(vid);
    }
    let up = Upload {
        file: &f.path,
        name: &f.name,
        sidecar: f.sidecar.as_deref(),
        created: iso(f.utc.unwrap_or(f.mtime)),
        modified: iso(f.mtime),
        favorite: f.favorite,
        live_video: live.as_deref(),
        visibility: f.private.then_some("locked"),
        checksum: &sum,
    };
    Ok((client.upload(&up)?, false))
}

/// Run a sync. `remove` is the owner's confirmation for moving items to
/// Immich's trash.
pub fn sync(db: &Db, client: &Client, remove: bool, check: Check) -> anyhow::Result<SyncReport> {
    let mut report = SyncReport::default();
    let want = wanted(&db.read())?;
    let todo: Vec<&Wanted> = want.iter().filter(|w| w.immich_id.is_none()).collect();
    let total = todo.len() + want.len();
    let mut done = 0usize;

    // 1. Uploads, after asking which files the server already has.
    for chunk in todo.chunks(100) {
        let sums: Vec<(String, String)> = chunk
            .iter()
            .filter_map(|w| {
                let f = file_info(&db.read(), w.id).ok()?;
                Some((w.id.to_string(), sha1_base64(&f.path).ok()?))
            })
            .collect();
        let known = client.existing(&sums)?;
        for w in chunk {
            if !check(done, total) {
                return Ok(report);
            }
            done += 1;
            match push_file(db, client, w.id, &known) {
                Ok((iid, dup)) => {
                    if dup {
                        report.linked += 1;
                    } else {
                        report.uploaded += 1;
                    }
                    // A file with saved metadata (and its sidecar) gives
                    // Immich the same state; otherwise the update step
                    // pushes it.
                    let dirty = file_info(&db.read(), w.id).map(|f| f.dirty).unwrap_or(true);
                    let synced = (!dirty && !dup).then(|| serde_json::to_string(&w.state).unwrap_or_default());
                    db.write().execute("UPDATE assets SET immich_id = ?2, immich_synced = ?3 WHERE id = ?1", params![w.id, iid, synced])?;
                }
                Err(e) => {
                    if report.errors.len() < 20 {
                        report.errors.push(format!("{}: {e}", w.id));
                    }
                    if e.to_string().contains("API key") || e.to_string().contains("could not be reached") {
                        return Err(e);
                    }
                }
            }
        }
    }

    // 2. Updates: what changed since the last push.
    let want = wanted(&db.read())?;
    let stale: Vec<&Wanted> = want.iter().filter(|w| w.immich_id.is_some() && !w.in_sync()).collect();
    let mut tag_names: BTreeSet<String> = BTreeSet::new();
    for w in &stale {
        tag_names.extend(w.state.tags.iter().cloned());
        if let Some(s) = &w.synced {
            tag_names.extend(s.tags.iter().cloned());
        }
    }
    let tag_ids = client.upsert_tags(&tag_names.into_iter().collect::<Vec<_>>())?;
    let mut add: HashMap<&str, Vec<String>> = HashMap::new();
    let mut drop: HashMap<&str, Vec<String>> = HashMap::new();
    for w in &stale {
        if !check(done, total) {
            return Ok(report);
        }
        done += 1;
        let iid = w.immich_id.clone().unwrap_or_default();
        let old = w.synced.clone().unwrap_or_default();
        let s = &w.state;
        let mut fields = serde_json::Map::new();
        if (w.synced.is_none() || s.date != old.date)
            && let Some(d) = &s.date
        {
            fields.insert("dateTimeOriginal".into(), json!(d));
        }
        let moved = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() > 1e-9,
            (a, b) => a.is_some() != b.is_some(),
        };
        if (w.synced.is_none() || moved(s.lat, old.lat) || moved(s.lon, old.lon))
            && let (Some(la), Some(lo)) = (s.lat, s.lon)
        {
            fields.insert("latitude".into(), json!(la));
            fields.insert("longitude".into(), json!(lo));
        }
        if w.synced.is_none() || s.description != old.description {
            fields.insert("description".into(), json!(s.description.clone().unwrap_or_default()));
        }
        if w.synced.is_none() || s.rating != old.rating {
            fields.insert("rating".into(), if s.rating > 0 { json!(s.rating) } else { Value::Null });
        }
        if w.synced.is_none() || s.favorite != old.favorite {
            fields.insert("isFavorite".into(), json!(s.favorite));
        }
        if w.synced.is_none() || s.visibility != old.visibility {
            fields.insert("visibility".into(), json!(s.visibility));
        }
        if !fields.is_empty()
            && let Err(e) = client.update(std::slice::from_ref(&iid), &Value::Object(fields))
        {
            report.errors.push(format!("{}: {e}", w.id));
            continue;
        }
        for t in &s.tags {
            if !old.tags.contains(t)
                && let Some(tid) = tag_ids.get(t)
            {
                add.entry(tid.as_str()).or_default().push(iid.clone());
            }
        }
        for t in &old.tags {
            if !s.tags.contains(t)
                && let Some(tid) = tag_ids.get(t)
            {
                drop.entry(tid.as_str()).or_default().push(iid.clone());
            }
        }
        report.updated += 1;
    }
    for (tid, ids) in &add {
        for chunk in ids.chunks(500) {
            client.tag(tid, chunk)?;
        }
    }
    for (tid, ids) in &drop {
        for chunk in ids.chunks(500) {
            client.untag(tid, chunk)?;
        }
    }
    {
        let c = db.write();
        for w in &stale {
            c.execute("UPDATE assets SET immich_synced = ?2 WHERE id = ?1", params![w.id, serde_json::to_string(&w.state)?])?;
        }
    }

    // 3. Albums for confirmed events.
    for (event, name, album, share, items) in albums(&db.read())? {
        let ids: Vec<String> = {
            let c = db.read();
            items
                .iter()
                .filter_map(|i| {
                    c.query_row("SELECT immich_id FROM assets WHERE id = ?1", [i], |r| r.get::<_, Option<String>>(0)).ok().flatten()
                })
                .collect()
        };
        if ids.is_empty() {
            continue;
        }
        let current = match &album {
            Some(a) => client.album_users(a)?.map(|u| (a.clone(), u)),
            None => None,
        };
        match current {
            Some((a, users)) => {
                for chunk in ids.chunks(1000) {
                    client.add_to_album(&a, chunk)?;
                }
                let missing: Vec<String> = share.iter().filter(|u| !users.contains(u)).cloned().collect();
                if !missing.is_empty() {
                    client.share_album(&a, &missing)?;
                }
            }
            None => {
                let (first, rest) = ids.split_at(ids.len().min(1000));
                let a = client.create_album(&name, first, &share)?;
                for chunk in rest.chunks(1000) {
                    client.add_to_album(&a, chunk)?;
                }
                db.write().execute("UPDATE events SET immich_album_id = ?2 WHERE id = ?1", params![event, a])?;
            }
        }
        report.albums += 1;
    }

    // 4. Items that should leave Immich, only with the owner's go-ahead.
    if remove {
        let gone = unwanted(&db.read())?;
        for chunk in gone.chunks(500) {
            let ids: Vec<String> = chunk.iter().map(|g| g.1.clone()).collect();
            client.trash(&ids)?;
            let c = db.write();
            for (id, _) in chunk {
                c.execute("UPDATE assets SET immich_id = NULL, immich_synced = NULL WHERE id = ?1", [id])?;
            }
            report.removed += chunk.len() as i32;
        }
    }
    Ok(report)
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImmichStatus {
    /// Settings › Network allows it.
    pub on: bool,
    pub url: Option<String>,
    pub has_key: bool,
    /// "3.3.1" when the server answered.
    pub server: Option<String>,
    /// The API key's user.
    pub user: Option<String>,
    pub error: Option<String>,
}

/// A confirmed event and its Immich album.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AlbumInfo {
    #[ts(type = "number")]
    pub event_id: i64,
    pub name: String,
    /// False when the owner keeps the event local.
    pub send: bool,
    /// Items that go to Immich.
    pub items: i32,
    pub album_id: Option<String>,
    /// User ids the album is shared with.
    pub share: Vec<String>,
}

pub fn album_list(c: &Connection) -> rusqlite::Result<Vec<AlbumInfo>> {
    let mut st = c.prepare(&format!(
        "SELECT e.id, e.name, e.immich != 'local', e.immich_album_id, e.immich_share,
                (SELECT count(*) FROM event_assets ea JOIN assets a ON a.id = ea.asset_id
                 WHERE ea.event_id = e.id AND a.missing = 0 AND a.trashed_at IS NULL AND (a.group_role IS NULL OR a.group_role = 'primary')
                   AND {IMMICH_SELECTED} = 1)
         FROM events e WHERE e.state = 'confirmed' ORDER BY e.start_utc DESC"
    ))?;
    st.query_map([], |r| {
        Ok(AlbumInfo {
            event_id: r.get(0)?,
            name: r.get(1)?,
            send: r.get(2)?,
            album_id: r.get(3)?,
            share: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
            items: r.get(5)?,
        })
    })?
    .collect()
}

/// An Immich CLI script for the selection (4.10): event folders become
/// albums with `--album-name`; other folders with selected items are sent
/// as they are (never `--album`, which would name albums after folders like
/// "Photos"). The API key is never written into the script.
pub fn cli_script(c: &Connection, windows: bool) -> rusqlite::Result<String> {
    // Folders with selected items, and the event that owns each event folder.
    let rows: Vec<(String, String, Option<String>)> = {
        let mut st = c.prepare(&format!(
            "SELECT DISTINCT r.path, a.dir,
                    (SELECT e.name FROM event_assets ea JOIN events e ON e.id = ea.event_id
                     WHERE ea.asset_id = a.id AND e.state = 'confirmed' AND e.own_folder = 1 AND e.immich != 'local')
             FROM assets a JOIN roots r ON r.id = a.root_id AND r.kind = 'library'
             WHERE a.missing = 0 AND a.trashed_at IS NULL AND {IMMICH_SELECTED} = 1
             ORDER BY r.path, a.dir"
        ))?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?
    };
    let mut seen = std::collections::HashSet::new();
    let quote = |s: &str| if windows { format!("'{}'", s.replace('\'', "''")) } else { format!("'{}'", s.replace('\'', "'\\''")) };
    let mut out = String::new();
    if windows {
        out += "# Roll: send the items chosen for Immich with the Immich CLI.\n";
        out += "# First: npm install -g @immich/cli, then: immich login <server address> <API key>\n";
        out += "# The CLI sends whole folders, sidecars included.\n$ErrorActionPreference = 'Stop'\n\n";
    } else {
        out += "#!/bin/sh\n# Roll: send the items chosen for Immich with the Immich CLI.\n";
        out += "# First: npm install -g @immich/cli, then: immich login <server address> <API key>\n";
        out += "# The CLI sends whole folders, sidecars included.\nset -e\n\n";
    }
    for (root, dir, event) in rows {
        let path = join_rel(Path::new(&root), &dir).to_string_lossy().into_owned();
        if !seen.insert(path.clone()) {
            continue;
        }
        match event {
            Some(name) => out += &format!("immich upload --album-name {} {}\n", quote(&name), quote(&path)),
            None => out += &format!("immich upload {}\n", quote(&path)),
        }
    }
    Ok(out)
}

/// The label a sync gets in History.
pub fn report_label(r: &SyncReport) -> String {
    let mut parts = Vec::new();
    if r.uploaded + r.linked > 0 {
        parts.push(format!("sent {}", r.uploaded + r.linked));
    }
    if r.updated > 0 {
        parts.push(format!("updated {}", r.updated));
    }
    if r.removed > 0 {
        parts.push(format!("moved {} to Immich's trash", r.removed));
    }
    if r.albums > 0 {
        parts.push(format!("{} album{}", r.albums, if r.albums == 1 { "" } else { "s" }));
    }
    if parts.is_empty() { "Immich: nothing to change".into() } else { format!("Immich: {}", parts.join(", ")) }
}
