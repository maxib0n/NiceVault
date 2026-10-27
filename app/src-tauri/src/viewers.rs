use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, lock_or_recover};
use nv_fs::{Fs, Kind, NodeId};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tauri::State;

const TICKET_TTL: Duration = Duration::from_secs(6 * 3600);
const MAX_TICKETS: usize = 8;
const CHUNK: usize = 1 << 20;
const MAX_CONNS: usize = 16;
const HEAD_DEADLINE: Duration = Duration::from_secs(5);
pub const MEDIA_RAM_MAX: u64 = 32 << 20;

fn ram_max() -> u64 {
    std::env::var("NICEVAULT_TEST_MEDIA_RAM_MAX").ok().and_then(|v| v.parse().ok()).unwrap_or(MEDIA_RAM_MAX)
}

fn is_media(mime: &str) -> bool {
    mime.starts_with("video/") || mime.starts_with("audio/")
}

struct Ticket {
    vault: String,
    node: NodeId,
    born: Instant,
}

#[derive(Default)]
struct Inner {
    port: Option<u16>,
    tickets: HashMap<String, Ticket>,
}

#[derive(Default)]
pub struct MediaServer {
    inner: Mutex<Inner>,
    conns: AtomicUsize,
}

fn token() -> String {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("OS random");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

struct Slot<'a>(&'a AtomicUsize);
impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Deadline {
    s: TcpStream,
    until: Instant,
}
impl Read for Deadline {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.s.set_read_timeout(Some(left))?;
        self.s.read(buf)
    }
}

#[cfg(unix)]
fn wait_readable(l: &TcpListener) -> bool {
    use std::os::fd::AsRawFd;
    let mut p = libc::pollfd { fd: l.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut p, 1, 1000) > 0 }
}
#[cfg(not(unix))]
fn wait_readable(_: &TcpListener) -> bool {
    true
}

pub fn tcp_owner(table: &str, local: SocketAddr, remote: SocketAddr) -> Option<u32> {
    let hex = |a: SocketAddr| match a {
        SocketAddr::V4(a) => Some(format!("{:08X}:{:04X}", u32::from_ne_bytes(a.ip().octets()), a.port())),
        SocketAddr::V6(_) => None,
    };
    let (l, r) = (hex(local)?, hex(remote)?);
    table.lines().skip(1).find_map(|line| {
        let f: Vec<&str> = line.split_whitespace().collect();
        (f.len() > 7 && f[1] == l && f[2] == r).then(|| f[7].parse().ok()).flatten()
    })
}

#[cfg(target_os = "linux")]
fn peer_is_us(conn: &TcpStream) -> bool {
    let (Ok(peer), Ok(me)) = (conn.peer_addr(), conn.local_addr()) else { return false };
    if !peer.ip().is_loopback() {
        return false;
    }
    let Ok(table) = std::fs::read_to_string("/proc/net/tcp") else { return false };
    tcp_owner(&table, peer, me) == Some(unsafe { libc::geteuid() })
}
#[cfg(target_os = "ios")]
fn peer_is_us(conn: &TcpStream) -> bool {
    conn.peer_addr().is_ok_and(|p| p.ip().is_loopback())
}
#[cfg(not(any(target_os = "linux", target_os = "ios")))]
fn peer_is_us(_: &TcpStream) -> bool {
    false
}

impl MediaServer {
    fn ensure_started(self: &Arc<Self>, g: &mut Inner, state: Weak<AppState>) -> std::io::Result<u16> {
        if let Some(p) = g.port {
            return Ok(p);
        }
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let me = Arc::downgrade(self);
        std::thread::Builder::new().name("nv-media".into()).spawn(move || {
            loop {
                let (Some(server), Some(st)) = (me.upgrade(), state.upgrade()) else { return };
                let ready = wait_readable(&listener);
                if server.sweep(&st, port) {
                    return;
                }
                if !ready {
                    continue;
                }
                let Ok((conn, _)) = listener.accept() else { continue };
                if server.conns.fetch_add(1, Ordering::AcqRel) >= MAX_CONNS {
                    server.conns.fetch_sub(1, Ordering::AcqRel);
                    continue;
                }
                let _ = std::thread::Builder::new().name("nv-media-conn".into()).spawn(move || {
                    let _slot = Slot(&server.conns);
                    let _ = server.serve(&st, conn, port);
                });
            }
        })?;
        g.port = Some(port);
        Ok(port)
    }

