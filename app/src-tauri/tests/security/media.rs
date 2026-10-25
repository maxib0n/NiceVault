//! Tests of the loopback media stream (src/viewers.rs), "Apri con app esterna" and the
//! Markdown link opener. Each test failed before the fixes and passes now.

use super::open_vault;
use nicevault_app::AppState;
use nicevault_app::viewers::{self, MediaServer};
use nv_fs::{Fs, Kind, NodeId, ROOT};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A volume with `name` holding `size` bytes of a known pattern.
fn with_file(dir: &std::path::Path, name: &str, size: usize) -> (Arc<AppState>, String, NodeId) {
    let (st, id) = open_vault(dir);
    let v = st.vault(&id).unwrap();
    let n = v.create(ROOT, name, Kind::File).unwrap().id;
    let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    v.write(n, 0, &data).unwrap();
    v.flush().unwrap();
    (st, id, n)
}

/// (port, "/m/<ticket>") of a ticket URL.
fn split(url: &str) -> (u16, String) {
    let rest = url.strip_prefix("http://127.0.0.1:").unwrap();
    let (port, path) = rest.split_once('/').unwrap();
    (port.parse().unwrap(), format!("/{path}"))
}

fn request(port: u16, raw: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut all = vec![];
    let _ = s.read_to_end(&mut all);
    String::from_utf8_lossy(&all[..all.len().min(512)]).into_owned()
}

/// Releasing the ticket (closing the viewer) stops a stream that is already flowing.
#[test]
#[cfg(target_os = "linux")]
fn media_stream_stops_after_release() {
    let tmp = tempfile::tempdir().unwrap();
    let size = 48 << 20;
    let (st, id, n) = with_file(tmp.path(), "film.webm", size);
    let ms = Arc::new(MediaServer::default());
    let url = ms.issue(&st, &id, n).unwrap();
    let (port, path) = split(&url);
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").unwrap();
    let mut buf = vec![0u8; 1 << 20];
    let mut got = 0usize;
    while got < (2 << 20) {
        got += s.read(&mut buf).unwrap();
    }
    ms.release(&url);
    loop {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(k) => got += k,
        }
    }
    st.lock_all("user");
    assert!(got < size / 2, "the stream went on after release: {got} of {size} bytes");
}

/// A client that drips its request one byte at a time is cut off by a total deadline
/// (a per-read timeout let it hold a thread forever).
#[test]
#[cfg(target_os = "linux")]
fn media_header_deadline() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, id, n) = with_file(tmp.path(), "clip.webm", 1000);
    let ms = Arc::new(MediaServer::default());
    let (port, _) = split(&ms.issue(&st, &id, n).unwrap());
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut r = s.try_clone().unwrap();
    let t0 = Instant::now();
    let reader = std::thread::spawn(move || {
        let mut sink = vec![];
        let _ = r.read_to_end(&mut sink);
        t0.elapsed()
    });
    for b in b"GET /m/".iter().cycle() {
        if reader.is_finished() || t0.elapsed() > Duration::from_secs(14) || s.write_all(&[*b]).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    let closed_after = reader.join().unwrap();
    st.lock_all("user");
    assert!(closed_after < Duration::from_secs(8), "a dripping client held the connection for {closed_after:?}");
}

/// Idle connections are capped (each one used to get a thread of its own, unbounded).
#[test]
#[cfg(target_os = "linux")]
fn media_connection_cap() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, id, n) = with_file(tmp.path(), "clip.webm", 1000);
    let ms = Arc::new(MediaServer::default());
    let (port, _) = split(&ms.issue(&st, &id, n).unwrap());
    let conns: Vec<TcpStream> = (0..40).map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(800));
    let mut dropped = 0;
    for mut c in conns {
        c.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        if matches!(c.read(&mut [0u8; 1]), Ok(0)) {
            dropped += 1;
        }
    }
    st.lock_all("user");
    assert!(dropped >= 20, "only {dropped} of 40 idle connections were refused");
}

/// With no ticket left the port is closed (nothing listens while no media plays).
#[test]
#[cfg(target_os = "linux")]
fn media_listener_closes_without_tickets() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, id, n) = with_file(tmp.path(), "clip.webm", 1000);
    let ms = Arc::new(MediaServer::default());
    let url = ms.issue(&st, &id, n).unwrap();
    let (port, _) = split(&url);
    ms.release(&url);
    let t0 = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_ok() && t0.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(100));
    }
    let closed = TcpStream::connect(("127.0.0.1", port)).is_err();
    // and a new ticket opens a working listener again
    let url2 = ms.issue(&st, &id, n).unwrap();
    let (port2, path2) = split(&url2);
    let ok = request(port2, &format!("GET {path2} HTTP/1.1\r\nHost: 127.0.0.1:{port2}\r\n\r\n"));
    st.lock_all("user");
    assert!(closed, "port {port} still accepts connections with no ticket");
    assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
}

