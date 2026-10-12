//! Immich sync against a small stand-in for the Immich v3 API (the
//! endpoints Roll uses, with their request shapes from the v3.3.1 OpenAPI
//! spec). The real round trip against Immich in Docker is `e2e/immich`.
//!
//! Uses fixture files (Live Photos, phone photos, screenshots); skipped
//! when `fixtures-out/` has not been generated.

use std::collections::{BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use parking_lot::Mutex;
use roll_core::actions::EditAction;
use roll_core::immich::{self, Client};
use roll_core::library::Library;
use serde_json::{Value, json};
use sha1::Digest;

#[derive(Debug, Default, Clone)]
struct Asset {
    name: String,
    sum: String,
    favorite: bool,
    visibility: String,
    live: Option<String>,
    date: Option<String>,
    description: Option<String>,
    rating: Option<i64>,
    tags: BTreeSet<String>,
    trashed: bool,
}

#[derive(Debug, Default)]
struct Album {
    name: String,
    assets: BTreeSet<String>,
    users: BTreeSet<String>,
}

#[derive(Default)]
struct State {
    assets: HashMap<String, Asset>,
    tags: HashMap<String, String>,
    albums: HashMap<String, Album>,
    uploads: usize,
    seq: usize,
}

impl State {
    fn id(&mut self) -> String {
        self.seq += 1;
        format!("00000000-0000-4000-8000-{:012}", self.seq)
    }
    fn tag_value(&self, id: &str) -> Option<String> {
        self.tags.iter().find(|(_, v)| *v == id).map(|(k, _)| k.clone())
    }
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_request(r: &mut BufReader<TcpStream>) -> Option<Request> {
    let mut line = String::new();
    if r.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next()?.to_string(), parts.next()?.to_string());
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        r.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }
    let mut body = Vec::new();
    if let Some(n) = headers.get("content-length").and_then(|v| v.parse::<usize>().ok()) {
        body.resize(n, 0);
        r.read_exact(&mut body).ok()?;
    } else if headers.get("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
        loop {
            let mut size = String::new();
            r.read_line(&mut size).ok()?;
            let n = usize::from_str_radix(size.trim(), 16).ok()?;
            let mut chunk = vec![0u8; n + 2];
            r.read_exact(&mut chunk).ok()?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
    }
    Some(Request { method, path, headers, body })
}

fn respond(w: &mut TcpStream, status: u16, body: &Value) {
    let text = if body.is_null() { String::new() } else { body.to_string() };
    let _ = write!(
        w,
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: keep-alive\r\n\r\n{text}",
        text.len()
    );
}

/// Multipart parts: name → (filename, bytes).
fn multipart(req: &Request) -> HashMap<String, (Option<String>, Vec<u8>)> {
    let ct = &req.headers["content-type"];
    let boundary = format!("--{}", ct.split("boundary=").nth(1).unwrap());
    let b = &req.body;
    let mut out = HashMap::new();
    let find = |from: usize, pat: &[u8]| b[from..].windows(pat.len()).position(|w| w == pat).map(|p| p + from);
    let mut pos = find(0, boundary.as_bytes()).unwrap() + boundary.len();
    while b[pos..].starts_with(b"\r\n") {
        let head_end = find(pos, b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&b[pos + 2..head_end]).to_string();
        let next = find(head_end + 4, format!("\r\n{boundary}").as_bytes()).unwrap();
        let data = b[head_end + 4..next].to_vec();
        let name = head.split("name=\"").nth(1).unwrap().split('"').next().unwrap().to_string();
        let filename = head.split("filename=\"").nth(1).map(|s| s.split('"').next().unwrap().to_string());
        out.insert(name, (filename, data));
        pos = next + 2 + boundary.len();
    }
    out
}

fn handle(st: &Mutex<State>, req: &Request) -> (u16, Value) {
    if req.headers.get("x-api-key").map(String::as_str) != Some("test-api-key-123") {
        return (401, json!({"message": "Invalid API key"}));
    }
    let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
    let ids =
        |v: &Value| -> Vec<String> { v["ids"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect() };
    let mut s = st.lock();
    let path = req.path.split('?').next().unwrap().trim_start_matches("/api").to_string();
    let seg: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (req.method.as_str(), seg.as_slice()) {
        ("GET", ["server", "version"]) => (200, json!({"major": 3, "minor": 3, "patch": 1, "prerelease": null})),
        ("GET", ["users", "me"]) => (200, json!({"id": "u0", "name": "Owner", "email": "owner@example.com"})),
        ("GET", ["users"]) => (
            200,
            json!([{"id": "u0", "name": "Owner", "email": "owner@example.com"}, {"id": "u1", "name": "Friend", "email": "friend@example.com"}]),
        ),
        ("POST", ["assets", "bulk-upload-check"]) => {
            let results: Vec<Value> = body["assets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| match s.assets.iter().find(|(_, x)| x.sum == a["checksum"].as_str().unwrap()) {
                    Some((id, x)) => {
                        json!({"id": a["id"], "action": "reject", "reason": "duplicate", "assetId": id, "isTrashed": x.trashed})
                    }
                    None => json!({"id": a["id"], "action": "accept"}),
                })
                .collect();
            (200, json!({ "results": results }))
        }
        ("POST", ["assets"]) => {
            let parts = multipart(req);
            let data = &parts["assetData"].1;
            let sum = base64::engine::general_purpose::STANDARD.encode(sha1::Sha1::digest(data));
            assert_eq!(req.headers.get("x-immich-checksum"), Some(&sum), "checksum header");
            if let Some((id, _)) = s.assets.iter().find(|(_, x)| x.sum == sum) {
                return (200, json!({"id": id, "status": "duplicate"}));
            }
            let text = |k: &str| parts.get(k).map(|p| String::from_utf8_lossy(&p.1).to_string());
            let id = s.id();
            let a = Asset {
                name: parts["assetData"].0.clone().unwrap_or_default(),
                sum,
                favorite: text("isFavorite").as_deref() == Some("true"),
                visibility: text("visibility").unwrap_or_else(|| "timeline".into()),
                live: text("livePhotoVideoId"),
                ..Default::default()
            };
            assert!(text("fileCreatedAt").is_some() && text("fileModifiedAt").is_some());
            s.assets.insert(id.clone(), a);
            s.uploads += 1;
            (201, json!({"id": id, "status": "created"}))
        }
        ("PATCH", ["assets"]) => {
            for id in ids(&body) {
                let a = s.assets.get_mut(&id).unwrap();
                if let Some(d) = body["dateTimeOriginal"].as_str() {
                    a.date = Some(d.to_string());
                }
                if let Some(d) = body["description"].as_str() {
                    a.description = Some(d.to_string());
                }
                if body.get("rating").is_some() {
                    a.rating = body["rating"].as_i64();
                }
                if let Some(f) = body["isFavorite"].as_bool() {
                    a.favorite = f;
                }
                if let Some(v) = body["visibility"].as_str() {
                    a.visibility = v.to_string();
                }
            }
            (204, Value::Null)
        }
        ("DELETE", ["assets"]) => {
            assert_eq!(body["force"], json!(false), "never a permanent delete");
            for id in ids(&body) {
                s.assets.get_mut(&id).unwrap().trashed = true;
            }
            (204, Value::Null)
        }
        ("PUT", ["tags"]) => {
            let mut out = Vec::new();
            for t in body["tags"].as_array().unwrap() {
                let v = t.as_str().unwrap().to_string();
                let id = match s.tags.get(&v) {
                    Some(id) => id.clone(),
                    None => {
                        let id = s.id();
                        s.tags.insert(v.clone(), id.clone());
                        id
                    }
                };
                out.push(json!({"id": id, "value": v, "name": v.rsplit('/').next().unwrap()}));
            }
            (200, json!(out))
        }
        (m @ ("PUT" | "DELETE"), ["tags", tid, "assets"]) => {
            let v = s.tag_value(tid).expect("known tag");
            for id in ids(&body) {
                let a = s.assets.get_mut(&id).unwrap();
                if m == "PUT" {
                    a.tags.insert(v.clone());
                } else {
                    a.tags.remove(&v);
                }
            }
            (200, json!([]))
        }
        ("POST", ["albums"]) => {
            let id = s.id();
            let album = Album {
                name: body["albumName"].as_str().unwrap().to_string(),
                assets: body["assetIds"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect(),
                users: body["albumUsers"].as_array().into_iter().flatten().map(|u| u["userId"].as_str().unwrap().to_string()).collect(),
            };
            s.albums.insert(id.clone(), album);
            (201, json!({ "id": id }))
        }
        ("GET", ["albums", id]) => match s.albums.get(*id) {
            Some(a) => (
                200,
                json!({"id": id, "albumUsers": a.users.iter().map(|u| json!({"user": {"id": u}, "role": "viewer"})).collect::<Vec<_>>()}),
            ),
            None => (404, json!({"message": "Not found"})),
        },
        ("PUT", ["albums", id, "assets"]) => {
            let ids = ids(&body);
            s.albums.get_mut(*id).unwrap().assets.extend(ids);
            (200, json!([]))
        }
        ("PUT", ["albums", id, "users"]) => {
            let users: Vec<String> =
                body["albumUsers"].as_array().unwrap().iter().map(|u| u["userId"].as_str().unwrap().to_string()).collect();
            s.albums.get_mut(*id).unwrap().users.extend(users);
            (200, json!({ "id": id }))
        }
        other => panic!("unexpected request {other:?}"),
    }
}

fn serve() -> (String, Arc<Mutex<State>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let state = Arc::new(Mutex::new(State::default()));
    let st = state.clone();
    std::thread::spawn(move || {
        for conn in l.incoming().flatten() {
            let st = st.clone();
            std::thread::spawn(move || {
                let mut w = conn.try_clone().unwrap();
                let mut r = BufReader::new(conn);
                while let Some(req) = read_request(&mut r) {
                    let (status, body) = handle(&st, &req);
                    respond(&mut w, status, &body);
                }
            });
        }
    });
    (url, state)
}

fn wait_idle(lib: &Library) {
    let start = Instant::now();
    let mut quiet = 0;
    while quiet < 3 {
        let busy = lib.jobs.list().iter().any(|j| matches!(j.state.as_str(), "queued" | "running"));
        quiet = if busy { 0 } else { quiet + 1 };
        assert!(start.elapsed() < Duration::from_secs(300));
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn id_of(lib: &Library, name: &str) -> i64 {
    lib.db.read().query_row("SELECT id FROM assets WHERE name = ?1", [name], |r| r.get(0)).unwrap()
}

fn immich_id(lib: &Library, id: i64) -> Option<String> {
    lib.db.read().query_row("SELECT immich_id FROM assets WHERE id = ?1", [id], |r| r.get(0)).unwrap()
}

#[test]
fn sync_uploads_updates_albums_and_removes() {
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out/library/Dump");
    if !fx.exists() {
        eprintln!("fixtures-out not found; skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let lib_dir = tmp.path().join("Archive");
    std::fs::create_dir_all(&lib_dir).unwrap();
    let mut names: Vec<String> =
        ["IMG_1004.JPG", "IMG_1004.MOV", "IMG_1016.JPG", "IMG_1016.MOV", "IMG_5001.PNG", "IMG_5002.PNG"].map(String::from).to_vec();
    names.extend((9101..=9112).map(|k| format!("IMG_{k}.HEIC")));
    for n in &names {
        std::fs::copy(fx.join(n), lib_dir.join(n)).unwrap();
    }
    let lib = Library::open(&tmp.path().join("data"), Arc::new(|_| {})).unwrap();
    lib.add_root(&lib_dir).unwrap();
    lib.start();
    wait_idle(&lib);

    // Screenshots stay local; photos go.
    let shot = id_of(&lib, "IMG_5001.PNG");
    lib.edit(&[shot, id_of(&lib, "IMG_5002.PNG")], &EditAction::Category { key: Some("screenshots".into()) }).unwrap();
    let photos: Vec<i64> = names.iter().filter(|n| n.ends_with(".JPG") || n.ends_with(".HEIC")).map(|n| id_of(&lib, n)).collect();
    lib.edit(&photos, &EditAction::Category { key: Some("photos".into()) }).unwrap();
    // The Lisbon photos are an event, shared with a friend.
    let lisbon: Vec<i64> = (9101..=9112).map(|k| id_of(&lib, &format!("IMG_{k}.HEIC"))).collect();
    lib.db
        .tx(|tx| {
            let b = roll_core::edits::begin_batch(tx, "event", "test")?;
            let e = roll_core::events::create(tx, b, "Lisbon weekend", &lisbon, None)?;
            tx.execute("UPDATE events SET immich_share = '[\"u1\"]' WHERE id = ?1", [e])?;
            Ok(())
        })
        .unwrap();

    let plan = immich::plan(&lib.db.read()).unwrap();
    assert_eq!((plan.uploads, plan.updates, plan.removes, plan.new_albums), (14, 0, 0, 1), "{plan:?}");
    assert!(!plan.upload_samples.contains(&(shot as i32)));

    let (url, server) = serve();
    let client = Client::new(&url, "test-api-key-123");
    assert_eq!(client.version().unwrap(), "3.3.1");
    assert!(Client::new(&url, "wrong").me().unwrap_err().to_string().contains("API key"));

    let r = immich::sync(&lib.db, &client, false, &|_, _| true).unwrap();
    assert_eq!((r.uploaded, r.linked, r.albums), (14, 0, 1), "{r:?}");
    {
        let s = server.lock();
        // 14 photos plus 2 Live Photo videos, linked to their stills.
        assert_eq!(s.uploads, 16);
        let live = immich_id(&lib, id_of(&lib, "IMG_1004.JPG")).unwrap();
        let video = immich_id(&lib, id_of(&lib, "IMG_1004.MOV")).unwrap();
        assert_eq!(s.assets[&live].live.as_deref(), Some(video.as_str()));
        let album = s.albums.values().next().unwrap();
        assert_eq!((album.name.as_str(), album.assets.len()), ("Lisbon weekend", 12));
        assert!(album.users.contains("u1"));
        assert!(!s.assets.values().any(|a| a.name.ends_with(".PNG")));
        // Unsaved category tags reach Immich as tags.
        assert!(s.assets[&live].tags.contains("Category/Photos"), "{:?}", s.assets[&live].tags);
    }

    // The CLI fallback: the event folder becomes its album.
    let script = lib.immich_script(false).unwrap();
    assert!(script.contains("immich upload") && !script.contains("test-api-key-123"), "{script}");

    // Nothing is uploaded twice.
    let r = immich::sync(&lib.db, &client, false, &|_, _| true).unwrap();
    assert_eq!((r.uploaded, r.linked, r.updated), (0, 0, 0), "{r:?}");
    assert_eq!(server.lock().uploads, 16);

    // Later edits show up after the next sync.
    let one = lisbon[0];
    lib.edit(&[one], &EditAction::Rating { rating: 4 }).unwrap();
    lib.edit(&[one], &EditAction::AddTag { path: "Trip/Lisbon".into() }).unwrap();
    lib.edit(&[one], &EditAction::Description { text: Some("Tram 28".into()) }).unwrap();
    assert_eq!(immich::plan(&lib.db.read()).unwrap().updates, 1);
    let r = immich::sync(&lib.db, &client, false, &|_, _| true).unwrap();
    assert_eq!(r.updated, 1);
    {
        let s = server.lock();
        let a = &s.assets[&immich_id(&lib, one).unwrap()];
        assert_eq!((a.rating, a.description.as_deref()), (Some(4), Some("Tram 28")));
        assert!(a.tags.contains("Trip/Lisbon"));
    }
    lib.edit(&[one], &EditAction::RemoveTag { path: "Trip/Lisbon".into() }).unwrap();
    immich::sync(&lib.db, &client, false, &|_, _| true).unwrap();
    assert!(!server.lock().assets[&immich_id(&lib, one).unwrap()].tags.contains("Trip/Lisbon"));

    // A file the server has but Roll lost track of is linked, not sent again.
    lib.db.write().execute("UPDATE assets SET immich_id = NULL, immich_synced = NULL WHERE id = ?1", [lisbon[1]]).unwrap();
    let r = immich::sync(&lib.db, &client, false, &|_, _| true).unwrap();
    assert_eq!((r.uploaded, r.linked), (0, 1), "{r:?}");

    // Private items go only when picked, and then locked.
    let private = lisbon[2];
    lib.edit(&[private], &EditAction::Private { on: true }).unwrap();
    lib.edit(&[private], &EditAction::Immich { mode: "send".into() }).unwrap();
    immich::sync(&lib.db, &client, false, &|_, _| true).unwrap();
    assert_eq!(server.lock().assets[&immich_id(&lib, private).unwrap()].visibility, "locked");

    // "Keep local only": to Immich's trash after confirmation; the local
    // file stays as it is.
    let gone = lisbon[3];
    let file = lib_dir.join("IMG_9104.HEIC");
    let before = std::fs::read(&file).unwrap();
    let iid = immich_id(&lib, gone).unwrap();
    lib.edit(&[gone], &EditAction::Immich { mode: "local".into() }).unwrap();
    assert_eq!(immich::plan(&lib.db.read()).unwrap().removes, 1);
    immich::sync(&lib.db, &client, false, &|_, _| true).unwrap();
    assert!(!server.lock().assets[&iid].trashed, "not without confirmation");
    let r = immich::sync(&lib.db, &client, true, &|_, _| true).unwrap();
    assert_eq!(r.removed, 1);
    assert!(server.lock().assets[&iid].trashed);
    assert_eq!(immich_id(&lib, gone), None);
    assert_eq!(std::fs::read(&file).unwrap(), before);
    assert!(Path::new(&file).exists());
}