    fn sweep(&self, state: &AppState, port: u16) -> bool {
        let mut g = lock_or_recover(&self.inner);
        g.tickets.retain(|_, k| k.born.elapsed() < TICKET_TTL && state.is_open(&k.vault));
        if g.tickets.is_empty() && g.port == Some(port) {
            g.port = None;
            return true;
        }
        g.port != Some(port)
    }

    pub fn issue(self: &Arc<Self>, state: &Arc<AppState>, vault: &str, node: NodeId) -> ApiResult<String> {
        let n = state.vault(vault)?.getattr(node)?;
        if n.kind != Kind::File {
            return Err(ApiError::new("not_a_file", "Not a file."));
        }
        if !is_media(crate::preview::mime_for(&n.name)) {
            return Err(ApiError::new("not_media", "Only audio and video files are played."));
        }
        let mut g = lock_or_recover(&self.inner);
        let port = self.ensure_started(&mut g, Arc::downgrade(state)).map_err(|e| ApiError::new("media_server", format!("Could not start the media stream: {e}")))?;
        let t = token();
        g.tickets.retain(|_, k| k.born.elapsed() < TICKET_TTL);
        while g.tickets.len() >= MAX_TICKETS {
            let oldest = g.tickets.iter().min_by_key(|(_, k)| k.born).map(|(k, _)| k.clone()).expect("non-empty");
            g.tickets.remove(&oldest);
        }
        g.tickets.insert(t.clone(), Ticket { vault: vault.to_owned(), node, born: Instant::now() });
        Ok(format!("http://127.0.0.1:{port}/m/{t}"))
    }

    pub fn source(self: &Arc<Self>, state: &Arc<AppState>, vault: &str, node: NodeId) -> ApiResult<Option<String>> {
        if !media_stream_needed() {
            return Ok(None);
        }
        let n = state.vault(vault)?.getattr(node)?;
        if !cfg!(target_os = "ios") && n.kind == Kind::File && is_media(crate::preview::mime_for(&n.name)) && n.size <= ram_max() {
            return Ok(None);
        }
        self.issue(state, vault, node).map(Some)
    }

    pub fn release(&self, url: &str) {
        if let Some(t) = url.rsplit('/').next() {
            lock_or_recover(&self.inner).tickets.remove(t);
        }
    }

    fn lookup(&self, t: &str) -> Option<(String, NodeId)> {
        let g = lock_or_recover(&self.inner);
        g.tickets.get(t).filter(|k| k.born.elapsed() < TICKET_TTL).map(|k| (k.vault.clone(), k.node))
    }

