use crate::error::ApiResult;
use crate::state::{AppState, lock_or_recover};
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use serde_json::json;
use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::http::{Request, Response, StatusCode, header};
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

pub const ICON_PX: u32 = 64;
pub const REFRESH_MS: i64 = 7 * 86_400_000;
pub const MAX_PAGE: u64 = 256 * 1024;
pub const MAX_ICON_FILE: u64 = 512 * 1024;
const WORKERS: usize = 4;
const MAX_REDIRECTS: usize = 4;
const TRY_LINKS: usize = 3;

static RUNNING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub struct Fetcher {
    agent: ureq::Agent,
    origin: Option<String>,
}

impl Default for Fetcher {
    fn default() -> Self {
        Fetcher::new()
    }
}

impl Fetcher {
    pub fn new() -> Fetcher {
        Fetcher { agent: agent(true, PublicOnly { inner: DefaultResolver::default(), loopback: false }), origin: None }
    }

    pub fn for_test(origin: &str) -> Fetcher {
        Fetcher { agent: agent(false, PublicOnly { inner: DefaultResolver::default(), loopback: true }), origin: Some(origin.trim_end_matches('/').to_owned()) }
    }

    fn allowed(&self, url: &str) -> bool {
        if self.origin.as_deref().is_some_and(|o| url.starts_with(o)) {
            return true;
        }
        url.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("https://")) && nv_core::passwords::icon_host(&[url.to_owned()]).is_some()
    }

    fn get(&self, url: &str, max: u64) -> Option<(String, Option<String>, Vec<u8>)> {
        let mut url = url.to_owned();
        let mut hops = 0;
        let mut resp = loop {
            if !self.allowed(&url) {
                return None;
            }
            let r = self.agent.get(&url).header("Accept", "text/html,image/*;q=0.9,*/*;q=0.5").call().ok()?;
            if !r.status().is_redirection() {
                break r;
            }
            hops += 1;
            let to = r.headers().get(header::LOCATION).and_then(|v| v.to_str().ok())?;
            if hops > MAX_REDIRECTS {
                return None;
            }
            url = join(&url, to)?;
        };
        if !resp.status().is_success() {
            return None;
        }
        let final_url = url;
        let ct = resp.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(|s| s.to_ascii_lowercase());
        if resp.body().content_length().is_some_and(|n| n > max) && !ct.as_deref().is_some_and(|c| c.contains("html")) {
            return None;
        }
        let mut out = Vec::new();
        resp.body_mut().as_reader().take(max + 1).read_to_end(&mut out).ok()?;
        if out.len() as u64 > max {
            if ct.as_deref().is_some_and(|c| c.contains("html")) {
                out.truncate(max as usize);
            } else {
                return None;
            }
        }
        Some((final_url, ct, out))
    }

    pub fn icon(&self, host: &str) -> Option<Vec<u8>> {
        let base = match &self.origin {
            Some(o) => format!("{o}/"),
            None => format!("https://{host}/"),
        };
        let mut tried: Vec<String> = Vec::new();
        if let Some((page_url, ct, html)) = self.get(&base, MAX_PAGE)
            && ct.as_deref().is_none_or(|c| c.contains("html"))
        {
            for href in best_links(&String::from_utf8_lossy(&html), &page_url).into_iter().take(TRY_LINKS) {
                if let Some(png) = self.load(&href) {
                    return Some(png);
                }
                tried.push(href);
            }
        }
        let ico = join(&base, "/favicon.ico")?;
        if tried.contains(&ico) {
            return None;
        }
        self.load(&ico)
    }

    fn load(&self, url: &str) -> Option<Vec<u8>> {
        if let Some(data) = url.strip_prefix("data:") {
            return to_png(&data_url(data)?);
        }
        let (_, _, bytes) = self.get(url, MAX_ICON_FILE)?;
        to_png(&bytes)
    }
}

fn agent(https_only: bool, resolver: impl Resolver) -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build();
    let config = ureq::Agent::config_builder()
        .tls_config(tls)
        .https_only(https_only)
        .proxy(None)
        .max_redirects(0)
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15")
        .timeout_global(Some(Duration::from_secs(12)))
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_recv_response(Some(Duration::from_secs(6)))
        .timeout_recv_body(Some(Duration::from_secs(6)))
        .http_status_as_error(false)
        .build();
    ureq::Agent::with_parts(config, DefaultConnector::default(), resolver)
}

