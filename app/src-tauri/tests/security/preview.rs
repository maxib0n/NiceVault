//! `nvault://` preview protocol (src/preview.rs).

use super::open_vault;
use nicevault_app::preview::{self, MAX_BODY};
use nv_fs::{Fs, Kind, ROOT};
use tauri::http::{Request, Response, StatusCode, header};

fn get(st: &nicevault_app::AppState, uri: &str, range: Option<&str>) -> Response<Vec<u8>> {
    let mut b = Request::builder().uri(uri);
    if let Some(r) = range {
        b = b.header(header::RANGE, r);
    }
    preview::handle(st, &b.body(Vec::new()).unwrap())
}

fn file(st: &nicevault_app::AppState, vault: &str, name: &str, data: &[u8]) -> u64 {
    let v = st.vault(vault).unwrap();
    let n = v.create(ROOT, name, Kind::File).unwrap();
    v.write(n.id, 0, data).unwrap();
    n.id
}

/// Verified sound: malformed / traversal-looking URLs never reach a node; HTML is never served
/// as a page; nosniff + no-store on content responses.
#[test]
fn urls_are_strict_and_html_is_inert() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, id) = open_vault(tmp.path());
    let html = file(&st, &id, "page.html", b"<script>alert(1)</script>");

    let r = get(&st, &format!("nvault://{id}/{html}"), None);
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers()[header::CONTENT_TYPE], "text/plain; charset=utf-8");
    assert_eq!(r.headers()["x-content-type-options"], "nosniff");
    assert_eq!(r.headers()[header::CACHE_CONTROL], "no-store");

    for bad in [
        format!("nvault://{id}/..%2f{html}"),
        format!("nvault://{id}/-1"),
        format!("nvault://{id}/99999999999999999999999"),
        format!("nvault://{id}/{html}x"),
        format!("nvault://{id}/"),
        "nvault://nope/1".to_string(),
        format!("nvault://%2e%2e/{html}"),
    ] {
        // A URI the http crate cannot even parse never reaches the handler.
        let Ok(req) = Request::builder().uri(bad.as_str()).body(Vec::new()) else { continue };
        let s = preview::handle(&st, &req).status();
        assert!(s == StatusCode::BAD_REQUEST || s == StatusCode::NOT_FOUND, "{bad} -> {s}");
    }
    // the root is a directory: refused
    assert_eq!(get(&st, &format!("nvault://{id}/{ROOT}"), None).status(), StatusCode::BAD_REQUEST);
    // garbage ranges are unsatisfiable, never a panic
    for r in ["bytes=5-1", "bytes=-0", "bytes=18446744073709551615-", "bytes=x-y", "bytes=-"] {
        let s = get(&st, &format!("nvault://{id}/{html}"), Some(r)).status();
        assert_eq!(s, StatusCode::RANGE_NOT_SATISFIABLE, "{r}");
    }
    // a locked volume answers 404, not stale bytes
    st.vault_lock(&id, "user").unwrap();
    assert_eq!(get(&st, &format!("nvault://{id}/{html}"), None).status(), StatusCode::NOT_FOUND);
}

/// A suffix range (`bytes=-N`) is not capped like other explicit ranges (MAX_BODY): the
/// handler allocates `vec![0; N]` for the whole file. On a multi-GB file this is an OOM abort.
#[test]
fn suffix_range_is_capped() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, id) = open_vault(tmp.path());
    let v = st.vault(&id).unwrap();
    let n = v.create(ROOT, "big.bin", Kind::File).unwrap();
    let size = MAX_BODY + (8 << 20); // sparse: set_size only
    v.set_size(n.id, size).unwrap();
    drop(v);
    let r = get(&st, &format!("nvault://{id}/{}", n.id), Some(&format!("bytes=-{size}")));
    assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
    assert!(r.body().len() as u64 <= MAX_BODY, "suffix range returned {} bytes (> MAX_BODY {MAX_BODY})", r.body().len());
}

/// Active content types (SVG, XML) are served with no CSP / sandbox. The nvault: scheme
/// is a "local" origin for Tauri (IPC allowed), so a document loaded from it could call
/// pm_reveal & co. Today only <img>/<video>/fetch load these URLs; this is defence in depth.
#[test]
fn active_types_are_sandboxed() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, id) = open_vault(tmp.path());
    let svg = file(&st, &id, "x.svg", br#"<svg xmlns="http://www.w3.org/2000/svg"><script>1</script></svg>"#);
    let xml = file(&st, &id, "x.xml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><script>1</script></html>"#);
    for node in [svg, xml] {
        let r = get(&st, &format!("nvault://{id}/{node}"), None);
        let csp = r.headers().get("content-security-policy").and_then(|v| v.to_str().ok()).unwrap_or("");
        assert!(csp.contains("sandbox") && csp.contains("default-src 'none'"), "node {node}: CSP = {csp:?}, type = {:?}", r.headers()[header::CONTENT_TYPE]);
    }
}

/// Every preview response says `Access-Control-Allow-Origin: *`, so any origin
/// that ever runs in the webview can read decrypted bytes cross-origin.
#[test]
fn cors_is_not_wildcard() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, id) = open_vault(tmp.path());
    let f = file(&st, &id, "a.txt", b"secret");
    let r = get(&st, &format!("nvault://{id}/{f}"), None);
    assert_ne!(r.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).map(|v| v.as_bytes()), Some(&b"*"[..]));
}
