//! The fixture library: a phone dump with every case Roll must handle, and
//! a manifest with the ground truth for each file.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Utc};
use image::RgbImage;
use roll_meta::exiftool::ExifTool;

use crate::exifblock::{self, ExifSpec};
use crate::heif;
use crate::manifest::{CameraSet, Item, Manifest, TrackFile};
use crate::pixels::{self, JunkKind, Rng};
use crate::tracks::{Stop, Track};

pub const SEED: u64 = 20261010;

pub struct Gen {
    out: PathBuf,
    ffmpeg: PathBuf,
    et: ExifTool,
    tmp: PathBuf,
    pub manifest: Manifest,
}

pub fn lt(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, mi, s).unwrap()
}

fn exif_fmt(t: NaiveDateTime) -> String {
    t.format("%Y:%m:%d %H:%M:%S").to_string()
}

fn iso(t: NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn off(min: i32) -> String {
    let s = if min < 0 { '-' } else { '+' };
    format!("{s}{:02}:{:02}", min.abs() / 60, min.abs() % 60)
}

fn to_utc(local: NaiveDateTime, offset_min: i32) -> DateTime<Utc> {
    (local - Duration::minutes(offset_min as i64)).and_utc()
}

fn to_local(utc: DateTime<Utc>, offset_min: i32) -> NaiveDateTime {
    utc.naive_utc() + Duration::minutes(offset_min as i64)
}

/// A phone or camera photo to create.
struct Shot {
    rel: String,
    seed: u64,
    variant: u64,
    size: (u32, u32),
    heic: bool,
    exif: ExifSpec,
    item: Item,
}

impl Gen {
    pub fn new(out: PathBuf, ffmpeg: PathBuf, exiftool: &Path) -> Result<Self> {
        let tmp = out.join(".tmp");
        std::fs::create_dir_all(&tmp)?;
        Ok(Self {
            out,
            ffmpeg,
            et: ExifTool::spawn(exiftool)?,
            tmp,
            manifest: Manifest { generator: "roll-fixtures", seed: SEED, library: "library", ..Default::default() },
        })
    }

    fn abs(&self, rel: &str) -> Result<PathBuf> {
        let p = self.out.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        Ok(p)
    }

    fn write(&self, rel: &str, bytes: &[u8]) -> Result<PathBuf> {
        let p = self.abs(rel)?;
        std::fs::write(&p, bytes).with_context(|| format!("writing {rel}"))?;
        Ok(p)
    }

    fn exiftool(&mut self, rel: &str, args: &[String]) -> Result<()> {
        let p = self.out.join(rel);
        let mut a: Vec<String> = vec!["-overwrite_original".into()];
        a.extend(args.iter().map(|s| roll_meta::exiftool::arg(s)));
        a.push(p.to_string_lossy().into_owned());
        let o = self.et.execute(&a)?;
        if o.stderr.contains("Error") {
            bail!("exiftool on {rel}: {}", o.stderr.trim());
        }
        Ok(())
    }

    fn push(&mut self, item: Item) {
        self.manifest.items.push(item);
    }

    fn shot(&mut self, s: Shot) -> Result<()> {
        let img = pixels::scene(s.size.0, s.size.1, s.seed, s.variant);
        let mut spec = s.exif;
        spec.width = Some(s.size.0);
        spec.height = Some(s.size.1);
        let tiff = exifblock::tiff(&spec);
        if s.heic {
            let png = self.tmp.join("frame.png");
            img.save(&png)?;
            let out = self.abs(&s.rel)?;
            heif::encode(&self.ffmpeg, &png, &out, &tiff)?;
        } else {
            self.write(&s.rel, &exifblock::insert_into_jpeg(&pixels::jpeg(&img, 88), &tiff))?;
        }
        let has_gps = spec.gps.is_some();
        let true_gps = s.item.true_gps.or(spec.gps.map(|(lat, lon, _)| [lat, lon]));
        self.push(Item { path: s.rel, kind: "photo", has_gps, true_gps, ..s.item });
        Ok(())
    }

    fn plain_jpeg(&mut self, rel: &str, img: &RgbImage, quality: u8, spec: Option<ExifSpec>) -> Result<PathBuf> {
        let bytes = pixels::jpeg(img, quality);
        match spec {
            Some(s) => self.write(rel, &exifblock::insert_into_jpeg(&bytes, &exifblock::tiff(&s))),
            None => self.write(rel, &bytes),
        }
    }

    /// A short clip of a still scene. `meta` are ExifTool assignments.
    #[allow(clippy::too_many_arguments)]
    fn video(&mut self, rel: &str, img: &RgbImage, secs: f32, hevc: bool, audio: bool, meta: &[String], item: Item) -> Result<()> {
        let png = self.tmp.join("frame.png");
        img.save(&png)?;
        let out = self.abs(rel)?;
        let mut cmd = Command::new(&self.ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-loop", "1", "-framerate", "30", "-i"]).arg(&png);
        if audio {
            cmd.args(["-f", "lavfi", "-i", &format!("sine=frequency=440:duration={secs}")]);
        }
        cmd.args([
            "-t",
            &secs.to_string(),
            "-vf",
            "zoompan=z='1+0.002*on':d=1:s=WxH:fps=30".replace("WxH", &format!("{}x{}", img.width(), img.height())).as_str(),
            "-pix_fmt",
            "yuv420p",
        ]);
        if hevc {
            cmd.args(["-c:v", "libx265", "-x265-params", "log-level=error", "-tag:v", "hvc1"]);
        } else {
            cmd.args(["-c:v", "libx264", "-preset", "veryfast"]);
        }
        if audio {
            cmd.args(["-c:a", "aac", "-shortest"]);
        }
        let status = cmd.arg(&out).status().context("running ffmpeg")?;
        if !status.success() {
            bail!("ffmpeg failed for {rel}");
        }
        if !meta.is_empty() {
            self.exiftool(rel, meta)?;
        }
        let has_gps = meta.iter().any(|m| m.contains("GPSCoordinates"));
        self.push(Item { path: rel.to_string(), kind: "video", has_gps, ..item });
        Ok(())
    }

    /// Apple QuickTime metadata for an iPhone clip.
    fn iphone_video_meta(
        model: &str,
        local: NaiveDateTime,
        offset_min: i32,
        gps: Option<(f64, f64)>,
        content_id: Option<&str>,
    ) -> Vec<String> {
        let utc = exif_fmt(to_utc(local, offset_min).naive_utc());
        let mut m = vec![
            "-Keys:Make=Apple".to_string(),
            format!("-Keys:Model={model}"),
            format!("-Keys:CreationDate={}{}", exif_fmt(local), off(offset_min)),
            format!("-QuickTime:CreateDate={utc}"),
            format!("-QuickTime:TrackCreateDate={utc}"),
            format!("-QuickTime:MediaCreateDate={utc}"),
        ];
        if let Some((lat, lon)) = gps {
            m.push(format!("-Keys:GPSCoordinates={lat}, {lon}, 80"));
        }
        if let Some(id) = content_id {
            m.push(format!("-Keys:ContentIdentifier={id}"));
        }
        m
    }

    pub fn finish(self) -> Result<Manifest> {
        std::fs::remove_dir_all(&self.tmp).ok();
        Ok(self.manifest)
    }
}

fn phone_spec(model: &str, local: NaiveDateTime, offset_min: i32, gps: Option<(f64, f64)>) -> ExifSpec {
    ExifSpec {
        make: Some(
            if model.starts_with("SM-") {
                "samsung"
            } else if model.starts_with("Pixel") {
                "Google"
            } else {
                "Apple"
            }
            .into(),
        ),
        model: Some(model.into()),
        software: Some(if model.starts_with("iPhone") { "16.5" } else { "G991BXXU5" }.into()),
        lens: Some(format!("{model} back camera 4.25mm f/1.8")),
        orientation: Some(1),
        date: Some(exif_fmt(local)),
        offset: Some(off(offset_min)),
        gps: gps.map(|(a, b)| (a, b, 75.0)),
        ..Default::default()
    }
}

fn camera_spec(make: &str, model: &str, serial: &str, file_local: NaiveDateTime, offset: Option<i32>) -> ExifSpec {
    ExifSpec {
        make: Some(make.into()),
        model: Some(model.into()),
        serial: Some(serial.into()),
        software: Some("Ver.1.10".into()),
        lens: Some("24-70mm f/2.8".into()),
        orientation: Some(1),
        date: Some(exif_fmt(file_local)),
        offset: offset.map(off),
        ..Default::default()
    }
}

fn stop(lat: f64, lon: f64, name: &'static str) -> Stop {
    Stop { lat, lon, name }
}

const HOME: (f64, f64) = (47.3769, 8.5417);

/// Random daytime moments while at a stop of the track.
fn moments(track: &Track, rng: &mut Rng, n: usize) -> Vec<DateTime<Utc>> {
    let mut out: Vec<DateTime<Utc>> = (0..n)
        .map(|_| {
            let (s, e, _) = *rng.pick(&track.stops);
            let span = (e - s).num_seconds().max(60);
            s + Duration::seconds(rng.range(0, span))
        })
        .collect();
    out.sort();
    out
}

pub fn generate(g: &mut Gen) -> Result<()> {
    lake_garda(g)?;
    new_york(g)?;
    home_and_misc(g)?;
    screens_and_saved(g)?;
    duplicates_and_paths(g)?;
    hike(g)?;
    camera_card(g)?;
    exports(g)?;
    Ok(())
}

/// Exports outside the library for Add photos: an Android WhatsApp chat
/// (day-first dates, German), an iPhone one (month-first, 12-hour clock),
/// and a Google Takeout export whose album repeats photos of a year folder.
fn exports(g: &mut Gen) -> Result<()> {
    // Android: `IMG-YYYYMMDD-WA0001.jpg`, no EXIF, times only in the chat.
    let mut chat = String::from("12.06.21, 09:58 - Nachrichten und Anrufe sind Ende-zu-Ende-verschlüsselt.\n");
    for k in 0..6u32 {
        let t = lt(2021, 6, 12 + k / 3, 10 + k, 5 * k, 0);
        let name = format!("IMG-{}-WA{:04}.jpg", t.format("%Y%m%d"), k + 1);
        let rel = format!("whatsapp-android/{name}");
        g.plain_jpeg(&rel, &pixels::scene(1600, 1200, 9700 + k as u64, 0), 80, None)?;
        let sender = if k % 2 == 0 { "Alice" } else { "Ben" };
        chat.push_str(&format!("{}, {} - {sender}: {name} (Datei angehängt)\n", t.format("%d.%m.%y"), t.format("%H:%M")));
        chat.push_str(&format!("{}, {} - {sender}: Schön hier\n", t.format("%d.%m.%y"), t.format("%H:%M")));
        g.push(Item {
            path: rel,
            kind: "photo",
            true_local: Some(iso(t)),
            tags: vec!["Source/WhatsApp".into(), format!("From/{sender}")],
            flags: vec!["whatsapp_export", "no_metadata"],
            ..Default::default()
        });
    }
    g.write("whatsapp-android/WhatsApp Chat mit Alice.txt", chat.as_bytes())?;

    // iPhone: `_chat.txt` in a folder named after the chat; 3/7/21 is
    // 7 March or 3 July only by the file names.
    let dir = "whatsapp-iphone/WhatsApp Chat - Bob";
    let mut chat = String::from("\u{feff}[7/3/21, 6:00:00 PM] Bob: \u{200e}Messages and calls are end-to-end encrypted.\n");
    for k in 0..4u32 {
        let t = lt(2021, 7, 3, 18, 20 + k, 11 + k);
        let name = format!("{:08}-PHOTO-{}.jpg", k + 3, t.format("%Y-%m-%d-%H-%M-%S"));
        let rel = format!("{dir}/{name}");
        g.plain_jpeg(&rel, &pixels::scene(1200, 1600, 9720 + k as u64, 0), 80, None)?;
        chat.push_str(&format!("[{}, {}] Bob: \u{200e}<attached: {name}>\n", t.format("%-m/%-d/%y"), t.format("%-I:%M:%S %p")));
        g.push(Item {
            path: rel,
            kind: "photo",
            true_local: Some(iso(t)),
            tags: vec!["Source/WhatsApp".into(), "From/Bob".into()],
            flags: vec!["whatsapp_export", "no_metadata"],
            ..Default::default()
        });
    }
    g.write(&format!("{dir}/_chat.txt"), chat.as_bytes())?;

    // Takeout: five Rome photos in the year folder, three of them again in
    // the album folder. EXIF is stripped; the JSON has the instants (UTC).
    let base = "takeout-export/Takeout/Google Photos";
    for k in 0..5u32 {
        let t = lt(2021, 9, 18, 11, 10 * k, 0);
        let utc = to_utc(t, 120);
        let name = format!("PXL_20210918_{}.jpg", utc.format("%H%M%S000"));
        let img = pixels::scene(1512, 2016, 9740 + k as u64, 0);
        let (lat, lon) = (41.8902 + k as f64 * 0.002, 12.4922);
        let json = serde_json::json!({
            "title": name, "description": "",
            "photoTakenTime": {"timestamp": utc.timestamp().to_string(), "formatted": ""},
            "geoData": {"latitude": lat, "longitude": lon, "altitude": 30.0},
            "geoDataExif": {"latitude": lat, "longitude": lon, "altitude": 30.0},
        });
        let mut folders = vec!["Photos from 2021"];
        if k < 3 {
            folders.push("Rome trip");
        }
        for f in folders {
            let rel = format!("{base}/{f}/{name}");
            g.plain_jpeg(&rel, &img, 85, None)?;
            g.write(&format!("{base}/{f}/{name}.supplemental-metadata.json"), serde_json::to_string_pretty(&json)?.as_bytes())?;
            g.push(Item {
                path: rel,
                kind: "photo",
                category: Some("photos"),
                true_local: Some(iso(t)),
                true_offset_min: Some(120),
                true_gps: Some([lat, lon]),
                tags: if k < 3 { vec!["Album/Rome trip".into()] } else { vec![] },
                flags: vec!["takeout_export", "no_metadata"],
                ..Default::default()
            });
        }
    }
    g.write(&format!("{base}/Rome trip/metadata.json"), br#"{"title": "Rome trip", "description": "", "access": "protected"}"#)?;
    Ok(())
}

/// Trip 1: phone photos with GPS, a Nikon with a clock 1 y 2 h ahead,
/// a Fujifilm that drifts, Live Photos, RAW pairs, location history.
fn lake_garda(g: &mut Gen) -> Result<()> {
    let days = vec![
        (NaiveDate::from_ymd_opt(2019, 8, 3).unwrap(), vec![stop(45.8858, 10.8410, "Riva del Garda"), stop(45.9187, 10.8869, "Arco")]),
        (NaiveDate::from_ymd_opt(2019, 8, 4).unwrap(), vec![stop(45.7631, 10.8085, "Malcesine"), stop(45.7366, 10.8545, "Monte Baldo")]),
        (
            NaiveDate::from_ymd_opt(2019, 8, 5).unwrap(),
            vec![stop(45.8133, 10.7922, "Limone sul Garda"), stop(45.8858, 10.8410, "Riva del Garda")],
        ),
        (NaiveDate::from_ymd_opt(2019, 8, 6).unwrap(), vec![stop(45.4925, 10.6086, "Sirmione"), stop(45.4713, 10.5387, "Desenzano")]),
        (NaiveDate::from_ymd_opt(2019, 8, 7).unwrap(), vec![stop(45.4384, 10.9916, "Verona"), stop(45.4390, 10.9947, "Arena di Verona")]),
        (NaiveDate::from_ymd_opt(2019, 8, 8).unwrap(), vec![stop(45.5490, 10.7213, "Bardolino"), stop(45.5770, 10.7085, "Garda")]),
        (NaiveDate::from_ymd_opt(2019, 8, 9).unwrap(), vec![stop(45.8858, 10.8410, "Riva del Garda")]),
    ];
    let track = Track::simulate(&days, 120, 11);
    let lh = "location-history";
    g.write(&format!("{lh}/lake-garda-2019.gpx"), track.gpx("Lake Garda 2019", 2).as_bytes())?;
    g.write(&format!("{lh}/Timeline.json"), track.timeline_json(HOME).as_bytes())?;
    g.write(&format!("{lh}/Takeout/Location History/Records.json"), track.records_json().as_bytes())?;
    g.write(&format!("{lh}/Takeout/Location History/Semantic Location History/2019/2019_AUGUST.json"), track.semantic_json().as_bytes())?;
    for (p, f) in [
        ("lake-garda-2019.gpx", "gpx"),
        ("Timeline.json", "google_timeline"),
        ("Takeout/Location History/Records.json", "google_records"),
        ("Takeout/Location History/Semantic Location History/2019/2019_AUGUST.json", "google_semantic"),
    ] {
        g.manifest.tracks.push(TrackFile { path: format!("{lh}/{p}"), format: f, covers: "lake garda trip, 2019-08-03 to 2019-08-09" });
    }

    let mut rng = Rng::new(101);
    let times = moments(&track, &mut rng, 48);
    for (i, t) in times.iter().enumerate() {
        let p = track.at(*t);
        let local = to_local(*t, 120);
        let live = i % 12 == 3;
        let heic = i % 3 != 0;
        let ext = if heic { "HEIC" } else { "JPG" };
        let name = format!("IMG_{:04}", 1001 + i);
        let cid = live.then(|| format!("A1B2C3D4-0000-4000-8000-{:012}", 2019_0000 + i));
        let mut spec = phone_spec("iPhone XS", local, 120, Some((p.lat, p.lon)));
        spec.maker_note = Some(exifblock::apple_maker_note(cid.as_deref(), None));
        let rel = format!("library/Dump/{name}.{ext}");
        let group = cid.as_ref().map(|c| format!("live:{c}"));
        g.shot(Shot {
            rel,
            seed: 5000 + i as u64,
            variant: 0,
            size: (2016, 1512),
            heic,
            exif: spec,
            item: Item {
                category: Some("photos"),
                true_local: Some(iso(local)),
                true_offset_min: Some(120),
                true_gps: Some([p.lat, p.lon]),
                camera: Some("Apple iPhone XS".into()),
                group: group.clone(),
                ..Default::default()
            },
        })?;
        if let Some(cid) = &cid {
            let img = pixels::scene(1008, 756, 5000 + i as u64, 0);
            let meta = Gen::iphone_video_meta("iPhone XS", local, 120, Some((p.lat, p.lon)), Some(cid));
            g.video(
                &format!("library/Dump/{name}.MOV"),
                &img,
                1.6,
                true,
                true,
                &meta,
                Item {
                    category: Some("photos"),
                    true_local: Some(iso(local)),
                    true_offset_min: Some(120),
                    true_gps: Some([p.lat, p.lon]),
                    camera: Some("Apple iPhone XS".into()),
                    group,
                    flags: vec!["live_motion"],
                    ..Default::default()
                },
            )?;
        }
    }
    // A trip video with sound and location.
    let t = times[20] + Duration::minutes(3);
    let p = track.at(t);
    let local = to_local(t, 120);
    let img = pixels::scene(1280, 720, 5020, 1);
    let meta = Gen::iphone_video_meta("iPhone XS", local, 120, Some((p.lat, p.lon)), None);
    g.video(
        "library/Dump/IMG_1100.MOV",
        &img,
        4.0,
        false,
        true,
        &meta,
        Item {
            category: Some("photos"),
            true_local: Some(iso(local)),
            true_offset_min: Some(120),
            true_gps: Some([p.lat, p.lon]),
            camera: Some("Apple iPhone XS".into()),
            ..Default::default()
        },
    )?;

    // Nikon D750 with its clock 1 year and 2 hours ahead; same scenes as the phone.
    let wrong = |t: NaiveDateTime| t.with_year(t.year() + 1).unwrap() + Duration::hours(2);
    for (k, i) in (0..times.len()).step_by(2).take(24).enumerate() {
        let true_utc = times[i] + Duration::seconds(rng.range(10, 120));
        let local = to_local(true_utc, 120);
        let file_local = wrong(local);
        let rel = format!("library/Old events/Italy 2019/DSC_{:04}.JPG", k + 1);
        g.shot(Shot {
            rel,
            seed: 5000 + i as u64,
            variant: 7,
            size: (1800, 1200),
            heic: false,
            exif: camera_spec("NIKON CORPORATION", "NIKON D750", "3012345", file_local, None),
            item: Item {
                category: Some("photos"),
                rule_decidable: true,
                true_local: Some(iso(local)),
                true_offset_min: Some(120),
                true_gps: Some([track.at(true_utc).lat, track.at(true_utc).lon]),
                file_local: Some(iso(file_local)),
                camera: Some("NIKON D750".into()),
                flags: vec!["wrong_clock"],
                ..Default::default()
            },
        })?;
    }
    // RAW + JPEG pairs from the same Nikon.
    for k in 0..4u32 {
        let true_utc = times[1 + k as usize * 9] + Duration::seconds(45);
        let local = to_local(true_utc, 120);
        let file_local = wrong(local);
        let stem = format!("library/Old events/Italy 2019/DSC_{:04}", 101 + k);
        let spec = camera_spec("NIKON CORPORATION", "NIKON D750", "3012345", file_local, None);
        let item = Item {
            category: Some("photos"),
            rule_decidable: true,
            true_local: Some(iso(local)),
            true_offset_min: Some(120),
            file_local: Some(iso(file_local)),
            camera: Some("NIKON D750".into()),
            group: Some(format!("raw:DSC_{:04}", 101 + k)),
            flags: vec!["wrong_clock"],
            ..Default::default()
        };
        g.shot(Shot {
            rel: format!("{stem}.JPG"),
            seed: 6100 + k as u64,
            variant: 0,
            size: (1800, 1200),
            heic: false,
            exif: spec.clone(),
            item: item.clone(),
        })?;
        // A RAW stand-in: a TIFF container with the camera's EXIF.
        let img = pixels::scene(900, 600, 6100 + k as u64, 0);
        let raw_rel = format!("{stem}.NEF");
        let raw_path = g.abs(&raw_rel)?;
        img.save_with_format(&raw_path, image::ImageFormat::Tiff)?;
        g.exiftool(
            &raw_rel,
            &[
                "-Make=NIKON CORPORATION".into(),
                "-Model=NIKON D750".into(),
                "-SerialNumber=3012345".into(),
                format!("-DateTimeOriginal={}", exif_fmt(file_local)),
            ],
        )?;
        g.push(Item { path: raw_rel, kind: "photo", flags: vec!["wrong_clock", "raw"], ..item });
    }
    g.manifest.camera_sets.push(CameraSet {
        camera: "NIKON D750",
        true_offset_secs: -(366 * 86400 + 7200),
        note: "clock 1 year 2 hours ahead (2020 is a leap year, so +366 d for August)",
    });

    // Fujifilm X-T3 drifting 5 minutes per day, starting 2 minutes ahead.
    let start = to_utc(lt(2019, 8, 3, 8, 0, 0), 120);
    for (k, i) in (1..times.len()).step_by(4).enumerate() {
        let true_utc = times[i] + Duration::seconds(30);
        let local = to_local(true_utc, 120);
        let days = (true_utc - start).num_seconds() as f64 / 86400.0;
        let drift = Duration::seconds(120 + (days * 300.0) as i64);
        let file_local = local + drift;
        g.shot(Shot {
            rel: format!("library/Dump/DSCF{:04}.JPG", 2001 + k),
            seed: 5000 + i as u64,
            variant: 3,
            size: (1800, 1200),
            heic: false,
            exif: camera_spec("FUJIFILM", "X-T3", "9AB12345", file_local, None),
            item: Item {
                category: Some("photos"),
                rule_decidable: true,
                true_local: Some(iso(local)),
                true_offset_min: Some(120),
                file_local: Some(iso(file_local)),
                camera: Some("FUJIFILM X-T3".into()),
                flags: vec!["clock_drift"],
                ..Default::default()
            },
        })?;
    }
    g.manifest.camera_sets.push(CameraSet {
        camera: "FUJIFILM X-T3",
        true_offset_secs: -120,
        note: "drifts 300 s per day from +120 s at 2019-08-03 08:00 local",
    });
    Ok(())
}

/// Trip 2: New York (UTC-4). The Canon was left on home time (UTC+2).
fn new_york(g: &mut Gen) -> Result<()> {
    let days = vec![
        (
            NaiveDate::from_ymd_opt(2021, 6, 10).unwrap(),
            vec![stop(40.7580, -73.9855, "Times Square"), stop(40.7812, -73.9665, "Central Park")],
        ),
        (NaiveDate::from_ymd_opt(2021, 6, 11).unwrap(), vec![stop(40.7061, -73.9969, "Brooklyn Bridge"), stop(40.7614, -73.9776, "MoMA")]),
        (
            NaiveDate::from_ymd_opt(2021, 6, 12).unwrap(),
            vec![stop(40.7480, -74.0048, "High Line"), stop(40.6892, -74.0445, "Liberty Island")],
        ),
    ];
    let track = Track::simulate(&days, -240, 21);
    let mut rng = Rng::new(202);
    let times = moments(&track, &mut rng, 24);
    for (i, t) in times.iter().enumerate() {
        let p = track.at(*t);
        let local = to_local(*t, -240);
        let heic = i % 2 == 0;
        g.shot(Shot {
            rel: format!("library/Dump/IMG_{:04}.{}", 4201 + i, if heic { "HEIC" } else { "JPG" }),
            seed: 7000 + i as u64,
            variant: 0,
            size: (2016, 1512),
            heic,
            exif: phone_spec("iPhone 12", local, -240, Some((p.lat, p.lon))),
            item: Item {
                category: Some("photos"),
                true_local: Some(iso(local)),
                true_offset_min: Some(-240),
                true_gps: Some([p.lat, p.lon]),
                camera: Some("Apple iPhone 12".into()),
                ..Default::default()
            },
        })?;
    }
    for (k, i) in (0..times.len()).step_by(1).take(18).enumerate() {
        let true_utc = times[i] + Duration::seconds(rng.range(5, 90));
        let local = to_local(true_utc, -240);
        let file_local = to_local(true_utc, 120);
        g.shot(Shot {
            rel: format!("library/Dump/IMG_{:04}.JPG", 8801 + k),
            seed: 7000 + i as u64,
            variant: 5,
            size: (1800, 1200),
            heic: false,
            exif: camera_spec("Canon", "Canon EOS 80D", "012345678901", file_local, None),
            item: Item {
                category: Some("photos"),
                rule_decidable: true,
                true_local: Some(iso(local)),
                true_offset_min: Some(-240),
                file_local: Some(iso(file_local)),
                camera: Some("Canon EOS 80D".into()),
                flags: vec!["home_time_zone"],
                ..Default::default()
            },
        })?;
    }
    g.manifest.camera_sets.push(CameraSet {
        camera: "Canon EOS 80D",
        true_offset_secs: -6 * 3600,
        note: "clock left on home time (UTC+2) in New York (UTC-4)",
    });
    Ok(())
}

fn home_and_misc(g: &mut Gen) -> Result<()> {
    let mut rng = Rng::new(303);
    // Phone photos at home over the years, including both DST transitions.
    let mut moments: Vec<(NaiveDateTime, i32, &str)> = vec![
        (lt(2021, 3, 28, 3, 30, 0), 120, "iPhone 12"),
        (lt(2021, 10, 31, 2, 30, 0), 120, "iPhone 12"),
        (lt(2021, 10, 31, 2, 30, 0), 60, "iPhone 12"),
    ];
    for _ in 0..30 {
        let y = rng.range(2018, 2024) as i32;
        let d = NaiveDate::from_ymd_opt(y, 1, 1).unwrap() + Duration::days(rng.range(0, 364));
        let summer = (4..=9).contains(&d.month());
        let model = match y {
            2018 | 2019 => "iPhone XS",
            2020..=2022 => "iPhone 12",
            _ => "SM-G991B",
        };
        moments.push((
            d.and_hms_opt(rng.range(9, 21) as u32, rng.range(0, 59) as u32, rng.range(0, 59) as u32).unwrap(),
            if summer { 120 } else { 60 },
            model,
        ));
    }
    for (i, (local, offset, model)) in moments.iter().enumerate() {
        let gps = (HOME.0 + (rng.f() - 0.5) * 0.05, HOME.1 + (rng.f() - 0.5) * 0.08);
        let no_gps = i % 9 == 4;
        let ext = if model.starts_with("SM-") { "jpg" } else { "JPG" };
        let name = if model.starts_with("SM-") {
            format!("{}.{ext}", local.format("%Y%m%d_%H%M%S"))
        } else {
            format!("IMG_{:04}.{ext}", 3001 + i)
        };
        g.shot(Shot {
            rel: format!("library/Dump/{name}"),
            seed: 9000 + i as u64,
            variant: 0,
            size: (1512, 2016),
            heic: false,
            exif: phone_spec(model, *local, *offset, (!no_gps).then_some(gps)),
            item: Item {
                category: Some("photos"),
                true_local: Some(iso(*local)),
                true_offset_min: Some(*offset),
                true_gps: (!no_gps).then_some([gps.0, gps.1]),
                camera: Some(model.to_string()),
                flags: if i < 3 { vec!["dst_boundary"] } else { vec![] },
                ..Default::default()
            },
        })?;
    }

    // A phone photo with no date between two dated neighbors (counter sequence).
    for (n, t) in [(1233, Some(lt(2020, 2, 14, 18, 0, 0))), (1234, None), (1235, Some(lt(2020, 2, 14, 18, 10, 0)))] {
        let mut spec = phone_spec("iPhone 12", t.unwrap_or(lt(2020, 2, 14, 18, 5, 0)), 60, Some((HOME.0, HOME.1)));
        if t.is_none() {
            spec.date = None;
            spec.offset = None;
        }
        g.shot(Shot {
            rel: format!("library/Dump/IMG_{n}.JPG"),
            seed: 9500 + n as u64,
            variant: 0,
            size: (1512, 2016),
            heic: false,
            exif: spec,
            item: Item {
                category: Some("photos"),
                true_local: Some(iso(t.unwrap_or(lt(2020, 2, 14, 18, 5, 0)))),
                true_offset_min: Some(60),
                true_gps: Some([HOME.0, HOME.1]),
                camera: Some("iPhone 12".into()),
                flags: if t.is_none() { vec!["missing_date"] } else { vec![] },
                ..Default::default()
            },
        })?;
    }

    // Panasonic with a reset clock (2000-01-01), plus phone photos of the same moments.
    for k in 0..8u32 {
        let true_local = lt(2020, 5, 17, 14, 0, 0) + Duration::minutes(k as i64 * 7);
        let file_local = lt(2000, 1, 1, 0, 0, 0) + Duration::minutes(k as i64 * 7);
        let gps = (47.3570, 8.5510);
        g.shot(Shot {
            rel: format!("library/Dump/P10{:05}.JPG", 100 + k),
            seed: 9700 + k as u64,
            variant: 9,
            size: (1800, 1200),
            heic: false,
            exif: camera_spec("Panasonic", "DMC-GX80", "WF0123456", file_local, None),
            item: Item {
                category: Some("photos"),
                rule_decidable: true,
                true_local: Some(iso(true_local)),
                true_offset_min: Some(120),
                file_local: Some(iso(file_local)),
                camera: Some("Panasonic DMC-GX80".into()),
                flags: vec!["default_date"],
                ..Default::default()
            },
        })?;
        if k % 2 == 0 {
            let t = true_local + Duration::seconds(40);
            g.shot(Shot {
                rel: format!("library/Dump/IMG_{:04}.HEIC", 5501 + k),
                seed: 9700 + k as u64,
                variant: 0,
                size: (2016, 1512),
                heic: true,
                exif: phone_spec("iPhone 12", t, 120, Some(gps)),
                item: Item {
                    category: Some("photos"),
                    true_local: Some(iso(t)),
                    true_offset_min: Some(120),
                    true_gps: Some([gps.0, gps.1]),
                    camera: Some("iPhone 12".into()),
                    ..Default::default()
                },
            })?;
        }
    }
    g.manifest.camera_sets.push(CameraSet {
        camera: "Panasonic DMC-GX80",
        true_offset_secs: (lt(2020, 5, 17, 14, 0, 0) - lt(2000, 1, 1, 0, 0, 0)).num_seconds(),
        note: "clock reset to 2000-01-01; true start 2020-05-17 14:00 +02:00",
    });

    // A burst.
    let burst = "BURST-7F3E-4A10-9C2B";
    for k in 0..5u32 {
        let t = lt(2020, 7, 4, 16, 20, 11);
        let mut spec = phone_spec("iPhone 12", t, 120, Some((47.3667, 8.5500)));
        spec.maker_note = Some(exifblock::apple_maker_note(None, Some(burst)));
        g.shot(Shot {
            rel: format!("library/Dump/IMG_{:04}.JPG", 3101 + k),
            seed: 9800,
            variant: k as u64,
            size: (1512, 2016),
            heic: false,
            exif: spec,
            item: Item {
                category: Some("photos"),
                true_local: Some(iso(t)),
                true_offset_min: Some(120),
                group: Some(format!("burst:{burst}")),
                camera: Some("iPhone 12".into()),
                ..Default::default()
            },
        })?;
    }

    // Live Photo stills whose videos were lost in an export.
    for k in 0..2u32 {
        let t = lt(2020, 8, 1, 11, 10 + k, 0);
        let mut spec = phone_spec("iPhone 12", t, 120, Some((47.37, 8.54)));
        spec.maker_note = Some(exifblock::apple_maker_note(Some(&format!("LOST-MOTION-{k}")), None));
        g.shot(Shot {
            rel: format!("library/Dump/IMG_{:04}.JPG", 3201 + k),
            seed: 9850 + k as u64,
            variant: 0,
            size: (1512, 2016),
            heic: false,
            exif: spec,
            item: Item {
                category: Some("photos"),
                true_local: Some(iso(t)),
                true_offset_min: Some(120),
                camera: Some("iPhone 12".into()),
                flags: vec!["live_still_only"],
                ..Default::default()
            },
        })?;
    }

    // Edited and original, with an .AAE sidecar, and a FullSizeRender.
    let t = lt(2020, 9, 12, 17, 45, 0);
    let spec = phone_spec("iPhone 12", t, 120, Some((47.37, 8.54)));
    let item = Item {
        category: Some("photos"),
        true_local: Some(iso(t)),
        true_offset_min: Some(120),
        camera: Some("iPhone 12".into()),
        group: Some("edit:IMG_2101".into()),
        ..Default::default()
    };
    g.shot(Shot {
        rel: "library/Dump/IMG_2101.HEIC".into(),
        seed: 9900,
        variant: 0,
        size: (2016, 1512),
        heic: true,
        exif: spec.clone(),
        item: item.clone(),
    })?;
    g.shot(Shot {
        rel: "library/Dump/IMG_E2101.JPG".into(),
        seed: 9900,
        variant: 2,
        size: (2016, 1512),
        heic: false,
        exif: spec.clone(),
        item: Item { flags: vec!["edited"], ..item.clone() },
    })?;
    g.write("library/Dump/IMG_2101.AAE", AAE.as_bytes())?;
    g.push(Item { path: "library/Dump/IMG_2101.AAE".into(), kind: "sidecar", group: item.group.clone(), ..Default::default() });
    let t2 = lt(2020, 9, 12, 18, 2, 0);
    let spec2 = phone_spec("iPhone 12", t2, 120, Some((47.37, 8.54)));
    let item2 = Item {
        category: Some("photos"),
        true_local: Some(iso(t2)),
        true_offset_min: Some(120),
        camera: Some("iPhone 12".into()),
        group: Some("edit:IMG_2103".into()),
        ..Default::default()
    };
    g.shot(Shot {
        rel: "library/Dump/IMG_2103.HEIC".into(),
        seed: 9901,
        variant: 0,
        size: (2016, 1512),
        heic: true,
        exif: spec2.clone(),
        item: item2.clone(),
    })?;
    g.shot(Shot {
        rel: "library/Dump/IMG_2103/FullSizeRender.jpg".into(),
        seed: 9901,
        variant: 2,
        size: (2016, 1512),
        heic: false,
        exif: spec2,
        item: Item { flags: vec!["edited"], ..item2 },
    })?;

    // Existing XMP sidecars, both naming styles.
    for (n, side, tags) in [(4001, "IMG_4001.JPG.xmp", "Places/Garden"), (4002, "IMG_4002.xmp", "People/Family")] {
        let t = lt(2019, 5, 1 + (n - 4001), 10, 0, 0);
        g.shot(Shot {
            rel: format!("library/Dump/IMG_{n}.JPG"),
            seed: 9950 + n as u64,
            variant: 0,
            size: (1512, 2016),
            heic: false,
            exif: phone_spec("iPhone XS", t, 120, None),
            item: Item {
                category: Some("photos"),
                true_local: Some(iso(t)),
                true_offset_min: Some(120),
                tags: vec![tags.into()],
                camera: Some("iPhone XS".into()),
                ..Default::default()
            },
        })?;
        let xmp = format!(
            "<?xpacket begin='\u{feff}' id='W5M0MpCehiHzreSzNTczkc9d'?>\n<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n<rdf:Description rdf:about='' xmlns:xmp='http://ns.adobe.com/xap/1.0/' xmlns:digiKam='http://www.digikam.org/ns/1.0/' xmp:Rating='4'>\n<digiKam:TagsList><rdf:Seq><rdf:li>{tags}</rdf:li></rdf:Seq></digiKam:TagsList>\n</rdf:Description></rdf:RDF></x:xmpmeta>\n<?xpacket end='w'?>\n"
        );
        g.write(&format!("library/Dump/{side}"), xmp.as_bytes())?;
        g.push(Item { path: format!("library/Dump/{side}"), kind: "sidecar", ..Default::default() });
    }

    // Google Takeout: images without EXIF, the truth in JSON sidecars.
    for k in 0..4u32 {
        let t = lt(2020, 7, 4, 15, 30 + k * 3, 0);
        let utc = to_utc(t, 120);
        let name = format!("PXL_20200704_{}.jpg", (t - Duration::hours(2)).format("%H%M%S000"));
        let rel = format!("library/Takeout/Google Photos/Photos from 2020/{name}");
        let img = pixels::scene(1512, 2016, 9960 + k as u64, 0);
        g.plain_jpeg(&rel, &img, 85, None)?;
        let (lat, lon) = (46.0207 + k as f64 * 0.001, 7.7491);
        let json = serde_json::json!({
            "title": name, "description": "",
            "photoTakenTime": {"timestamp": utc.timestamp().to_string(), "formatted": ""},
            "geoData": {"latitude": lat, "longitude": lon, "altitude": 1600.0},
            "geoDataExif": {"latitude": lat, "longitude": lon, "altitude": 1600.0},
            "favorited": k == 0
        });
        let json_name = if k == 3 { format!("{name}.supplemental-metadata.json") } else { format!("{name}.json") };
        g.write(&format!("library/Takeout/Google Photos/Photos from 2020/{json_name}"), serde_json::to_string_pretty(&json)?.as_bytes())?;
        g.push(Item {
            path: rel,
            kind: "photo",
            category: Some("photos"),
            true_local: Some(iso(t)),
            true_offset_min: Some(120),
            true_gps: Some([lat, lon]),
            tags: if k == 0 { vec!["Favorite".into()] } else { vec![] },
            flags: vec!["takeout", "no_metadata"],
            ..Default::default()
        });
        g.push(Item { path: format!("library/Takeout/Google Photos/Photos from 2020/{json_name}"), kind: "sidecar", ..Default::default() });
    }

    // Documents photographed with a phone.
    for k in 0..6u32 {
        let t = lt(2021, 1 + k, 5 + k, 12, 0, 0);
        let img = pixels::document(1512, 2016, 9970 + k as u64, k >= 4);
        let spec = phone_spec("iPhone 12", t, 60, Some((HOME.0, HOME.1)));
        let rel = format!("library/Dump/IMG_{:04}.JPG", 6001 + k);
        g.plain_jpeg(&rel, &img, 85, Some(ExifSpec { width: Some(1512), height: Some(2016), ..spec }))?;
        g.push(Item {
            path: rel,
            kind: "photo",
            category: Some("documents"),
            has_gps: true,
            true_gps: Some([HOME.0, HOME.1]),
            true_local: Some(iso(t)),
            true_offset_min: Some(60),
            camera: Some("iPhone 12".into()),
            ..Default::default()
        });
    }

    // Junk.
    let junk: [(&str, JunkKind, (u32, u32)); 5] = [
        ("IMG_6101.JPG", JunkKind::Black, (1512, 2016)),
        ("IMG_6102.JPG", JunkKind::White, (1512, 2016)),
        ("IMG_6103.JPG", JunkKind::Blurred, (1512, 2016)),
        ("IMG_6104.JPG", JunkKind::Blurred, (2016, 1512)),
        ("IMG_6105.JPG", JunkKind::Pocket, (1512, 2016)),
    ];
    for (i, (name, kind, (w, h))) in junk.into_iter().enumerate() {
        let t = lt(2022, 3, 3, 10 + i as u32, 0, 0);
        let img = pixels::junk(w, h, 9980 + i as u64, kind);
        let rel = format!("library/Dump/{name}");
        g.plain_jpeg(&rel, &img, 85, Some(phone_spec("iPhone 12", t, 60, Some((HOME.0, HOME.1)))))?;
        g.push(Item {
            path: rel,
            kind: "photo",
            category: Some("junk"),
            has_gps: true,
            true_gps: Some([HOME.0, HOME.1]),
            true_local: Some(iso(t)),
            true_offset_min: Some(60),
            ..Default::default()
        });
    }
    let tiny = pixels::scene(120, 90, 9990, 0);
    g.write("library/Dump/tiny.png", &pixels::png(&tiny))?;
    g.push(Item { path: "library/Dump/tiny.png".into(), kind: "photo", category: Some("junk"), flags: vec!["tiny"], ..Default::default() });
    let full = pixels::jpeg(&pixels::scene(1512, 2016, 9991, 0), 85);
    g.write("library/Dump/IMG_9999.JPG", &full[..full.len() / 3])?;
    g.push(Item {
        path: "library/Dump/IMG_9999.JPG".into(),
        kind: "photo",
        category: Some("junk"),
        flags: vec!["broken"],
        ..Default::default()
    });

    // An event folder copied in by hand, never checked.
    for k in 0..10u32 {
        let t = lt(2018, 12, 24, 17, 0, 0) + Duration::minutes(k as i64 * 11);
        g.shot(Shot {
            rel: format!("library/2018/2018-12 Christmas/IMG_{:04}.JPG", 101 + k),
            seed: 9400 + k as u64,
            variant: 0,
            size: (1600, 1200),
            heic: false,
            exif: camera_spec("Canon", "Canon PowerShot G7 X", "5432109876", t, Some(60)),
            item: Item {
                category: Some("photos"),
                rule_decidable: true,
                true_local: Some(iso(t)),
                true_offset_min: Some(60),
                camera: Some("Canon PowerShot G7 X".into()),
                flags: vec!["unchecked_event"],
                ..Default::default()
            },
        })?;
    }

    // Videos: HEVC MP4 from an Android phone, and a rotated iPhone clip.
    let t = lt(2023, 6, 18, 19, 5, 0);
    let img = pixels::scene(1280, 720, 9601, 0);
    let utc = exif_fmt(to_utc(t, 120).naive_utc());
    g.video(
        "library/Dump/20230618_190500.mp4",
        &img,
        2.0,
        true,
        true,
        &[
            "-Keys:Make=samsung".into(),
            "-Keys:Model=SM-G991B".into(),
            format!("-QuickTime:CreateDate={utc}"),
            format!("-QuickTime:MediaCreateDate={utc}"),
            format!("-Keys:GPSCoordinates={}, {}, 410", HOME.0, HOME.1),
        ],
        Item {
            category: Some("photos"),
            true_local: Some(iso(t)),
            true_offset_min: Some(120),
            true_gps: Some([HOME.0, HOME.1]),
            camera: Some("SM-G991B".into()),
            ..Default::default()
        },
    )?;
    let t = lt(2022, 8, 20, 12, 0, 0);
    let img = pixels::scene(1280, 720, 9602, 0);
    let mut meta = Gen::iphone_video_meta("iPhone 12", t, 120, Some((46.5197, 6.6323)), None);
    meta.push("-Rotation=90".into());
    g.video(
        "library/Dump/IMG_7001.MOV",
        &img,
        2.0,
        false,
        true,
        &meta,
        Item {
            category: Some("photos"),
            true_local: Some(iso(t)),
            true_offset_min: Some(120),
            true_gps: Some([46.5197, 6.6323]),
            camera: Some("iPhone 12".into()),
            flags: vec!["rotated"],
            ..Default::default()
        },
    )?;
    Ok(())
}

const AAE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>adjustmentFormatIdentifier</key><string>com.apple.photo</string><key>adjustmentFormatVersion</key><string>1.5</string></dict></plist>\n";

/// Screenshots, memes, saved social posts, wallpapers.
fn screens_and_saved(g: &mut Gen) -> Result<()> {
    // iOS PNG screenshots with the XMP marker.
    for k in 0..6u32 {
        let t = lt(2021, 4, 2 + k, 9 + k, 15, 0);
        let rel = format!("library/Dump/IMG_{:04}.PNG", 5001 + k);
        g.write(&rel, &pixels::png(&pixels::screenshot(1170, 2532, 5001 + k as u64, k % 2 == 1)))?;
        g.exiftool(&rel, &["-XMP-exif:UserComment=Screenshot".into(), format!("-XMP-photoshop:DateCreated={}{}", exif_fmt(t), off(120))])?;
        g.push(Item {
            path: rel,
            kind: "photo",
            category: Some("screenshots"),
            rule_decidable: true,
            true_local: Some(iso(t)),
            true_offset_min: Some(120),
            ..Default::default()
        });
    }
    // A game played years ago, also iOS screenshots (for Teach examples).
    for k in 0..8u32 {
        let t = lt(2016, 7, 20 + k / 2, 18, 5 * k, 0);
        let rel = format!("library/Dump/IMG_{:04}.PNG", 640 + k);
        g.write(&rel, &pixels::png(&pixels::game_screen(750, 1334, 640 + k as u64)))?;
        g.exiftool(&rel, &["-XMP-exif:UserComment=Screenshot".into(), format!("-XMP-photoshop:DateCreated={}{}", exif_fmt(t), off(120))])?;
        g.push(Item {
            path: rel,
            kind: "photo",
            category: Some("screenshots"),
            rule_decidable: true,
            tags: vec!["Category/Screenshots/Pokémon GO".into()],
            true_local: Some(iso(t)),
            true_offset_min: Some(120),
            flags: vec!["teach_example"],
            ..Default::default()
        });
    }
    // Export-tool JPEG screenshots with the EXIF marker, in a flat year folder.
    for k in 0..6u32 {
        let t = lt(2020, 10, 1 + k * 3, 20, 4, 0);
        let rel = format!("library/2020/IMG_iPhone11_2020 - {}.jpeg", 12 + k);
        let spec = ExifSpec {
            date: Some(exif_fmt(t)),
            offset: Some(off(120)),
            user_comment: Some("Screenshot".into()),
            width: Some(828),
            height: Some(1792),
            ..Default::default()
        };
        g.plain_jpeg(&rel, &pixels::screenshot(828, 1792, 5100 + k as u64, k % 3 == 0), 90, Some(spec))?;
        g.push(Item {
            path: rel,
            kind: "photo",
            category: Some("screenshots"),
            rule_decidable: true,
            true_local: Some(iso(t)),
            true_offset_min: Some(120),
            ..Default::default()
        });
    }
    // The size-signature group: saved images, all 854×1590, one encoder.
    for k in 0..24u32 {
        let t = lt(2020, 3, 1, 21, 0, 0) + Duration::hours(k as i64 * 37);
        let rel = format!("library/2020/IMG_iPhone11_2020 - {}.jpeg", 200 + k);
        let spec = ExifSpec { date: Some(exif_fmt(t)), offset: Some(off(60)), width: Some(854), height: Some(1590), ..Default::default() };
        g.plain_jpeg(&rel, &pixels::social(854, 1590, 5200 + k as u64), 82, Some(spec))?;
        g.push(Item {
            path: rel,
            kind: "photo",
            category: Some("social_media"),
            file_local: Some(iso(t)),
            flags: vec!["signature_854x1590", "saved_date"],
            ..Default::default()
        });
    }
    // Android screenshots.
    for k in 0..4u32 {
        let t = lt(2023, 3, 1, 10, 15 + k, 0);
        let rel = format!("library/Dump/Screenshot_{}.png", t.format("%Y%m%d-%H%M%S"));
        g.write(&rel, &pixels::png(&pixels::screenshot(1080, 2340, 5300 + k as u64, k % 2 == 0)))?;
        g.push(Item {
            path: rel,
            kind: "photo",
            category: Some("screenshots"),
            rule_decidable: true,
            true_local: Some(iso(t)),
            ..Default::default()
        });
    }
    // Screen recordings.
    for (k, name) in ["RPReplay_Final1612345678.MP4", "Screen_Recording_20210812-140322_Chrome.mp4"].iter().enumerate() {
        let img = pixels::screenshot(590, 1280, 5400 + k as u64, false);
        g.video(
            &format!("library/Dump/{name}"),
            &img,
            3.0,
            false,
            false,
            &[],
            Item { category: Some("screenshots"), rule_decidable: true, ..Default::default() },
        )?;
    }
    // Memes, received through chat apps.
    let meme_names = [
        "IMG-20200102-WA0004.jpg",
        "received_1029384756.jpeg",
        "IMG-20210315-WA0012.jpg",
        "meme_01.jpg",
        "IMG-20220101-WA0001.jpg",
        "received_5647382910.jpeg",
        "IMG-20190905-WA0021.jpg",
        "funny.jpg",
    ];
    for (k, name) in meme_names.iter().enumerate() {
        let (w, h) = *Rng::new(5500 + k as u64).pick(&[(500, 500), (640, 800), (720, 720), (1080, 1080), (600, 700)]);
        let rel = format!("library/Dump/{name}");
        g.plain_jpeg(&rel, &pixels::meme(w, h, 5500 + k as u64), 78, None)?;
        let source = if name.contains("-WA") {
            vec!["Source/WhatsApp".to_string()]
        } else if name.starts_with("received_") {
            vec!["Source/Messenger".to_string()]
        } else {
            vec![]
        };
        g.push(Item { path: rel, kind: "photo", category: Some("memes"), tags: source, ..Default::default() });
    }
    for k in 0..2u32 {
        let rel = format!("library/Dump/reaction_{k}.gif");
        let path = g.abs(&rel)?;
        let file = std::fs::File::create(path)?;
        let mut enc = image::codecs::gif::GifEncoder::new(file);
        enc.set_repeat(image::codecs::gif::Repeat::Infinite)?;
        for f in 0..3u64 {
            let frame = image::DynamicImage::ImageRgb8(pixels::meme(320, 320, 5600 + k as u64 * 10 + f)).to_rgba8();
            enc.encode_frame(image::Frame::new(frame))?;
        }
        g.push(Item { path: rel, kind: "photo", category: Some("memes"), ..Default::default() });
    }
    let t = lt(2020, 1, 2, 19, 30, 0);
    g.video(
        "library/Dump/VID-20200102-WA0003.mp4",
        &pixels::meme(480, 848, 5700),
        4.0,
        false,
        true,
        &[format!("-QuickTime:CreateDate={}", exif_fmt(to_utc(t, 60).naive_utc()))],
        Item {
            category: Some("memes"),
            tags: vec!["Source/WhatsApp".into()],
            file_local: Some(iso(t)),
            file_offset_min: Some(60),
            flags: vec!["saved_date"],
            ..Default::default()
        },
    )?;
    // Saved social posts.
    for k in 0..8u32 {
        let rel = format!("library/Dump/{}_{}_n.jpg", 330_000_000 + k * 7919, 1_000_000 + k);
        g.plain_jpeg(&rel, &pixels::social(1080, 1350, 5800 + k as u64), 80, None)?;
        g.push(Item { path: rel, kind: "photo", category: Some("social_media"), ..Default::default() });
    }
    for (k, ms) in [1_565_611_402_000u64, 1_598_000_000_000].iter().enumerate() {
        let rel = format!("library/Dump/FB_IMG_{ms}.jpg");
        g.plain_jpeg(&rel, &pixels::social(960, 1200, 5850 + k as u64), 80, None)?;
        g.push(Item {
            path: rel,
            kind: "photo",
            category: Some("social_media"),
            tags: vec!["Source/Facebook".into()],
            ..Default::default()
        });
    }
    for k in 0..2u32 {
        let rel = format!("library/Dump/IMG_{:04}.JPG", 7301 + k);
        let spec = ExifSpec { software: Some("Instagram".into()), width: Some(1080), height: Some(1080), ..Default::default() };
        g.plain_jpeg(&rel, &pixels::social(1080, 1080, 5870 + k as u64), 80, Some(spec))?;
        g.push(Item {
            path: rel,
            kind: "photo",
            category: Some("social_media"),
            tags: vec!["Source/Instagram".into()],
            ..Default::default()
        });
    }
    for k in 0..2u32 {
        let rel = format!("library/Dump/{}.mp4", 8_000_000_000u64 + k as u64 * 1013);
        g.video(
            &rel,
            &pixels::social(576, 1024, 5880 + k as u64),
            3.0,
            false,
            true,
            &[],
            Item { category: Some("social_media"), ..Default::default() },
        )?;
    }
    // Wallpapers.
    for (k, (w, h, ext)) in [(1170u32, 2532u32, "jpg"), (2560, 1440, "jpg"), (1920, 1080, "png"), (1284, 2778, "jpg")].iter().enumerate() {
        let rel = format!("library/Dump/wallpaper_{:02}.{ext}", k + 1);
        let img = pixels::wallpaper(*w, *h, 5900 + k as u64);
        if *ext == "png" {
            g.write(&rel, &pixels::png(&img))?;
        } else {
            g.plain_jpeg(&rel, &img, 92, None)?;
        }
        g.push(Item { path: rel, kind: "photo", category: Some("wallpapers"), ..Default::default() });
    }
    Ok(())
}

fn duplicates_and_paths(g: &mut Gen) -> Result<()> {
    let items = g.manifest.items.clone();
    let phone: Vec<&Item> = items.iter().filter(|i| i.path.contains("/Dump/IMG_10") && i.path.ends_with(".JPG")).take(5).collect();
    for it in &phone[..3] {
        let name = it.path.rsplit('/').next().unwrap();
        let rel = format!("library/Backup/{name}");
        std::fs::copy(g.out.join(&it.path), g.abs(&rel)?)?;
        g.push(Item { path: rel, duplicate_of: Some(it.path.clone()), flags: vec!["exact_copy"], ..(*it).clone() });
    }
    for it in &phone[3..5] {
        let img = image::open(g.out.join(&it.path))?.resize(1024, 1024, image::imageops::FilterType::Triangle).to_rgb8();
        let name = it.path.rsplit('/').next().unwrap();
        let rel = format!("library/Backup/resized/{name}");
        g.plain_jpeg(&rel, &img, 75, None)?;
        g.push(Item {
            path: rel,
            duplicate_of: Some(it.path.clone()),
            has_gps: false,
            flags: vec!["resized_copy", "no_metadata"],
            ..(*it).clone()
        });
    }
    let nikon: Vec<&Item> = items.iter().filter(|i| i.path.contains("Italy 2019/DSC_00")).take(2).collect();
    for (k, it) in nikon.iter().enumerate() {
        let img = image::open(g.out.join(&it.path))?.resize(1600, 1600, image::imageops::FilterType::Triangle).to_rgb8();
        let rel = format!("library/Dump/IMG-20190805-WA{:04}.jpg", 7 + k);
        g.plain_jpeg(&rel, &img, 70, None)?;
        g.push(Item {
            path: rel,
            duplicate_of: Some(it.path.clone()),
            tags: vec!["Source/WhatsApp".into()],
            file_local: None,
            has_gps: false,
            camera: None,
            // The category follows from the camera original (duplicates),
            // not from a rule: the WhatsApp name only sets the source.
            rule_decidable: false,
            flags: vec!["whatsapp_copy", "no_metadata"],
            ..(*it).clone()
        });
    }

    // Awkward names and paths.
    let t = lt(2020, 6, 6, 12, 0, 0);
    for (k, name) in ["Zürich Ünïcødé 写真 2020.jpg", "Spaß & Café — été.heic"].iter().enumerate() {
        g.shot(Shot {
            rel: format!("library/Dump/{name}"),
            seed: 9200 + k as u64,
            variant: 0,
            size: (1512, 2016),
            heic: name.ends_with("heic"),
            exif: phone_spec("iPhone 12", t, 120, Some((47.3769, 8.5417))),
            item: Item {
                category: Some("photos"),
                true_local: Some(iso(t)),
                true_offset_min: Some(120),
                true_gps: Some([47.3769, 8.5417]),
                flags: vec!["unicode_name"],
                ..Default::default()
            },
        })?;
    }
    let long = "A very long folder name used to test paths beyond the classic Windows limit of 260 characters";
    let rel = format!("library/Dump/{long}/{long}/{long} 2021/IMG_8001.JPG");
    let t = lt(2021, 9, 9, 9, 9, 9);
    g.shot(Shot {
        rel: rel.clone(),
        seed: 9210,
        variant: 0,
        size: (1512, 2016),
        heic: false,
        exif: phone_spec("iPhone 12", t, 120, None),
        item: Item {
            category: Some("photos"),
            true_local: Some(iso(t)),
            true_offset_min: Some(120),
            flags: vec!["long_path"],
            ..Default::default()
        },
    })?;
    // Case-only collision: both land in the same year folder.
    for (k, rel) in ["library/Imports/A/DSC07001.JPG", "library/Imports/B/dsc07001.jpg"].iter().enumerate() {
        let t = lt(2021, 11, 20, 15, k as u32, 0);
        g.shot(Shot {
            rel: rel.to_string(),
            seed: 9220 + k as u64,
            variant: 0,
            size: (1800, 1200),
            heic: false,
            exif: camera_spec("SONY", "ILCE-7M3", "4012345", t, Some(60)),
            item: Item {
                category: Some("photos"),
                rule_decidable: true,
                true_local: Some(iso(t)),
                true_offset_min: Some(60),
                camera: Some("SONY ILCE-7M3".into()),
                flags: vec!["case_collision"],
                ..Default::default()
            },
        })?;
    }
    // A read-only file.
    let t = lt(2021, 12, 24, 18, 0, 0);
    let rel = "library/Dump/IMG_7100.JPG".to_string();
    g.shot(Shot {
        rel: rel.clone(),
        seed: 9230,
        variant: 0,
        size: (1512, 2016),
        heic: false,
        exif: phone_spec("iPhone 12", t, 60, Some((HOME.0, HOME.1))),
        item: Item {
            category: Some("photos"),
            true_local: Some(iso(t)),
            true_offset_min: Some(60),
            true_gps: Some([HOME.0, HOME.1]),
            flags: vec!["read_only"],
            ..Default::default()
        },
    })?;
    let p = g.out.join(&rel);
    let mut perm = std::fs::metadata(&p)?.permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&p, perm)?;
    Ok(())
}

/// 500 camera photos without GPS on a hike, with a GPX track of it.
fn hike(g: &mut Gen) -> Result<()> {
    let start_local = lt(2022, 7, 16, 9, 0, 0);
    let start = to_utc(start_local, 120);
    let way = [
        (46.6000, 11.7230, 2500.0),
        (46.6140, 11.7360, 2450.0),
        (46.6200, 11.7590, 2300.0),
        (46.6030, 11.7800, 2100.0),
        (46.5800, 11.7660, 1900.0),
    ];
    let track = Track::path(start, 120, &way, 7 * 60, 10);
    g.write("location-history/dolomites-hike-2022-07-16.gpx", track.gpx("Dolomites hike", 1).as_bytes())?;
    g.manifest.tracks.push(TrackFile {
        path: "location-history/dolomites-hike-2022-07-16.gpx".into(),
        format: "gpx",
        covers: "500 Sony photos, 2022-07-16 09:00-16:00 +02:00",
    });
    let templates: Vec<Vec<u8>> = (0..20).map(|k| pixels::jpeg(&pixels::scene(480, 320, 3000 + k, 0), 80)).collect();
    for k in 0..500u32 {
        let t = start + Duration::seconds(k as i64 * 50 + 7);
        let p = track.at(t);
        let local = to_local(t, 120);
        let spec = ExifSpec { width: Some(480), height: Some(320), ..camera_spec("SONY", "ILCE-7M3", "4012345", local, Some(120)) };
        let rel = format!("library/Dump/Sony/DSC{:05}.JPG", 1001 + k);
        g.write(&rel, &exifblock::insert_into_jpeg(&templates[k as usize % templates.len()], &exifblock::tiff(&spec)))?;
        g.push(Item {
            path: rel,
            kind: "photo",
            category: Some("photos"),
            rule_decidable: true,
            true_local: Some(iso(local)),
            true_offset_min: Some(120),
            true_gps: Some([p.lat, p.lon]),
            camera: Some("SONY ILCE-7M3".into()),
            flags: vec!["gpx_covered"],
            ..Default::default()
        });
    }
    Ok(())
}

/// Phone photos of a Lisbon weekend in the library, and a camera card
/// outside it with a clock 9 h 30 min behind and no GPS.
fn camera_card(g: &mut Gen) -> Result<()> {
    let days = vec![
        (NaiveDate::from_ymd_opt(2022, 5, 20).unwrap(), vec![stop(38.7139, -9.1334, "Alfama"), stop(38.6916, -9.2160, "Belém")]),
        (NaiveDate::from_ymd_opt(2022, 5, 21).unwrap(), vec![stop(38.7107, -9.1436, "Baixa"), stop(38.7979, -9.3907, "Sintra")]),
    ];
    let track = Track::simulate(&days, 60, 31);
    let mut rng = Rng::new(404);
    let times = moments(&track, &mut rng, 16);
    for (i, t) in times.iter().enumerate() {
        let p = track.at(*t);
        let local = to_local(*t, 60);
        g.shot(Shot {
            rel: format!("library/Dump/IMG_{:04}.HEIC", 9101 + i),
            seed: 8000 + i as u64,
            variant: 0,
            size: (2016, 1512),
            heic: true,
            exif: phone_spec("iPhone 12", local, 60, Some((p.lat, p.lon))),
            item: Item {
                category: Some("photos"),
                true_local: Some(iso(local)),
                true_offset_min: Some(60),
                true_gps: Some([p.lat, p.lon]),
                camera: Some("iPhone 12".into()),
                ..Default::default()
            },
        })?;
    }
    for k in 0..40usize {
        let i = k % times.len();
        let true_utc = times[i] + Duration::seconds(20 + (k / times.len()) as i64 * 95);
        let local = to_local(true_utc, 60);
        let file_local = local - Duration::minutes(9 * 60 + 30);
        let rel = format!("camera-card/DCIM/100CANON/IMG_{:04}.JPG", k + 1);
        g.shot(Shot {
            rel,
            seed: 8000 + i as u64,
            variant: 11 + (k / times.len()) as u64,
            size: (1800, 1200),
            heic: false,
            exif: camera_spec("Canon", "Canon EOS R6", "033021000123", file_local, None),
            item: Item {
                category: Some("photos"),
                rule_decidable: true,
                true_local: Some(iso(local)),
                true_offset_min: Some(60),
                true_gps: Some([track.at(true_utc).lat, track.at(true_utc).lon]),
                file_local: Some(iso(file_local)),
                camera: Some("Canon EOS R6".into()),
                flags: vec!["camera_card", "wrong_clock"],
                ..Default::default()
            },
        })?;
    }
    g.manifest.camera_sets.push(CameraSet {
        camera: "Canon EOS R6",
        true_offset_secs: 9 * 3600 + 1800,
        note: "camera card for the import flow; clock 9 h 30 min behind; Lisbon, UTC+1",
    });
    Ok(())
}