#[derive(Debug)]
pub struct PublicOnly<R: Resolver> {
    pub inner: R,
    pub loopback: bool,
}

impl<R: Resolver> Resolver for PublicOnly<R> {
    fn resolve(&self, uri: &ureq::http::Uri, config: &ureq::config::Config, timeout: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let all = self.inner.resolve(uri, config, timeout)?;
        let mut out = self.inner.empty();
        for a in all.iter().filter(|a| public_ip(a.ip()) || (self.loopback && a.ip().is_loopback())) {
            out.push(*a);
        }
        if out.is_empty() { Err(ureq::Error::HostNotFound) } else { Ok(out) }
    }
}

pub fn public_ip(ip: std::net::IpAddr) -> bool {
    use std::net::{IpAddr, Ipv4Addr};
    fn v4(a: Ipv4Addr) -> bool {
        let [b0, b1, b2, _] = a.octets();
        !(b0 == 0
            || b0 == 10
            || b0 == 127
            || (b0 == 100 && (64..128).contains(&b1))
            || (b0 == 169 && b1 == 254)
            || (b0 == 172 && (16..32).contains(&b1))
            || (b0 == 192 && b1 == 168)
            || (b0 == 192 && b1 == 0 && (b2 == 0 || b2 == 2))
            || (b0 == 192 && b1 == 88 && b2 == 99)
            || (b0 == 198 && (b1 == 18 || b1 == 19))
            || (b0 == 198 && b1 == 51 && b2 == 100)
            || (b0 == 203 && b1 == 0 && b2 == 113)
            || b0 >= 224)
    }
    match ip {
        IpAddr::V4(a) => v4(a),
        IpAddr::V6(a) => {
            let s = a.segments();
            let embedded = |hi: usize| Ipv4Addr::new((s[hi] >> 8) as u8, s[hi] as u8, (s[hi + 1] >> 8) as u8, s[hi + 1] as u8);
            if s[..5] == [0; 5] && (s[5] == 0xffff || s[5] == 0) {
                return v4(embedded(6));
            }
            if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                return v4(embedded(6));
            }
            if s[0] == 0x2002 {
                return v4(embedded(1));
            }
            !((s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] & 0xffc0) == 0xfec0
                || (s[0] & 0xff00) == 0xff00
                || s[..4] == [0x100, 0, 0, 0]
                || (s[0] == 0x2001 && s[1] < 0x200)
                || (s[0] == 0x2001 && s[1] == 0xdb8))
        }
    }
}

pub fn best_links(html: &str, page_url: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let end = lower.find("</head").unwrap_or(lower.len());
    let mut cands: Vec<(u32, u32, String)> = Vec::new();
    let mut base = page_url.to_owned();
    let mut at = 0;
    while let Some(i) = lower[at..end].find('<').map(|i| i + at) {
        let rest = &lower[i + 1..end];
        let is_link = rest.starts_with("link") && rest[4..].starts_with(|c: char| c.is_ascii_whitespace() || c == '/');
        let is_base = rest.starts_with("base") && rest[4..].starts_with(|c: char| c.is_ascii_whitespace());
        let Some(close) = lower[i..end].find('>').map(|c| c + i) else { break };
        if is_link || is_base {
            let attrs = attributes(&html[i + 5..close]);
            let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
            if is_base {
                if let Some(b) = get("href").and_then(|h| join(page_url, h)) {
                    base = b;
                }
            } else if let (Some(rel), Some(href)) = (get("rel"), get("href")) {
                let rel = rel.to_ascii_lowercase();
                let rels: Vec<&str> = rel.split_ascii_whitespace().collect();
                let touch = rels.iter().any(|r| r.starts_with("apple-touch-icon"));
                if (rels.contains(&"icon") || touch)
                    && let Some(url) = if href.trim_start().starts_with("data:") { Some(href.trim().to_owned()) } else { join(&base, href) }
                {
                    let ty = get("type").unwrap_or("").to_ascii_lowercase();
                    let path = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
                    let kind = if ty.contains("png") || path.ends_with(".png") || url.starts_with("data:image/png") || touch {
                        0
                    } else if ty.contains("svg") || path.ends_with(".svg") || url.starts_with("data:image/svg") {
                        2
                    } else {
                        1
                    };
                    let size = get("sizes")
                        .and_then(|s| s.split_ascii_whitespace().filter_map(|x| x.to_ascii_lowercase().split_once('x').and_then(|(w, _)| w.parse::<u32>().ok())).max())
                        .or(if touch { Some(180) } else { None });
                    let miss = match size {
                        Some(s) if s >= ICON_PX => s - ICON_PX,
                        Some(s) => (ICON_PX - s) * 4,
                        None if kind == 2 => 0,
                        None => 40,
                    };
                    if !cands.iter().any(|c| c.2 == url) {
                        cands.push((kind, miss, url));
                    }
                }
            }
        }
        at = close + 1;
    }
    cands.sort_by_key(|c| (c.0, c.1));
    cands.into_iter().map(|c| c.2).collect()
}

