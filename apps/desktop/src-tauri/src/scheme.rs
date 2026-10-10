//! The `roll://` scheme (`http://roll.localhost/` on Windows). Serves
//! thumbnails and previews from `thumbs.db`, full images and videos from
//! disk with range requests.
//!
//! Paths: `thumb/<id>`, `preview/<id>`, `full/<id>`, `video/<id>?native=1`.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use roll_core::Library;
use tauri::http::{header, Request, Response, StatusCode};

const MAX_CHUNK: u64 = 4 * 1024 * 1024;

fn status(code: StatusCode, msg: &str) -> Response<Vec<u8>> {
    Response::builder().status(code).header(header::CONTENT_TYPE, "text/plain").body(msg.as_bytes().to_vec()).unwrap()
}

fn bytes(mime: &str, data: Vec<u8>, immutable: bool) -> Response<Vec<u8>> {
    let cache = if immutable { "public, max-age=31536000, immutable" } else { "no-cache" };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, cache)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(data)
        .unwrap()
}

/// Serve a file, honoring `Range` so videos can seek.
fn file(path: &Path, mime: &str, range: Option<&str>) -> Response<Vec<u8>> {
    let Ok(mut f) = std::fs::File::open(path) else { return status(StatusCode::NOT_FOUND, "missing file") };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let parsed = range.and_then(|r| r.strip_prefix("bytes=")).and_then(|r| {
        let (a, b) = r.split_once('-')?;
        let start: u64 = if a.is_empty() { len.saturating_sub(b.parse().ok()?) } else { a.parse().ok()? };
        let end: u64 = if a.is_empty() || b.is_empty() { len.saturating_sub(1) } else { b.parse::<u64>().ok()?.min(len.saturating_sub(1)) };
        (start <= end && start < len).then_some((start, end))
    });
    match parsed {
        Some((start, end)) => {
            let end = end.min(start + MAX_CHUNK - 1);
            let mut buf = vec![0u8; (end - start + 1) as usize];
            if f.seek(SeekFrom::Start(start)).and_then(|_| f.read_exact(&mut buf)).is_err() {
                return status(StatusCode::INTERNAL_SERVER_ERROR, "read failed");
            }
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, mime)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
                .header(header::CONTENT_LENGTH, buf.len().to_string())
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(buf)
                .unwrap()
        }
        None if range.is_some() => Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{len}"))
            .body(Vec::new())
            .unwrap(),
        None => {
            let mut data = Vec::with_capacity(len as usize);
            if f.read_to_end(&mut data).is_err() {
                return status(StatusCode::INTERNAL_SERVER_ERROR, "read failed");
            }
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(data)
                .unwrap()
        }
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn handle(lib: &Arc<Library>, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let path = percent_decode(req.uri().path());
    let query = req.uri().query().unwrap_or("");
    let mut parts = path.trim_start_matches('/').split('/');
    let (Some(kind), Some(id)) = (parts.next(), parts.next().and_then(|s| s.parse::<i64>().ok())) else {
        return status(StatusCode::BAD_REQUEST, "bad path");
    };
    let range = req.headers().get(header::RANGE).and_then(|v| v.to_str().ok());
    let result = match kind {
        "thumb" => lib.thumb(id).map(|d| d.map(|d| bytes("image/webp", d, true))),
        "preview" => lib.preview(id).map(|d| d.map(|d| bytes("image/webp", d, true))),
        "full" => lib.full_image(id).map(|f| f.map(|(p, mime)| file(&p, mime, range))),
        "video" => {
            let native = query.split('&').any(|kv| kv == "native=1");
            lib.playable_video(id, native).map(|f| f.map(|(p, mime)| file(&p, mime, range)))
        }
        _ => return status(StatusCode::NOT_FOUND, "unknown"),
    };
    match result {
        Ok(Some(r)) => r,
        Ok(None) => status(StatusCode::NOT_FOUND, "not found"),
        Err(e) => {
            tracing::warn!("roll://{kind}/{id}: {e}");
            status(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        let dir = std::env::temp_dir().join(format!("roll-scheme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("v.bin");
        std::fs::write(&p, (0..100u8).collect::<Vec<_>>()).unwrap();
        let r = file(&p, "video/mp4", Some("bytes=10-19"));
        assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(r.body(), &(10..20u8).collect::<Vec<_>>());
        let r = file(&p, "video/mp4", Some("bytes=90-"));
        assert_eq!(r.body().len(), 10);
        let r = file(&p, "video/mp4", Some("bytes=-5"));
        assert_eq!(r.body(), &[95, 96, 97, 98, 99]);
        assert_eq!(file(&p, "video/mp4", Some("bytes=200-")).status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(file(&p, "video/mp4", None).body().len(), 100);
        assert_eq!(percent_decode("thumb%2F12"), "thumb/12");
        std::fs::remove_dir_all(dir).ok();
    }
}
