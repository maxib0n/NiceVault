//! `nvault://<vault>/<node>` (Windows: `http://nvault.localhost/<vault>/<node>`): decrypted
//! bytes straight from the store into the webview, with HTTP Range. Nothing touches disk.

use crate::state::AppState;
use nv_fs::{Fs, Kind, NodeId};
use tauri::http::{Request, Response, StatusCode, header};

// Tauri 2's protocol responder takes the whole body as one buffer (no streaming bodies), so
// memory is bounded by capping every response body instead:
/// Largest body for one open-ended Range request (media elements ask for more as they play),
/// and for an audio/video request without Range.
pub const MAX_RANGE: u64 = 8 << 20;
/// Largest body for any other request (explicit ranges, or no Range on an image/PDF/text).
/// Bigger files get 206 + the first slice + `Accept-Ranges`, and the client asks for the rest.
pub const MAX_BODY: u64 = 64 << 20;

pub fn mime_for(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "heic" => "image/heic",
        "tif" | "tiff" => "image/tiff",
        "pdf" => "application/pdf",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "ogv" => "video/ogg",
        "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "wav" => "audio/wav",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "flac" => "audio/flac",
        "txt" | "log" | "ini" | "cfg" | "conf" => "text/plain; charset=utf-8",
        "md" | "markdown" => "text/markdown; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "json" => "application/json",
        "html" | "htm" | "xml" | "xhtml" => "text/plain; charset=utf-8", // never let a vault file run as a page
        "css" | "js" | "ts" | "rs" | "py" | "sh" | "c" | "h" | "cpp" | "java" | "go" | "toml" | "yaml" | "yml" => {
            "text/plain; charset=utf-8"
        }
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "odt" => "application/vnd.oasis.opendocument.text",
        _ => "application/octet-stream",
    }
}

pub(crate) fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// (vault id, node) from either URL form.
pub fn parse_target(uri: &tauri::http::Uri) -> Option<(String, NodeId)> {
    let host = uri.host().unwrap_or("");
    let segs: Vec<&str> = uri.path().split('/').filter(|s| !s.is_empty()).collect();
    let (vault, node) = if host.is_empty() || host == "nvault.localhost" || host == "localhost" {
        (*segs.first()?, *segs.get(1)?)
    } else {
        (host, *segs.first()?)
    };
    Some((percent_decode(vault), node.parse().ok()?))
}

/// `bytes=a-b`, `bytes=a-`, `bytes=-n` -> inclusive (start, end). Err = unsatisfiable.
fn parse_range(h: &str, size: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = h.trim().strip_prefix("bytes=") else { return Ok(None) };
    let first = spec.split(',').next().unwrap_or("").trim();
    let (a, b) = first.split_once('-').ok_or(())?;
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().map_err(|_| ())?;
        if n == 0 {
            return Err(());
        }
        (size.saturating_sub(n.min(MAX_BODY)), size.checked_sub(1).ok_or(())?)
    } else {
        let s: u64 = a.parse().map_err(|_| ())?;
        let e = if b.is_empty() {
            size.saturating_sub(1).min(s.saturating_add(MAX_RANGE - 1))
        } else {
            // A shorter range than asked is legal: Content-Range says what was sent.
            b.parse::<u64>().map_err(|_| ())?.min(size.saturating_sub(1)).min(s.saturating_add(MAX_BODY - 1))
        };
        (s, e)
    };
    if size == 0 || start >= size || start > end {
        return Err(());
    }
    Ok(Some((start, end)))
}

/// Even if a vault file is ever loaded as a document (SVG, a frame), it runs nothing.
const CSP: &str = "default-src 'none'; img-src data:; style-src 'unsafe-inline'; sandbox";

/// The app's own origin (echoed when the request comes from it), never `*`.
fn allow_origin(req: &Request<Vec<u8>>) -> &'static str {
    const OURS: [&str; 3] = ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"];
    let o = req.headers().get(header::ORIGIN).and_then(|h| h.to_str().ok()).unwrap_or("");
    if let Some(&m) = OURS.iter().find(|&&x| x == o) {
        return m;
    }
    if cfg!(debug_assertions) && o == "http://localhost:5173" {
        return "http://localhost:5173"; // devUrl
    }
    if cfg!(windows) { OURS[1] } else { OURS[0] }
}

fn base(status: StatusCode, origin: &str) -> tauri::http::response::Builder {
    Response::builder()
        .status(status)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin)
        .header(header::VARY, "Origin")
        .header(header::CONTENT_SECURITY_POLICY, CSP)
        .header("X-Content-Type-Options", "nosniff")
}

fn plain(status: StatusCode, origin: &str, msg: &str) -> Response<Vec<u8>> {
    base(status, origin).header(header::CONTENT_TYPE, "text/plain; charset=utf-8").body(msg.as_bytes().to_vec()).unwrap()
}

fn unsatisfiable(size: u64, origin: &str) -> Response<Vec<u8>> {
    base(StatusCode::RANGE_NOT_SATISFIABLE, origin).header(header::CONTENT_RANGE, format!("bytes */{size}")).body(Vec::new()).unwrap()
}