fn attributes(s: &str) -> Vec<(String, String)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let n0 = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        if n0 == i {
            i += 1;
            continue;
        }
        let name = s[n0..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let v0 = i + 1;
                i = v0;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = s[v0..i.min(b.len())].to_owned();
                i += 1;
            } else {
                let v0 = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = s[v0..i].to_owned();
            }
        }
        let value = value.replace("&amp;", "&").replace("&#38;", "&").replace("&quot;", "\"").replace("&#x2F;", "/").replace("&#47;", "/");
        out.push((name, value));
    }
    out
}

pub fn join(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    let (scheme, rest) = base.split_once("://")?;
    let auth_end = rest.find('/').unwrap_or(rest.len());
    let origin = format!("{scheme}://{}", &rest[..auth_end]);
    let lower = href.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return Some(href.to_owned());
    }
    if href.contains("://") || (href.contains(':') && !href.starts_with('/') && href.split(['/', '?', '#']).next().is_some_and(|f| f.contains(':'))) {
        return None;
    }
    if let Some(r) = href.strip_prefix("//") {
        return Some(format!("{scheme}://{r}"));
    }
    if href.starts_with('/') {
        return Some(format!("{origin}{href}"));
    }
    let path = &rest[auth_end..];
    let path = path.split(['?', '#']).next().unwrap_or("");
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    Some(format!("{origin}{dir}/{href}"))
}

fn data_url(s: &str) -> Option<Vec<u8>> {
    let (meta, data) = s.split_once(',')?;
    if data.len() as u64 > MAX_ICON_FILE * 2 {
        return None;
    }
    if meta.to_ascii_lowercase().ends_with(";base64") {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(data.trim().replace(['\n', '\r', ' '], "")).ok()
    } else {
        let b = data.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%'
                && i + 2 < b.len()
                && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
            {
                out.push(v);
                i += 3;
                continue;
            }
            out.push(b[i]);
            i += 1;
        }
        Some(out)
    }
}

fn looks_svg(b: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&b[..b.len().min(1024)]).to_ascii_lowercase();
    let t = head.trim_start_matches('\u{feff}').trim_start();
    (t.starts_with("<?xml") || t.starts_with("<svg") || t.starts_with("<!--") || t.starts_with("<!doctype svg")) && head.contains("<svg")
}

pub fn to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = if looks_svg(bytes) { svg_image(bytes)? } else { raster_image(bytes)? };
    if img.width() < 8 || img.height() < 8 {
        return None;
    }
    let img = if img.width() > ICON_PX || img.height() > ICON_PX { img.resize(ICON_PX, ICON_PX, image::imageops::FilterType::Lanczos3) } else { img };
    let mut out = Cursor::new(Vec::new());
    img.to_rgba8().write_to(&mut out, ImageFormat::Png).ok()?;
    let out = out.into_inner();
    (out.len() <= nv_core::passwords::MAX_ICON).then_some(out)
}