/// A request a web page made (it carries an Origin, or a Referer that is not the app)
/// is refused before the ticket is even looked up. WebKitGTK's own media requests send
/// neither (checked in the real app: e2e/real/media-disk.mjs).
#[test]
#[cfg(target_os = "linux")]
fn media_refuses_foreign_origin() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, id, n) = with_file(tmp.path(), "clip.webm", 1000);
    let ms = Arc::new(MediaServer::default());
    let (port, path) = split(&ms.issue(&st, &id, n).unwrap());
    let host = format!("Host: 127.0.0.1:{port}\r\n");
    let origin = request(port, &format!("GET {path} HTTP/1.1\r\n{host}Origin: https://evil.example\r\n\r\n"));
    let null_origin = request(port, &format!("GET {path} HTTP/1.1\r\n{host}Origin: null\r\n\r\n"));
    let referer = request(port, &format!("GET {path} HTTP/1.1\r\n{host}Referer: https://evil.example/page\r\n\r\n"));
    let probe = request(port, &format!("GET /m/{} HTTP/1.1\r\n{host}Origin: https://evil.example\r\n\r\n", "0".repeat(64)));
    let plain = request(port, &format!("GET {path} HTTP/1.1\r\n{host}\r\n"));
    st.lock_all("user");
    for (what, r) in [("origin", &origin), ("null origin", &null_origin), ("referer", &referer), ("probe", &probe)] {
        assert!(r.starts_with("HTTP/1.1 403"), "{what}: {r}");
    }
    assert!(plain.starts_with("HTTP/1.1 200"), "{plain}");
}

/// A ticket only for audio / video (the stream is the media player's, nothing else).
#[test]
fn media_url_only_for_media() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, id, n) = with_file(tmp.path(), "tax return.pdf", 1000);
    let ms = Arc::new(MediaServer::default());
    let e = ms.issue(&st, &id, n).unwrap_err();
    st.lock_all("user");
    assert_eq!(e.code, "not_media");
}

/// Programs, scripts and launchers that Linux (Wine, Java, .desktop), macOS
/// (.command, .terminal, .webloc, .app) or any OS would run are refused on every OS, by
/// name and by their first bytes; for "Apri con app esterna" (mounted) and the temp copy.
#[test]
fn open_refuses_unix_runners() {
    use nicevault_app::state::runs_code;
    for n in [
        "Fattura.pdf.exe", "setup.msi", "a.lnk", "tool.jar", "app.desktop", "run.sh", "x.AppImage", "p.flatpakref", "go.command", "t.terminal",
        "site.webloc", "s.inetloc", "f.fileloc", "Installer.pkg", "Some.app", "script.py", "a.scpt", "x.run", "y.deb", "z.rpm", "w.workflow",
    ] {
        assert!(runs_code(n), "{n:?}");
    }
    for n in ["Fattura.pdf", "notes.txt", "photo.jpeg", "clip.webm", "archive.zip", "sheet.xlsx", "desktop", "readme"] {
        assert!(!runs_code(n), "{n:?}");
    }
    let tmp = tempfile::tempdir().unwrap();
    let (st, id) = open_vault(tmp.path());
    let v = st.vault(&id).unwrap();
    let mut nodes = vec![];
    for (name, body) in [("Avvia.desktop", &b"[Desktop Entry]\nExec=sh -c id\n"[..]), ("vacanze", b"\x7fELF\x02\x01\x01"), ("leggimi", b"#!/bin/sh\nid\n"), ("foto.jpg", b"MZ\x90\x00")] {
        let n = v.create(ROOT, name, Kind::File).unwrap().id;
        v.write(n, 0, body).unwrap();
        nodes.push((name, n));
    }
    drop(v);
    for (name, n) in nodes {
        // refused before the mount is even looked at, and before any temp copy is made
        assert_eq!(viewers::mounted_path(&st, &id, n).unwrap_err().code, "unsafe_type", "{name} (mounted)");
        assert_eq!(st.fs_open_external(&id, n).unwrap_err().code, "unsafe_type", "{name} (external)");
    }
    st.lock_all("user");
}

/// Links are re-serialised; mailto keeps the address and subject/body/cc/bcc only;
/// a backtick or `$` (shell metacharacters for a handler's command line) is refused.
#[test]
fn open_link_is_normalised() {
    let ok = |u: &str| viewers::checked_link(u).map(|s| s.to_string());
    assert_eq!(ok("mailto:a@b.it?subject=Ciao%20Mario&attach=/etc/passwd&body=x").as_deref(), Some("mailto:a@b.it?subject=Ciao%20Mario&body=x"));
    assert_eq!(ok("mailto:a@b.it?Attachment=~/.ssh/id_ed25519").as_deref(), Some("mailto:a@b.it"));
    assert_eq!(ok("https://example.org/a`id`"), None);
    assert_eq!(ok("https://example.org/$(id)"), None);
    assert_eq!(ok("https://user:pw@example.org/"), None);
    assert_eq!(ok("HTTPS://Example.ORG/a b").as_deref(), None);
    assert_eq!(ok("HTTPS://Example.ORG/x?y=1#f").as_deref(), Some("https://example.org/x?y=1#f"));
}