    fn serve(&self, state: &AppState, conn: TcpStream, port: u16) -> std::io::Result<()> {
        if !peer_is_us(&conn) {
            return Ok(());
        }
        conn.set_write_timeout(Some(Duration::from_secs(30)))?;
        let mut reader = BufReader::new(Deadline { s: conn.try_clone()?, until: Instant::now() + HEAD_DEADLINE });
        let mut out = conn;
        let req = match read_head(&mut reader) {
            Some(r) => r,
            None => return respond(&mut out, 400, &[], b""),
        };
        let host_ok = req.header("host").is_some_and(|h| h == format!("127.0.0.1:{port}"));
        if !host_ok {
            return respond(&mut out, 421, &[], b"");
        }
        let cors = req.header("origin").filter(|o| is_app_origin_exact(o));
        if (req.header("origin").is_some() && cors.is_none()) || req.header("referer").is_some_and(|r| !is_app_origin(r)) {
            return respond(&mut out, 403, &[], b"");
        }
        let head_only = match req.method.as_str() {
            "GET" => false,
            "HEAD" => true,
            _ => return respond(&mut out, 405, &[("Allow", "GET, HEAD".into())], b""),
        };
        let Some(t) = req.path.strip_prefix("/m/").filter(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit())) else {
            return respond(&mut out, 404, &[], b"");
        };
        let Some((vid, node)) = self.lookup(t) else { return respond(&mut out, 404, &[], b"") };
        let Ok((size, mime)) = state.vault(&vid).and_then(|v| Ok(v.getattr(node)?)).map(|n| (n.size, crate::preview::mime_for(&n.name))) else {
            lock_or_recover(&self.inner).tickets.remove(t);
            return respond(&mut out, 404, &[], b"");
        };
        if !is_media(mime) {
            return respond(&mut out, 404, &[], b"");
        }
        let (status, start, end) = match req.header("range").map(|r| parse_range(r, size)) {
            None | Some(Ok(None)) => (200, 0, size.saturating_sub(1)),
            Some(Ok(Some((s, e)))) => (206, s, e),
            Some(Err(())) => return respond(&mut out, 416, &[("Content-Range", format!("bytes */{size}"))], b""),
        };
        let len = if size == 0 { 0 } else { end - start + 1 };
        let mut headers = vec![
            ("Content-Type", mime.to_owned()),
            ("Accept-Ranges", "bytes".into()),
            ("Content-Length", len.to_string()),
            ("Cache-Control", "no-store".into()),
            ("X-Content-Type-Options", "nosniff".into()),
            ("Content-Security-Policy", "default-src 'none'; sandbox".into()),
            ("Referrer-Policy", "no-referrer".into()),
        ];
        if let Some(o) = cors {
            headers.push(("Access-Control-Allow-Origin", o.to_owned()));
            headers.push(("Vary", "Origin".into()));
        }
        if status == 206 {
            headers.push(("Content-Range", format!("bytes {start}-{end}/{size}")));
        }
        write_head(&mut out, status, &headers)?;
        if head_only || len == 0 {
            return out.flush();
        }
        let mut buf = vec![0u8; CHUNK];
        let mut pos = start;
        while pos <= end {
            if self.lookup(t).is_none_or(|k| k != (vid.clone(), node)) {
                break;
            }
            let want = (end - pos + 1).min(CHUNK as u64) as usize;
            let Ok(v) = state.vault(&vid) else { break };
            let k = match v.read(node, pos, &mut buf[..want]) {
                Ok(0) | Err(_) => break,
                Ok(k) => k,
            };
            drop(v);
            if out.write_all(&buf[..k]).is_err() {
                break;
            }
            pos += k as u64;
        }
        buf.fill(0);
        out.flush()
    }
}

fn is_app_origin_exact(o: &str) -> bool {
    ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"].contains(&o)
}

fn is_app_origin(r: &str) -> bool {
    ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"].iter().any(|o| r.strip_prefix(o).is_some_and(|rest| rest.is_empty() || rest.starts_with('/')))
}

struct Head {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}
impl Head {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

fn read_head(r: &mut impl BufRead) -> Option<Head> {
    let mut line = String::new();
    let mut total = 0usize;
    let mut next = |line: &mut String| -> Option<()> {
        line.clear();
        let n = r.by_ref().take(8192).read_line(line).ok()?;
        total += n;
        (n > 0 && line.ends_with('\n') && total < 32 * 1024).then_some(())
    };
    next(&mut line)?;
    let mut parts = line.trim_end().splitn(3, ' ');
    let method = parts.next()?.to_owned();
    let path = parts.next()?.to_owned();
    if !parts.next()?.starts_with("HTTP/1.") {
        return None;
    }
    let mut headers = vec![];
    for _ in 0..100 {
        next(&mut line)?;
        let l = line.trim_end();
        if l.is_empty() {
            return Some(Head { method, path, headers });
        }
        let (k, v) = l.split_once(':')?;
        headers.push((k.trim().to_owned(), v.trim().to_owned()));
    }
    None
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        421 => "Misdirected Request",
        _ => "Error",
    }
}

fn write_head(out: &mut impl Write, status: u16, headers: &[(&str, String)]) -> std::io::Result<()> {
    let mut s = format!("HTTP/1.1 {status} {}\r\nConnection: close\r\n", reason(status));
    for (k, v) in headers {
        s.push_str(&format!("{k}: {v}\r\n"));
    }
    s.push_str("\r\n");
    out.write_all(s.as_bytes())
}

fn respond(out: &mut impl Write, status: u16, headers: &[(&str, String)], body: &[u8]) -> std::io::Result<()> {
    let mut h = headers.to_vec();
    h.push(("Content-Length", body.len().to_string()));
    write_head(out, status, &h)?;
    out.write_all(body)?;
    out.flush()
}

