//! The Immich round trip (PROMPT 9.3) against a real server: the same
//! fixture files as `immich_sync.rs`, checked through Immich's own API.
//! Start the throwaway server with `e2e/immich/server.ps1 setup`, then
//!
//! ROLL_IMMICH_URL=http://localhost:2283 ROLL_IMMICH_KEY=… \
//!   cargo test -p roll-core --test immich_real -- --ignored --nocapture
//!
//! The test only adds to the server (and trashes one item it uploaded), so
//! runs can repeat; names carry a run stamp.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use roll_core::actions::EditAction;
use roll_core::datefix::{Amount, DateFix};
use roll_core::immich::{self, Client};
use roll_core::library::Library;
use roll_geo::Place;
use serde_json::{Value, json};

struct Api {
    base: String,
    key: String,
    agent: ureq::Agent,
}

impl Api {
    fn get(&self, path: &str) -> Value {
        self.agent.get(format!("{}/api{path}", self.base)).header("x-api-key", &self.key).call().unwrap().into_body().read_json().unwrap()
    }
    fn post(&self, path: &str, body: &Value) -> Value {
        self.agent
            .post(format!("{}/api{path}", self.base))
            .header("x-api-key", &self.key)
            .send_json(body)
            .unwrap()
            .into_body()
            .read_json()
            .unwrap()
    }
    fn asset(&self, id: &str) -> Value {
        self.get(&format!("/assets/{id}"))
    }
    /// Immich applies metadata in background jobs: wait until `ok` holds.
    fn until(&self, id: &str, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let a = self.asset(id);
            if ok(&a) {
                return a;
            }
            assert!(start.elapsed() < Duration::from_secs(90), "{what}: {a:#}");
            std::thread::sleep(Duration::from_millis(500));
        }
    }
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

fn tags(a: &Value) -> Vec<String> {
    a["tags"].as_array().map(|t| t.iter().filter_map(|t| t["value"].as_str().map(String::from)).collect()).unwrap_or_default()
}

fn instant(s: &str) -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::parse_from_rfc3339(s).unwrap_or_else(|_| chrono::DateTime::parse_from_rfc3339(&format!("{s}Z")).unwrap())
}