pub fn handle(state: &AppState, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let origin = allow_origin(req);
    if req.method() == tauri::http::Method::OPTIONS {
        return base(StatusCode::NO_CONTENT, origin).header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Range").body(Vec::new()).unwrap();
    }
    if let Some(mut r) = crate::pm_icons::serve(state, req) {
        // site icons get the same headers as file previews (app origin, never `*`; CSP)
        let h = r.headers_mut();
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, header::HeaderValue::from_static(origin));
        h.insert(header::VARY, header::HeaderValue::from_static("Origin"));
        h.insert(header::CONTENT_SECURITY_POLICY, header::HeaderValue::from_static(CSP));
        h.insert("X-Content-Type-Options", header::HeaderValue::from_static("nosniff"));
        return r;
    }
    let Some((vid, node)) = parse_target(req.uri()) else { return plain(StatusCode::BAD_REQUEST, origin, "bad url") };
    let Ok(v) = state.vault(&vid) else { return plain(StatusCode::NOT_FOUND, origin, "locked") };
    let n = match v.getattr(node) {
        Ok(n) if n.kind == Kind::File => n,
        Ok(_) => return plain(StatusCode::BAD_REQUEST, origin, "is a directory"),
        Err(_) => return plain(StatusCode::NOT_FOUND, origin, "not found"),
    };
    let range = req.headers().get(header::RANGE).and_then(|h| h.to_str().ok());
    let mime = mime_for(&n.name);
    // `?thumb=<px>`: a grid tile's picture at the size it is shown (thumbs.rs), else the original
    // `?fit=<px>`: a picture shown whole (quick look), sized to the window
    if let Some(px) = crate::thumbs::requested(req.uri().query()).or_else(|| crate::thumbs::requested_fit(req.uri().query())).filter(|_| mime.starts_with("image/") && range.is_none())
        && let Some((body, thumb_mime)) = crate::thumbs::thumbnail(&vid, &v, &n, px)
    {
        return base(StatusCode::OK, origin)
            .header(header::CONTENT_TYPE, thumb_mime)
            .header(header::CONTENT_LENGTH, body.len().to_string())
            .header(header::CACHE_CONTROL, "no-store")
            .body(body)
            .unwrap();
    }
    let (status, start, end) = match range.map(|r| parse_range(r, n.size)) {
        None | Some(Ok(None)) => {
            let cap = if mime.starts_with("video/") || mime.starts_with("audio/") { MAX_RANGE } else { MAX_BODY };
            if n.size <= cap {
                (StatusCode::OK, 0, n.size.saturating_sub(1))
            } else {
                (StatusCode::PARTIAL_CONTENT, 0, cap - 1)
            }
        }
        Some(Ok(Some((s, e)))) => (StatusCode::PARTIAL_CONTENT, s, e),
        Some(Err(())) => return unsatisfiable(n.size, origin),
    };
    let len = if n.size == 0 { 0 } else { end - start + 1 };
    let mut body = vec![0u8; len as usize];
    let mut got = 0usize;
    while got < body.len() {
        match v.read(node, start + got as u64, &mut body[got..]) {
            Ok(0) => break,
            Ok(k) => got += k,
            Err(_) => return plain(StatusCode::INTERNAL_SERVER_ERROR, origin, "read failed"),
        }
    }
    body.truncate(got);
    if status == StatusCode::PARTIAL_CONTENT && got == 0 {
        return unsatisfiable(n.size, origin); // shrank since getattr
    }
    let mut b = base(status, origin)
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, body.len().to_string())
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "Content-Range, Content-Length, Accept-Ranges");
    if status == StatusCode::PARTIAL_CONTENT {
        b = b.header(header::CONTENT_RANGE, format!("bytes {start}-{}/{}", start + got as u64 - 1, n.size));
    }
    b.body(body).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        let u: tauri::http::Uri = "nvault://abc123/42".parse().unwrap();
        assert_eq!(parse_target(&u), Some(("abc123".into(), 42)));
        let u: tauri::http::Uri = "http://nvault.localhost/abc123/7".parse().unwrap();
        assert_eq!(parse_target(&u), Some(("abc123".into(), 7)));
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-9", 100), Ok(Some((0, 9))));
        assert_eq!(parse_range("bytes=90-", 100), Ok(Some((90, 99))));
        assert_eq!(parse_range("bytes=-10", 100), Ok(Some((90, 99))));
        assert_eq!(parse_range("bytes=95-200", 100), Ok(Some((95, 99))));
        assert_eq!(parse_range("bytes=100-", 100), Err(()));
        assert_eq!(parse_range("bytes=0-", 0), Err(()));
        let huge = 1u64 << 40;
        assert_eq!(parse_range("bytes=0-", huge), Ok(Some((0, MAX_RANGE - 1))));
        assert_eq!(parse_range(&format!("bytes=5-{}", huge - 1), huge), Ok(Some((5, 5 + MAX_BODY - 1))));
        assert_eq!(parse_range(&format!("bytes=-{huge}"), huge), Ok(Some((huge - MAX_BODY, huge - 1))));
    }
}