fn parse_range(h: &str, size: u64) -> Result<Option<(u64, u64)>, ()> {
    let num = |s: &str| if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) { s.parse::<u64>().map_err(|_| ()) } else { Err(()) };
    let Some(spec) = h.trim().strip_prefix("bytes=") else { return Ok(None) };
    let first = spec.split(',').next().unwrap_or("").trim();
    let (a, b) = first.split_once('-').ok_or(())?;
    if size == 0 {
        return Err(());
    }
    let (start, end) = if a.is_empty() {
        let n = num(b)?;
        if n == 0 {
            return Err(());
        }
        (size.saturating_sub(n), size - 1)
    } else {
        let s = num(a)?;
        let e = if b.is_empty() { size - 1 } else { num(b)?.min(size - 1) };
        (s, e)
    };
    if start >= size || start > end {
        return Err(());
    }
    Ok(Some((start, end)))
}

pub fn media_stream_needed() -> bool {
    (cfg!(target_os = "linux") || cfg!(target_os = "ios")) && std::env::var_os("NV_MEDIA_NO_STREAM").is_none()
}

pub fn checked_link(raw: &str) -> Option<String> {
    let u = raw.trim();
    if u.len() > 2048 || u.chars().any(|c| c.is_control() || c.is_whitespace() || matches!(c, '"' | '\\' | '<' | '>' | '`' | '$')) {
        return None;
    }
    let p = url::Url::parse(u).ok()?;
    match p.scheme() {
        "http" | "https" => {
            if p.host_str().is_none_or(str::is_empty) || !p.username().is_empty() || p.password().is_some() {
                return None;
            }
            Some(p.as_str().to_owned())
        }
        "mailto" => {
            let to = p.path();
            if !to.bytes().all(|b| b.is_ascii_alphanumeric() || b"@.-_+,!#'*^{}~".contains(&b)) {
                return None;
            }
            let mut out = format!("mailto:{to}");
            let mut sep = '?';
            for pair in p.query().unwrap_or("").split('&').filter(|x| !x.is_empty()) {
                let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                let k = k.to_ascii_lowercase();
                if !matches!(k.as_str(), "subject" | "body" | "cc" | "bcc") {
                    continue;
                }
                out.push(sep);
                out.push_str(&k);
                out.push('=');
                out.push_str(&pct_encode(&pct_decode(v)?));
                sep = '&';
            }
            if out.len() <= 4096 { Some(out) } else { None }
        }
        _ => None,
    }
}

fn pct_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    let s = String::from_utf8(out).ok()?;
    (!s.chars().any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')).then_some(s)
}

fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn mounted_path(state: &AppState, vault: &str, node: NodeId) -> ApiResult<std::path::PathBuf> {
    let v = state.vault(vault)?;
    let n = v.getattr(node)?;
    if n.kind != Kind::File {
        return Err(ApiError::new("not_a_file", "Not a file."));
    }
    crate::state::refuse_runnable(&v, &n)?;
    let info = state.vault_list_open().into_iter().find(|i| i.id == vault).ok_or_else(ApiError::locked)?;
    let mp = info.mountpoint.ok_or_else(|| ApiError::new("not_mounted", "The volume is not mounted as a drive."))?;
    let rel = crate::props::node_path(&v, node)?;
    let mut p = std::path::PathBuf::from(mp);
    for seg in rel.split('/').filter(|s| !s.is_empty()) {
        if seg == ".." || seg == "." {
            return Err(ApiError::new("bad_path", "Unexpected file name."));
        }
        p.push(seg);
    }
    Ok(p)
}

pub mod cmd {
    use super::*;
    type St<'a> = State<'a, Arc<AppState>>;
    type Ms<'a> = State<'a, Arc<MediaServer>>;

    #[tauri::command(rename_all = "snake_case")]
    pub async fn media_url(st: St<'_>, ms: Ms<'_>, vault: String, node: NodeId) -> ApiResult<Option<String>> {
        let (s, m) = (st.inner().clone(), ms.inner().clone());
        tauri::async_runtime::spawn_blocking(move || m.source(&s, &vault, node))
            .await
            .map_err(|e| ApiError::new("error", format!("Internal error: {e}")))?
    }