fn raster_image(bytes: &[u8]) -> Option<DynamicImage> {
    let mut r = ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?;
    if !matches!(r.format(), Some(ImageFormat::Png | ImageFormat::Ico | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP)) {
        return None;
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(1024);
    limits.max_image_height = Some(1024);
    limits.max_alloc = Some(32 << 20);
    r.limits(limits);
    r.decode().ok()
}

fn svg_image(bytes: &[u8]) -> Option<DynamicImage> {
    use resvg::{tiny_skia, usvg};
    if bytes.len() > 256 * 1024 {
        return None;
    }
    let opt = usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver { resolve_data: Box::new(|_, _, _| None), resolve_string: Box::new(|_, _| None) },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(bytes, &opt).ok()?;
    let size = tree.size();
    let (w, h) = (size.width(), size.height());
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let scale = ICON_PX as f32 / w.max(h);
    let (pw, ph) = (((w * scale).round() as u32).max(1), ((h * scale).round() as u32).max(1));
    let mut pixmap = tiny_skia::Pixmap::new(pw, ph)?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let mut rgba = Vec::with_capacity((pw * ph * 4) as usize);
    for p in pixmap.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    image::RgbaImage::from_raw(pw, ph, rgba).map(DynamicImage::ImageRgba8)
}

pub fn due(hosts: Vec<(String, Option<i64>)>, now: i64) -> Vec<String> {
    hosts.into_iter().filter(|(_, f)| f.is_none_or(|f| now - f >= REFRESH_MS)).map(|(h, _)| h).collect()
}

impl AppState {
    pub fn pm_icons_refresh(&self, vault: &str) -> ApiResult<usize> {
        self.pm_icons_refresh_with(vault, Arc::new(Fetcher::new()))
    }

    pub fn pm_icons_refresh_with(&self, vault: &str, fetcher: Arc<Fetcher>) -> ApiResult<usize> {
        let v = self.vault(vault)?;
        if !lock_or_recover(&self.store).site_icons() {
            return Ok(0);
        }
        let todo = due(v.pm_icon_hosts()?, nv_core::passwords::now_ms());
        if todo.is_empty() {
            return Ok(0);
        }
        {
            let mut r = lock_or_recover(&RUNNING);
            if !r.get_or_insert_with(HashSet::new).insert(vault.to_owned()) {
                return Ok(0);
            }
        }
        let total = todo.len();
        let st = self.arc();
        let vault = vault.to_owned();
        std::thread::Builder::new()
            .name("nv-pm-icons".into())
            .spawn(move || {
                let queue = Mutex::new(todo.into_iter());
                let done = Mutex::new((0usize, Instant::now()));
                std::thread::scope(|s| {
                    for _ in 0..WORKERS.min(total) {
                        s.spawn(|| {
                            loop {
                                let Some(host) = lock_or_recover(&queue).next() else { break };
                                if !lock_or_recover(&st.store).site_icons() {
                                    break;
                                }
                                let png = fetcher.icon(&host);
                                if v.pm_icon_put(&host, png.as_deref(), nv_core::passwords::now_ms()).is_err() {
                                    break;
                                }
                                let mut d = lock_or_recover(&done);
                                d.0 += 1;
                                if png.is_some() && d.1.elapsed() >= Duration::from_millis(500) {
                                    d.1 = Instant::now();
                                    st.emit("pm-icons", json!({ "vault": vault, "done": d.0, "total": total }));
                                }
                            }
                        });
                    }
                });
                let n = lock_or_recover(&done).0;
                if let Some(r) = lock_or_recover(&RUNNING).as_mut() {
                    r.remove(&vault);
                }
                st.emit("pm-icons", json!({ "vault": vault, "done": n, "total": total }));
            })
            .expect("spawn icon thread");
        Ok(total)
    }
}

pub fn serve(state: &AppState, req: &Request<Vec<u8>>) -> Option<Response<Vec<u8>>> {
    let uri = req.uri();
    let host = uri.host().unwrap_or("");
    let segs: Vec<&str> = uri.path().split('/').filter(|s| !s.is_empty()).collect();
    let (vault, rest) = if host.is_empty() || host == "nvault.localhost" || host == "localhost" { (*segs.first()?, &segs[1..]) } else { (host, &segs[..]) };
    if rest.first() != Some(&"pm-icon") {
        return None;
    }
    let vault = crate::preview::percent_decode(vault);
    let not_found = || Response::builder().status(StatusCode::NOT_FOUND).header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*").body(Vec::new()).unwrap();
    let Some(icon_host) = rest.get(1) else { return Some(not_found()) };
    let png = state.vault(&vault).ok().and_then(|v| v.pm_icon_get(icon_host).ok());
    Some(match png {
        Some(png) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/png")
            .header(header::CONTENT_LENGTH, png.len().to_string())
            .header(header::CACHE_CONTROL, "private, max-age=31536000, immutable")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header("X-Content-Type-Options", "nosniff")
            .body(png)
            .unwrap(),
        None => not_found(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_ranked_and_resolved() {
        let html = r#"<html><HEAD><meta charset=utf-8>
          <link rel="stylesheet" href="/a.css">
          <link rel="mask-icon" href="/mask.svg">
          <LINK REL="shortcut icon" href="/favicon.ico">
          <link rel=icon type="image/svg+xml" href="icon.svg">
          <link rel="icon" sizes="16x16" href="/i16.png">
          <link rel="icon" sizes="32x32 192x192" href="//cdn.example.com/i192.png?v=1&amp;x=2">
          <link rel="apple-touch-icon" href="/touch.png">
          <link rel="icon" href="javascript:alert(1)">
          </head><body><link rel="icon" href="/late.png"></body>"#;
        let l = best_links(html, "https://site.example/app/home");
        assert_eq!(
            l,
            [
                "https://site.example/touch.png",
                "https://cdn.example.com/i192.png?v=1&x=2",
                "https://site.example/i16.png",
                "https://site.example/favicon.ico",
                "https://site.example/app/icon.svg",
            ]
        );
        assert_eq!(best_links(r#"<base href="https://b.example/x/"><link rel=icon href=i.png>"#, "https://a.example/")[0], "https://b.example/x/i.png");
        assert_eq!(join("https://a.example", "/f.ico").as_deref(), Some("https://a.example/f.ico"));
        assert_eq!(join("https://a.example/", "mailto:x@y"), None);
    }

    fn png_of(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x % 255) as u8, (y % 255) as u8, 90, 255]));
        let mut c = Cursor::new(Vec::new());
        img.write_to(&mut c, ImageFormat::Png).unwrap();
        c.into_inner()
    }

    #[test]
    fn images_are_normalised() {
        let p = to_png(&png_of(180, 90)).unwrap();
        let back = image::load_from_memory(&p).unwrap();
        assert_eq!((back.width(), back.height()), (64, 32));
        let small = image::load_from_memory(&to_png(&png_of(16, 16)).unwrap()).unwrap();
        assert_eq!(small.width(), 16, "small icons are not blown up here (the UI scales)");
        assert!(to_png(&png_of(4, 4)).is_none());
        assert!(to_png(b"<html>not an image</html>").is_none());
        let mut ico = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::load_from_memory(&png_of(48, 48)).unwrap().to_rgba8()).write_to(&mut ico, ImageFormat::Ico).unwrap();
        assert_eq!(image::load_from_memory(&to_png(&ico.into_inner()).unwrap()).unwrap().width(), 48);
    }

    #[test]
    fn svg_is_rasterised_without_scripts_or_external_images() {
        let svg = br##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="100">
            <script>alert(1)</script>
            <rect width="100" height="100" fill="#c00" onload="alert(2)"/>
            <image href="file:///etc/passwd" width="100" height="100"/>
            <image xlink:href="https://tracker.example/pixel.png" width="100" height="100"/>
            </svg>"##;
        let png = to_png(svg).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (64, 64));
        assert_eq!(img.get_pixel(32, 32).0, [204, 0, 0, 255], "only the shape is drawn");
        assert!(png.starts_with(b"\x89PNG") && !png.windows(6).any(|w| w == b"script"));
    }

    #[test]
    fn due_hosts() {
        let now = 10 * REFRESH_MS;
        let v = due(vec![("a".into(), None), ("b".into(), Some(now - 1000)), ("c".into(), Some(now - REFRESH_MS))], now);
        assert_eq!(v, ["a", "c"]);
    }

    #[test]
    fn only_public_addresses_are_fetched() {
        for a in [
            "0.0.0.0", "10.1.2.3", "127.0.0.1", "100.64.0.1", "100.127.255.254", "169.254.169.254", "172.16.0.1", "172.31.255.255",
            "192.168.1.1", "192.0.0.8", "192.0.2.1", "198.18.0.1", "198.51.100.7", "203.0.113.9", "224.0.0.1", "255.255.255.255",
            "::", "::1", "::ffff:127.0.0.1", "::ffff:192.168.0.1", "::ffff:169.254.169.254", "::127.0.0.1", "64:ff9b::a00:1",
            "2002:c0a8:101::1", "fc00::1", "fd12:3456::1", "fe80::1", "fec0::1", "ff02::1", "100::1", "2001::1", "2001:db8::1",
        ] {
            assert!(!public_ip(a.parse().unwrap()), "{a}");
        }
        for a in ["1.1.1.1", "8.8.8.8", "100.63.0.1", "100.128.0.1", "172.32.0.1", "93.184.216.34", "::ffff:8.8.8.8", "2606:4700::1111", "2a00:1450:4001::200e", "64:ff9b::808:808"] {
            assert!(public_ip(a.parse().unwrap()), "{a}");
        }
    }

    #[test]
    fn a_name_resolving_to_the_local_network_is_never_contacted() {
        #[derive(Debug)]
        struct Hostile(std::net::SocketAddr);
        impl Resolver for Hostile {
            fn resolve(&self, _: &ureq::http::Uri, _: &ureq::config::Config, _: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
                let mut v = self.empty();
                v.push(self.0);
                Ok(v)
            }
        }
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_ip().unwrap();
        let hits = Arc::new(Mutex::new(0));
        let h2 = hits.clone();
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                *h2.lock().unwrap() += 1;
                let _ = req.respond(tiny_http::Response::from_data(png_of(16, 16)));
            }
        });
        let url = format!("http://rebind.example:{}/favicon.ico", addr.port());
        let control = Fetcher { agent: agent(false, Hostile(addr)), origin: None };
        assert!(control.agent.get(&url).call().is_ok(), "control: without the filter the private address is reached");
        assert_eq!(*hits.lock().unwrap(), 1);
        let guarded = Fetcher { agent: agent(false, PublicOnly { inner: Hostile(addr), loopback: false }), origin: None };
        assert!(matches!(guarded.agent.get(&url).call(), Err(ureq::Error::HostNotFound)));
        assert_eq!(*hits.lock().unwrap(), 1, "the local server was contacted");
    }

    #[test]
    fn fetches_from_a_local_site() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", server.server_addr().to_ip().unwrap());
        let big = png_of(128, 128);
        let ico = {
            let mut c = Cursor::new(Vec::new());
            image::load_from_memory(&png_of(32, 32)).unwrap().write_to(&mut c, ImageFormat::Ico).unwrap();
            c.into_inner()
        };
        let hits = Arc::new(Mutex::new(Vec::<String>::new()));
        let h2 = hits.clone();
        let mode = Arc::new(Mutex::new("links"));
        let m2 = mode.clone();
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                let url = req.url().to_owned();
                h2.lock().unwrap().push(url.clone());
                let m = *m2.lock().unwrap();
                let html = |b: &str| tiny_http::Response::from_data(b.as_bytes().to_vec()).with_header("Content-Type: text/html".parse::<tiny_http::Header>().unwrap());
                let _ = match (m, url.as_str()) {
                    ("links", "/") => req.respond(html(r#"<head><link rel="icon" href="/static/missing.png"><link rel=apple-touch-icon href="/go"></head>"#)),
                    ("links", "/go") => req.respond(tiny_http::Response::empty(302).with_header("Location: /static/touch.png".parse::<tiny_http::Header>().unwrap())),
                    ("links", "/static/touch.png") => req.respond(tiny_http::Response::from_data(big.clone())),
                    ("ico", "/") => req.respond(html("<head><title>no icons</title></head>")),
                    ("ico", "/favicon.ico") => req.respond(tiny_http::Response::from_data(ico.clone())),
                    _ => req.respond(tiny_http::Response::empty(404)),
                };
            }
        });
        let f = Fetcher::for_test(&origin);
        let png = f.icon("site.example").unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().width(), 64);
        *mode.lock().unwrap() = "ico";
        let png = f.icon("site.example").unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().width(), 32);
        *mode.lock().unwrap() = "none";
        assert!(f.icon("site.example").is_none());
        let h = hits.lock().unwrap().clone();
        assert!(h.contains(&"/static/missing.png".to_owned()) && h.contains(&"/favicon.ico".to_owned()));
        let real = Fetcher::new();
        assert!(real.get(&format!("{origin}/"), MAX_PAGE).is_none());
        for u in ["https://192.168.1.1/favicon.ico", "https://localhost/x.png", "https://nas.local/i.png", "http://example.com/"] {
            assert!(!real.allowed(u), "{u}");
        }
        assert!(real.allowed("https://www.example.com/favicon.ico"));
    }
}