#[test]
#[ignore = "needs a real Immich server: ROLL_IMMICH_URL and ROLL_IMMICH_KEY"]
fn round_trip() {
    let (Ok(url), Ok(key)) = (std::env::var("ROLL_IMMICH_URL"), std::env::var("ROLL_IMMICH_KEY")) else { return };
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out/library/Dump");
    assert!(fx.exists(), "run `cargo run -p roll-fixtures` first");
    let api = Api {
        base: url.trim_end_matches('/').to_string(),
        key: key.clone(),
        agent: ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(60))).build().into(),
    };
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();

    // Someone to share the event album with.
    let users = api.get("/admin/users");
    let friend = match users.as_array().unwrap().iter().find(|u| u["email"] == "friend@roll.test") {
        Some(u) => u["id"].as_str().unwrap().to_string(),
        None => {
            api.post("/admin/users", &json!({"email": "friend@roll.test", "password": format!("pw-{stamp}-x9"), "name": "Friend"}))["id"]
                .as_str()
                .unwrap()
                .to_string()
        }
    };

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

    let shot = id_of(&lib, "IMG_5001.PNG");
    lib.edit(&[shot, id_of(&lib, "IMG_5002.PNG")], &EditAction::Category { key: Some("screenshots".into()) }).unwrap();
    let photos: Vec<i64> = names.iter().filter(|n| n.ends_with(".JPG") || n.ends_with(".HEIC")).map(|n| id_of(&lib, n)).collect();
    lib.edit(&photos, &EditAction::Category { key: Some("photos".into()) }).unwrap();
    let lisbon: Vec<i64> = (9101..=9112).map(|k| id_of(&lib, &format!("IMG_{k}.HEIC"))).collect();
    let album_name = format!("Lisbon weekend {stamp}");
    lib.db
        .tx(|tx| {
            let b = roll_core::edits::begin_batch(tx, "event", "test")?;
            let e = roll_core::events::create(tx, b, &album_name, &lisbon, None)?;
            tx.execute("UPDATE events SET immich_share = ?1 WHERE id = ?2", rusqlite::params![json!([friend]).to_string(), e])?;
            Ok(())
        })
        .unwrap();

    let client = Client::new(&url, &key);
    let version = client.version().unwrap();
    println!("Immich {version}");
    assert!(version.starts_with("3."), "{version}");
    assert!(Client::new(&url, "wrong").me().unwrap_err().to_string().contains("API key"));

    // First sync: 14 items, the Live Photo videos with them, one album.
    let r = immich::sync(&lib.db, &client, false, &|_, _| true).unwrap();
    println!("first sync: {r:?}");
    assert_eq!(r.uploaded + r.linked, 14, "{r:?}");
    assert_eq!(r.albums, 1);
    assert!(immich_id(&lib, shot).is_none(), "screenshots stay local");

    let live = immich_id(&lib, id_of(&lib, "IMG_1004.JPG")).unwrap();
    let video = immich_id(&lib, id_of(&lib, "IMG_1004.MOV")).unwrap();
    let a = api.until(&live, "Live Photo linked and tagged", |a| a["livePhotoVideoId"].as_str() == Some(video.as_str()));
    assert!(tags(&a).contains(&"Category/Photos".to_string()), "{:?}", tags(&a));
    // Date and place match Roll's.
    let (local, lat, lon): (Option<String>, Option<f64>, Option<f64>) = lib
        .db
        .read()
        .query_row("SELECT taken_utc, lat, lon FROM assets WHERE id = ?1", [id_of(&lib, "IMG_1004.JPG")], |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?.map(|t| chrono::DateTime::from_timestamp_millis(t).unwrap().to_rfc3339()),
                r.get(1)?,
                r.get(2)?,
            ))
        })
        .unwrap();
    if let Some(t) = local {
        let a = api.until(&live, "date", |a| a["exifInfo"]["dateTimeOriginal"].is_string());
        assert_eq!(instant(a["exifInfo"]["dateTimeOriginal"].as_str().unwrap()), instant(&t));
        println!("date matches: {t}");
    }
    if let (Some(la), Some(lo)) = (lat, lon) {
        let a = api.until(&live, "place", |a| a["exifInfo"]["latitude"].is_number());
        assert!((a["exifInfo"]["latitude"].as_f64().unwrap() - la).abs() < 1e-4);
        assert!((a["exifInfo"]["longitude"].as_f64().unwrap() - lo).abs() < 1e-4);
        println!("place matches: {la:.4}, {lo:.4}");
    }

    let albums = api.get("/albums");
    let album = albums.as_array().unwrap().iter().find(|a| a["albumName"] == album_name.as_str()).expect("event album");
    assert_eq!(album["assetCount"], 12);
    assert!(album["albumUsers"].as_array().unwrap().iter().any(|u| u["user"]["id"] == friend.as_str()), "{album:#}");

    // Nothing is uploaded twice.
    let stats = api.get("/assets/statistics");
    let r = immich::sync(&lib.db, &client, false, &|_, _| true).unwrap();
    assert_eq!((r.uploaded, r.linked, r.updated), (0, 0, 0), "{r:?}");
    assert_eq!(api.get("/assets/statistics"), stats);

    // Later changes in Roll show up after the next sync.
    let one = lisbon[0];
    let iid = immich_id(&lib, one).unwrap();
    lib.edit(&[one], &EditAction::Rating { rating: 4 }).unwrap();
    lib.edit(&[one], &EditAction::AddTag { path: "Trip/Lisbon".into() }).unwrap();
    lib.edit(&[one], &EditAction::Description { text: Some("Tram 28".into()) }).unwrap();
    lib.set_location(&[one], Some(Place { lat: 38.7139, lon: -9.1394, accuracy_m: 10.0 }), Some("Lisbon")).unwrap();
    let before = api.asset(&iid)["exifInfo"]["dateTimeOriginal"].as_str().map(instant);
    lib.date_apply(&DateFix {
        ids: vec![one],
        mode: "amount".into(),
        reference: None,
        second: None,
        offset_min: None,
        amount: Some(Amount { hours: 1, ..Default::default() }),
        exact: None,
        camera_from: None,
        camera_to: None,
    })
    .unwrap();
    let r = immich::sync(&lib.db, &client, false, &|_, _| true).unwrap();
    assert_eq!(r.updated, 1, "{r:?}");
    let a = api.until(&iid, "edits applied", |a| {
        a["exifInfo"]["rating"] == 4
            && a["exifInfo"]["description"] == "Tram 28"
            && a["exifInfo"]["latitude"].as_f64().is_some_and(|l| (l - 38.7139).abs() < 1e-4)
            && tags(a).contains(&"Trip/Lisbon".to_string())
    });
    if let Some(b) = before {
        let after = api.until(&iid, "date moved", |a| a["exifInfo"]["dateTimeOriginal"].as_str().map(instant) != Some(b));
        assert_eq!(instant(after["exifInfo"]["dateTimeOriginal"].as_str().unwrap()) - b, chrono::Duration::hours(1), "{a:#}");
        println!("date moved by an hour: {b} -> {}", after["exifInfo"]["dateTimeOriginal"]);
    }

    // "Keep local only": Immich's trash after confirmation, the local file untouched.
    let gone = lisbon[3];
    let file = lib_dir.join("IMG_9104.HEIC");
    let bytes = std::fs::read(&file).unwrap();
    let gid = immich_id(&lib, gone).unwrap();
    lib.edit(&[gone], &EditAction::Immich { mode: "local".into() }).unwrap();
    immich::sync(&lib.db, &client, false, &|_, _| true).unwrap();
    assert_eq!(api.asset(&gid)["isTrashed"], false, "not without confirmation");
    let r = immich::sync(&lib.db, &client, true, &|_, _| true).unwrap();
    assert_eq!(r.removed, 1);
    assert_eq!(api.asset(&gid)["isTrashed"], true);
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    println!("round trip ok: album {album_name}");
}