    #[tauri::command(rename_all = "snake_case")]
    pub fn media_release(ms: Ms<'_>, url: String) {
        ms.release(&url);
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn fs_open_mounted(st: St<'_>, vault: String, node: NodeId) -> ApiResult<()> {
        let s = st.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let p = mounted_path(&s, &vault, node)?;
            open::that_detached(&p).map_err(|e| ApiError::new("open_failed", format!("Could not open the file: {e}")))
        })
        .await
        .map_err(|e| ApiError::new("error", format!("Internal error: {e}")))?
    }

    #[tauri::command(rename_all = "snake_case")]
    pub fn open_link(url: String) -> ApiResult<()> {
        let u = checked_link(&url).ok_or_else(|| ApiError::new("bad_link", "Only web and e-mail links can be opened."))?;
        open::that_detached(&u).map_err(|e| ApiError::new("open_failed", format!("Could not open the link: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-9", 100), Ok(Some((0, 9))));
        assert_eq!(parse_range("bytes=90-", 100), Ok(Some((90, 99))));
        assert_eq!(parse_range("bytes=-10", 100), Ok(Some((90, 99))));
        assert_eq!(parse_range("bytes=95-200", 100), Ok(Some((95, 99))));
        assert_eq!(parse_range("bytes=100-", 100), Err(()));
        assert_eq!(parse_range("bytes=0-", 0), Err(()));
        assert_eq!(parse_range("items=0-", 10), Ok(None));
        assert_eq!(parse_range("bytes=+5-", 100), Err(()));
        assert_eq!(parse_range("bytes=0-+9", 100), Err(()));
        assert_eq!(parse_range("bytes=-+9", 100), Err(()));
        assert_eq!(parse_range("bytes=0-", 10 << 30), Ok(Some((0, (10 << 30) - 1))));
    }

    #[test]
    fn links() {
        assert_eq!(checked_link("https://example.org/a?b=1").as_deref(), Some("https://example.org/a?b=1"));
        assert_eq!(checked_link("mailto:a@b.it").as_deref(), Some("mailto:a@b.it"));
        assert_eq!(checked_link("mailto:a@b.it,c@d.it?CC=e@f.it&subject=Ciao+a%20te").as_deref(), Some("mailto:a@b.it,c@d.it?cc=e%40f.it&subject=Ciao%2Ba%20te"));
        assert_eq!(checked_link("mailto:a@b.it%3Fattach%3D%2Fetc%2Fpasswd"), None);
        assert_eq!(checked_link("mailto:a@b.it?body=%0D%0AX-Bad:%201%01"), None);
        assert_eq!(checked_link("https://xn--80ak6aa92e.com/").as_deref(), Some("https://xn--80ak6aa92e.com/"));
        assert_eq!(checked_link("javascript:alert(1)"), None);
        assert_eq!(checked_link("file:///etc/passwd"), None);
        assert_eq!(checked_link("https://a b"), None);
        assert_eq!(checked_link("https://a\n.b"), None);
    }

    #[test]
    fn heads() {
        let raw = b"GET /m/abc HTTP/1.1\r\nHost: 127.0.0.1:5\r\nRange: bytes=1-2\r\n\r\n";
        let h = read_head(&mut &raw[..]).unwrap();
        assert_eq!((h.method.as_str(), h.path.as_str(), h.header("range"), h.header("HOST")), ("GET", "/m/abc", Some("bytes=1-2"), Some("127.0.0.1:5")));
        assert!(read_head(&mut &b"garbage"[..]).is_none());
        let long = format!("GET /m/abc HTTP/1.1\r\nX-Pad: {}Range: bytes=0-1\r\n\r\n", "a".repeat(8190));
        assert!(read_head(&mut long.as_bytes()).is_none());
    }

    #[test]
    fn tcp_table_owner() {
        let t = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
                 0: 0100007F:1388 0100007F:9C40 01 00000000:00000000 00:00000000 00000000  1000        0 1 1 0 20 4 30 10 -1\n\
                 1: 0100007F:9C40 0100007F:1388 01 00000000:00000000 00:00000000 00000000  1001        0 2 1 0 20 4 30 10 -1\n";
        let client: SocketAddr = "127.0.0.1:40000".parse().unwrap();
        let server: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        if cfg!(target_endian = "little") {
            assert_eq!(tcp_owner(t, client, server), Some(1001));
            assert_eq!(tcp_owner(t, server, client), Some(1000));
        }
        assert_eq!(tcp_owner(t, "127.0.0.1:40001".parse().unwrap(), server), None);
    }

    fn get(port: u16, path: &str, extra: &str) -> (u16, Vec<u8>, String) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}\r\n").unwrap();
        let mut all = vec![];
        s.read_to_end(&mut all).unwrap();
        let split = all.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&all[..split]).into_owned();
        let status = head[9..12].parse().unwrap();
        (status, all[split + 4..].to_vec(), head)
    }

    #[test]
    fn streams_ranges_from_the_store_and_guards_the_door() {
        let tmp = tempfile::tempdir().unwrap();
        let store = crate::store::Store::new(crate::store::Locations { exe_dir: None, removable: Some(vec![]), config_dir: None });
        let state = AppState::new(store, Box::new(crate::state::NoEvents));
        let path = tmp.path().join("m.nvault");
        let info = state.vault_create(path.to_str().unwrap(), secrecy::SecretString::from("pw-for-the-test-123".to_owned()), Some("fast")).unwrap();
        let v = state.vault(&info.id).unwrap();
        let data: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        let id = v.create(nv_fs::ROOT, "clip.webm", Kind::File).unwrap().id;
        v.write(id, 0, &data).unwrap();
        v.flush().unwrap();
        drop(v);
        let ms = Arc::new(MediaServer::default());
        let url = ms.issue(&state, &info.id, id).unwrap();
        let port: u16 = url.split(':').nth(2).unwrap().split('/').next().unwrap().parse().unwrap();
        let path = &url[url.find("/m/").unwrap()..];

        let (st, body, head) = get(port, path, "Range: bytes=2000000-\r\n");
        assert_eq!(st, 206);
        assert!(head.contains("Content-Range: bytes 2000000-2999999/3000000"), "{head}");
        assert!(head.contains("Content-Type: video/webm"));
        assert_eq!(body, &data[2_000_000..]);
        let (st, body, _) = get(port, path, "");
        assert_eq!((st, body.len()), (200, data.len()));
        assert_eq!(get(port, path, "Range: bytes=5000000-\r\n").0, 416);
        for o in ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"] {
            let (st, _, head) = get(port, path, &format!("Origin: {o}\r\nRange: bytes=0-9\r\n"));
            assert_eq!(st, 206);
            assert!(head.contains(&format!("Access-Control-Allow-Origin: {o}\r\n")) || head.ends_with(&format!("Access-Control-Allow-Origin: {o}")), "{head}");
            assert!(head.contains("Vary: Origin"), "{head}");
        }
        for o in ["http://evil.example", "http://tauri.localhost.evil.example", "null", "http://localhost:5173", "tauri://localhost/"] {
            let (st, _, head) = get(port, path, &format!("Origin: {o}\r\n"));
            assert_eq!(st, 403, "{o}");
            assert!(!head.contains("Access-Control-Allow-Origin"));
        }
        assert!(!get(port, path, "").2.contains("Access-Control-Allow-Origin"));
        assert_eq!(get(port, &format!("/m/{}", "0".repeat(64)), "").0, 404);
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: evil.example:{port}\r\n\r\n").unwrap();
        let mut all = String::new();
        s.read_to_string(&mut all).unwrap();
        assert!(all.starts_with("HTTP/1.1 421"), "{all}");
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").unwrap();
        let mut all = String::new();
        s.read_to_string(&mut all).unwrap();
        assert!(all.starts_with("HTTP/1.1 405"), "{all}");
        let url2 = ms.issue(&state, &info.id, id).unwrap();
        ms.release(&url);
        assert_eq!(get(port, path, "").0, 404);
        state.vault_lock(&info.id, "user").unwrap();
        let dead = (|| {
            let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
            write!(s, "GET {} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n", &url2[url2.find("/m/").unwrap()..]).ok()?;
            let mut all = String::new();
            s.read_to_string(&mut all).ok()?;
            Some(all)
        })();
        assert!(dead.as_deref().is_none_or(|r| r.starts_with("HTTP/1.1 404")), "{dead:?}");
    }
}
