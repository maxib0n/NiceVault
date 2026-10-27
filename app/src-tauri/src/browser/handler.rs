use super::crypto::{self, Half, Key};
use super::{Access, Browser, Refs, SpaceRow, Target, b64, unb64};
use crate::state::{AppState, lock_or_recover, random_id};
use nv_bridge::{MAX_IN, read_frame, write_frame};
use nv_core::passwords::matching::{self, Hit, Origin, Rule};
use nv_core::passwords::totp::Totp;
use nv_core::passwords::{EntrySummary, Secret};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const MAX_ROWS: usize = 50;
const FILLS_PER_MIN: usize = 30;
const ICON_MAX: usize = 16 * 1024;
const ICONS_TOTAL: usize = 256 * 1024;

struct Session {
    send: Half,
    pair_id: String,
    label: String,
    key: Key,
}

pub struct Conn {
    browser: Weak<Browser>,
    out: Mutex<Box<dyn Write + Send>>,
    sess: Mutex<Option<Session>>,
    refs: Mutex<Refs>,
    fills: Mutex<VecDeque<Instant>>,
    caller: Mutex<String>,
    ended: AtomicBool,
    pairing: Mutex<Option<([u8; 32], x25519_dalek::StaticSecret, String)>>,
}

pub(crate) fn open_local(app: &AppState) -> Vec<(String, String)> {
    lock_or_recover(&app.vaults).values().map(|o| (o.path.to_string_lossy().into_owned(), o.name.clone())).collect()
}

fn err(id: &Value, code: &str) -> Value {
    json!({ "id": id, "ok": false, "error": { "code": code } })
}

struct Vol {
    alias: String,
    name: String,
    kind: &'static str,
    state: &'static str,
    accent: Option<String>,
    browser_enabled: bool,
    default: bool,
    key: String,
    vault: Option<String>,
}

impl Vol {
    fn json(&self) -> Value {
        json!({ "alias": self.alias, "name": self.name, "kind": self.kind, "state": self.state, "accent": self.accent,
                "browser_enabled": self.browser_enabled, "default": self.default })
    }
}

struct Row {
    target: Target,
    title: String,
    username: String,
    volume: String,
    alias: String,
    kind: &'static str,
    has_totp: bool,
    favorite: bool,
    hit: Option<Hit>,
    icon: Option<String>,
    url_host: Option<String>,
}

fn same_path(a: &str, b: &str) -> bool {
    crate::store::normalize(std::path::Path::new(a)) == crate::store::normalize(std::path::Path::new(b))
}

impl Conn {
    pub fn new(browser: Weak<Browser>, out: Box<dyn Write + Send>) -> Arc<Conn> {
        Arc::new(Conn {
            browser,
            out: Mutex::new(out),
            sess: Mutex::new(None),
            refs: Mutex::default(),
            fills: Mutex::default(),
            caller: Mutex::default(),
            ended: AtomicBool::new(false),
            pairing: Mutex::default(),
        })
    }

    fn write_plain(&self, v: &Value) {
        let mut o = lock_or_recover(&self.out);
        let _ = write_frame(&mut *o, v.to_string().as_bytes());
    }

    pub fn push(&self, v: &Value) {
        let mut g = lock_or_recover(&self.sess);
        let Some(s) = g.as_mut() else { return };
        let c = s.send.seal(v.to_string().as_bytes());
        let frame = json!({ "t": "m", "c": b64(&c) });
        let mut o = lock_or_recover(&self.out);
        let _ = write_frame(&mut *o, frame.to_string().as_bytes());
    }

    pub fn end_if_pair(&self, id: &str) {
        let mut g = lock_or_recover(&self.sess);
        if g.as_ref().is_some_and(|s| s.pair_id == id) {
            *g = None;
            self.ended.store(true, Ordering::SeqCst);
            drop(g);
            self.write_plain(&json!({ "t": "unpaired" }));
        }
    }

    pub fn run<R: std::io::Read>(self: &Arc<Self>, mut input: R) {
        let mut recv: Option<Half> = None;
        while let Ok(Some(f)) = read_frame(&mut input, MAX_IN) {
            if self.ended.load(Ordering::SeqCst) {
                break;
            }
            let Ok(v) = serde_json::from_slice::<Value>(&f) else { break };
            if let Some(h) = v.get("bridge_hello") {
                *lock_or_recover(&self.caller) = h.get("caller").and_then(Value::as_str).unwrap_or("").chars().take(200).collect();
                continue;
            }
            match v.get("t").and_then(Value::as_str) {
                Some("pair_begin") => self.pair_begin(&v),
                Some("pair_reveal") => self.pair_reveal(&v),
                Some("hello") => recv = self.hello(&v),
                Some("m") => {
                    let (Some(r), Some(c)) = (recv.as_mut(), v.get("c").and_then(Value::as_str).and_then(unb64)) else {
                        self.write_plain(&json!({ "t": "no_session" }));
                        continue;
                    };
                    let Some(plain) = r.open(&c) else {
                        self.write_plain(&json!({ "t": "bad_frame" }));
                        break;
                    };
                    let Ok(req) = serde_json::from_slice::<Value>(&plain) else { break };
                    let reply = self.handle(&req);
                    self.push(&reply);
                }
                _ => self.write_plain(&json!({ "t": "unsupported" })),
            }
        }
        *lock_or_recover(&self.sess) = None;
    }

    fn pair_begin(&self, v: &Value) {
        let commit: Option<[u8; 32]> = v.get("commit").and_then(Value::as_str).and_then(unb64).and_then(|k| k.try_into().ok());
        let Some(commit) = commit else {
            *lock_or_recover(&self.pairing) = None;
            return self.write_plain(&json!({ "t": "pair_refused", "code": "old_extension" }));
        };
        let (secret, epk_a) = crypto::pair_secret();
        let label = v.get("browser").and_then(Value::as_str).unwrap_or("Browser").to_owned();
        *lock_or_recover(&self.pairing) = Some((commit, secret, label));
        self.write_plain(&json!({ "t": "pair_wait", "epk": b64(&epk_a) }));
    }

    fn pair_reveal(&self, v: &Value) {
        let Some(b) = self.browser.upgrade() else { return };
        let pending = lock_or_recover(&self.pairing).take();
        let epk: Option<[u8; 32]> = v.get("epk").and_then(Value::as_str).and_then(unb64).and_then(|k| k.try_into().ok());
        let (Some((commit, secret, label)), Some(epk)) = (pending, epk) else {
            return self.write_plain(&json!({ "t": "pair_refused", "code": "bad_commit" }));
        };
        if !subtle_eq(&crypto::pair_commit(&epk), &commit) {
            return self.write_plain(&json!({ "t": "pair_refused", "code": "bad_commit" }));
        }
        let Some(offer) = crypto::pair_with(&secret, &epk) else {
            return self.write_plain(&json!({ "t": "pair_refused", "code": "bad_key" }));
        };
        let caller = lock_or_recover(&self.caller).clone();
        match b.pair_request(&label, &caller, offer) {
            Ok(id) => self.write_plain(&json!({ "t": "pair_done", "pair_id": id })),
            Err(code) => self.write_plain(&json!({ "t": "pair_refused", "code": code })),
        }
    }

    fn hello(&self, v: &Value) -> Option<Half> {
        let b = self.browser.upgrade()?;
        let pair_id = v.get("pair_id").and_then(Value::as_str).unwrap_or("");
        let n_e = v.get("n").and_then(Value::as_str).and_then(unb64).filter(|n| (16..=64).contains(&n.len()));
        let (Some(n_e), Some((key, label))) = (n_e, b.key_of(pair_id)) else {
            self.write_plain(&json!({ "t": "unpaired" }));
            return None;
        };
        let mut n_a = [0u8; 16];
        getrandom::fill(&mut n_a).expect("OS random");
        let (recv, send) = crypto::session(&key, &n_e, &n_a);
        let portable = b.app().is_none_or(|a| !lock_or_recover(&a.store).installed());
        let info = json!({ "app_version": env!("CARGO_PKG_VERSION"), "proto": super::PROTO, "portable": portable, "label": label });
        let mut g = lock_or_recover(&self.sess);
        let mut s = Session { send, pair_id: pair_id.to_owned(), label, key };
        let c = s.send.seal(info.to_string().as_bytes());
        {
            let mut o = lock_or_recover(&self.out);
            let _ = write_frame(&mut *o, json!({ "t": "hello_ok", "n": b64(&n_a), "c": b64(&c) }).to_string().as_bytes());
        }
        *g = Some(s);
        lock_or_recover(&self.refs).clear();
        Some(recv)
    }

    fn key(&self) -> Option<(Key, String)> {
        lock_or_recover(&self.sess).as_ref().map(|s| (Zeroizing::new(*s.key), s.label.clone()))
    }

    fn handle(&self, req: &Value) -> Value {
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let (Some(b), Some((key, label))) = (self.browser.upgrade(), self.key()) else { return err(&id, "unpaired") };
        let Some(app) = b.app() else { return err(&id, "unavailable") };
        let t = req.get("type").and_then(Value::as_str).unwrap_or("");
        let r = match t {
            "status" => self.status(&b, &app, &key),
            "match" => self.matches(&b, &app, &key, req),
            "search" => self.search(&b, &app, &key, req),
            "fill" | "totp" | "copy" => self.secret(&b, &app, &label, t, req),
            "unlock_request" => self.unlock(&b, &app, &key, req),
            "show" => {
                b.host.show_app();
                Ok(json!({}))
            }
            _ => Err("unsupported"),
        };
        match r {
            Ok(Value::Object(mut m)) => {
                m.insert("id".into(), id);
                m.insert("ok".into(), true.into());
                Value::Object(m)
            }
            Ok(_) => json!({ "id": id, "ok": true }),
            Err(code) => err(&id, code),
        }
    }

    fn volumes(&self, b: &Browser, app: &AppState, key: &Key) -> Vec<Vol> {
        let default = lock_or_recover(&app.store).pm_default_volume();
        let mut out: Vec<Vol> = lock_or_recover(&app.vaults)
            .values()
            .map(|o| {
                let path = o.path.to_string_lossy().into_owned();
                Vol {
                    alias: crypto::alias(key, &format!("local:{}", crate::store::normalize(&o.path).display())),
                    name: o.name.clone(),
                    kind: "local",
                    state: "open",
                    accent: Some(o.autolock.accent.clone()),
                    browser_enabled: o.autolock.browser_enabled,
                    default: default.as_deref().is_some_and(|d| same_path(d, &path)),
                    key: path,
                    vault: Some(o.id.clone()),
                }
            })
            .collect();
        let recent = lock_or_recover(&app.store).recent();
        for r in recent {
            if out.iter().any(|v| v.kind == "local" && same_path(&v.key, &r.path)) {
                continue;
            }
            out.push(Vol {
                alias: crypto::alias(key, &format!("local:{}", crate::store::normalize(std::path::Path::new(&r.path)).display())),
                name: r.name.clone(),
                kind: "local",
                state: if r.exists { "locked" } else { "absent" },
                accent: r.accent.clone(),
                browser_enabled: true,
                default: default.as_deref().is_some_and(|d| same_path(d, &r.path)),
                key: r.path,
                vault: None,
            });
        }
        if default.is_none() && out.iter().filter(|v| v.kind == "local").count() == 1 {
            out[0].default = true;
        }
        if let Some(sp) = b.spaces() {
            for (sid, name) in sp.open() {
                out.push(Vol {
                    alias: crypto::alias(key, &format!("space:{sid}")),
                    name,
                    kind: "space",
                    state: "open",
                    accent: None,
                    browser_enabled: true,
                    default: false,
                    key: sid,
                    vault: None,
                });
            }
        }
        out
    }

    fn status(&self, b: &Browser, app: &AppState, key: &Key) -> Result<Value, &'static str> {
        let v = self.volumes(b, app, key);
        Ok(json!({ "app_version": env!("CARGO_PKG_VERSION"), "volumes": v.iter().map(Vol::json).collect::<Vec<_>>() }))
    }

    fn rows(&self, b: &Browser, app: &AppState, key: &Key, page: Option<&Origin>, query: Option<&str>) -> Vec<Row> {
        let mut rows = vec![];
        for v in self.volumes(b, app, key) {
            if v.state != "open" || !v.browser_enabled {
                continue;
            }
            if let Some(vid) = &v.vault {
                let Some(vault) = lock_or_recover(&app.vaults).get(vid).map(|o| o.vault.clone()) else { continue };
                let list = match query {
                    Some(q) => vault.pm_search(q),
                    None => vault.pm_list(),
                };
                let Ok(list) = list else { continue };
                for s in list {
                    let hit = page.and_then(|p| entry_hit(&vault, &s, p));
                    rows.push(Row {
                        target: Target::Local { vault: vid.clone(), entry: s.id.clone() },
                        title: s.title,
                        username: s.username,
                        volume: v.name.clone(),
                        alias: v.alias.clone(),
                        kind: "local",
                        has_totp: s.has_totp,
                        favorite: s.favorite,
                        hit,
                        url_host: first_host(&s.urls),
                        icon: s.icon,
                    });
                }
            } else if v.kind == "space"
                && let Some(sp) = b.spaces()
            {
                for s in sp.rows(&v.key) {
                    if query.is_some_and(|q| !space_matches(&s, q)) {
                        continue;
                    }
                    let hit = page.and_then(|p| matching::match_entry(&s.urls, p, s.rule));
                    rows.push(Row {
                        target: Target::Space { space: v.key.clone(), entry: s.id.clone() },
                        title: s.title,
                        username: s.username,
                        volume: v.name.clone(),
                        alias: v.alias.clone(),
                        kind: "space",
                        has_totp: s.has_totp,
                        favorite: false,
                        hit,
                        url_host: first_host(&s.urls),
                        icon: None,
                    });
                }
            }
        }
        rows
    }

    fn known_sites(&self, b: &Browser, app: &AppState, key: &Key) -> Vec<String> {
        let mut sites = vec![];
        for v in self.volumes(b, app, key) {
            if v.state != "open" || !v.browser_enabled {
                continue;
            }
            if let Some(vid) = &v.vault {
                let Some(vault) = lock_or_recover(&app.vaults).get(vid).map(|o| o.vault.clone()) else { continue };
                for s in vault.pm_list().unwrap_or_default() {
                    sites.extend(matching::sites_of(&s.urls));
                }
            } else if let Some(sp) = b.spaces().filter(|_| v.kind == "space") {
                for s in sp.rows(&v.key) {
                    sites.extend(matching::sites_of(&s.urls));
                }
            }
        }
        sites.sort();
        sites.dedup();
        sites
    }

    fn row_json(&self, app: &AppState, r: Row, icons: &mut usize) -> Value {
        let rf = lock_or_recover(&self.refs).of(r.target.clone(), random_id);
        let icon = r.icon.as_deref().and_then(|i| {
            let Target::Local { vault, .. } = &r.target else { return None };
            let host = i.split('/').next()?;
            let v = lock_or_recover(&app.vaults).get(vault).map(|o| o.vault.clone())?;
            let png = v.pm_icon_get(host).ok()?;
            (png.len() <= ICON_MAX && *icons + png.len() <= ICONS_TOTAL).then(|| {
                *icons += png.len();
                format!("data:image/png;base64,{}", b64(&png))
            })
        });
        let j = json!({
            "ref": rf, "title": r.title, "username": r.username, "volume": r.volume, "alias": r.alias, "kind": r.kind,
            "has_totp": r.has_totp, "favorite": r.favorite, "icon": icon, "url_host": r.url_host,
            "quality": r.hit.map(|h| h.quality.as_str()), "insecure": r.hit.is_some_and(|h| h.insecure),
        });
        j
    }

    fn matches(&self, b: &Browser, app: &AppState, key: &Key, req: &Value) -> Result<Value, &'static str> {
        let frame_s = req.get("frame").and_then(Value::as_str).unwrap_or("");
        let top = req.get("top").and_then(Value::as_str).and_then(matching::parse_page);
        let Some(frame) = matching::parse_page(frame_s) else {
            return Ok(json!({ "entries": [], "blocked": "origin" }));
        };
        let vols = self.volumes(b, app, key);
        let locked: Vec<&str> = vols.iter().filter(|v| v.state != "open").map(|v| v.alias.as_str()).collect();
        let mut rows: Vec<Row> = self.rows(b, app, key, Some(&frame), None).into_iter().filter(|r| r.hit.is_some()).collect();
        rows.sort_by_cached_key(|r| {
            let h = r.hit.expect("filtered");
            (h.insecure, h.quality, !r.favorite, r.title.to_lowercase())
        });
        rows.truncate(MAX_ROWS);
        let lookalike = if rows.is_empty() { matching::lookalike(&frame, self.known_sites(b, app, key).iter().map(String::as_str)) } else { None };
        let mut icons = 0;
        let entries: Vec<Value> = rows.into_iter().map(|r| self.row_json(app, r, &mut icons)).collect();
        let host_unicode = matching::unicode_host(&frame.host);
        Ok(json!({
            "entries": entries,
            "site": frame.site,
            "host": frame.host,
            "host_unicode": (host_unicode != frame.host).then_some(host_unicode),
            "insecure_page": frame.scheme == "http",
            "top_host": top.as_ref().map(|t| t.host.clone()),
            "cross_site": top.as_ref().is_some_and(|t| t.site != frame.site),
            "locked": locked,
            "lookalike": lookalike.map(|of| json!({ "of": of, "of_unicode": matching::unicode_host(&of) })),
        }))
    }

    fn search(&self, b: &Browser, app: &AppState, key: &Key, req: &Value) -> Result<Value, &'static str> {
        let q = req.get("query").and_then(Value::as_str).unwrap_or("");
        let frame = req.get("frame").and_then(Value::as_str).and_then(matching::parse_page);
        let mut rows = self.rows(b, app, key, frame.as_ref(), Some(q));
        rows.sort_by_cached_key(|r| (r.hit.is_none(), r.hit.map(|h| h.quality), !r.favorite, r.title.to_lowercase()));
        let total = rows.len();
        rows.truncate(MAX_ROWS);
        let mut icons = 0;
        let entries: Vec<Value> = rows.into_iter().map(|r| self.row_json(app, r, &mut icons)).collect();
        let lookalike = frame.as_ref().and_then(|f| matching::lookalike(f, self.known_sites(b, app, key).iter().map(String::as_str)));
        Ok(json!({ "entries": entries, "total": total, "lookalike": lookalike.map(|of| json!({ "of": of })) }))
    }

    fn check_origin(&self, b: &Browser, app: &AppState, target: &Target, frame: &Origin, confirm: &[&str]) -> Result<(), &'static str> {
        let (hit, rule) = match target {
            Target::Local { vault, entry } => {
                let v = lock_or_recover(&app.vaults).get(vault).map(|o| o.vault.clone()).ok_or("locked")?;
                let s = v.pm_list().map_err(|_| "locked")?.into_iter().find(|s| &s.id == entry).ok_or("not_found")?;
                (entry_hit(&v, &s, frame), entry_rule(&v, &s.id))
            }
            Target::Space { space, entry } => {
                let sp = b.spaces().ok_or("locked")?;
                let s = sp.rows(space).into_iter().find(|s| &s.id == entry).ok_or("not_found")?;
                (matching::match_entry(&s.urls, frame, s.rule), s.rule)
            }
        };
        if rule == Rule::Never {
            return Err("never");
        }
        match hit {
            Some(h) if !h.insecure || confirm.contains(&"insecure") => Ok(()),
            Some(_) => Err("insecure"),
            None => {
                let key = self.key().ok_or("unpaired")?.0;
                if matching::lookalike(frame, self.known_sites(b, app, &key).iter().map(String::as_str)).is_some() {
                    Err("lookalike")
                } else if confirm.contains(&"mismatch") {
                    Ok(())
                } else {
                    Err("mismatch")
                }
            }
        }
    }

    fn rate_ok(&self) -> bool {
        let mut g = lock_or_recover(&self.fills);
        let now = Instant::now();
        while g.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60)) {
            g.pop_front();
        }
        if g.len() >= FILLS_PER_MIN {
            return false;
        }
        g.push_back(now);
        true
    }

    fn secret(&self, b: &Browser, app: &AppState, label: &str, kind: &str, req: &Value) -> Result<Value, &'static str> {
        let rf = req.get("ref").and_then(Value::as_str).unwrap_or("");
        let target = lock_or_recover(&self.refs).get(rf).cloned().ok_or("not_found")?;
        if let Target::Local { vault, .. } = &target {
            let key = self.key().ok_or("unpaired")?.0;
            if !self.volumes(b, app, &key).iter().any(|v| v.vault.as_deref() == Some(vault.as_str()) && v.state == "open" && v.browser_enabled) {
                return Err("not_found");
            }
        }
        let frame_s = req.get("frame").and_then(Value::as_str).unwrap_or("");
        let site = matching::parse_page(frame_s).map(|f| f.host).unwrap_or_default();
        let title = match &target {
            Target::Local { vault, entry } => lock_or_recover(&app.vaults)
                .get(vault)
                .and_then(|o| o.vault.pm_list().ok())
                .and_then(|l| l.into_iter().find(|s| &s.id == entry))
                .map(|s| s.title)
                .unwrap_or_default(),
            Target::Space { space, entry } => b.spaces().and_then(|sp| sp.rows(space).into_iter().find(|s| &s.id == entry)).map(|s| s.title).unwrap_or_default(),
        };
        let note = |refused: bool| b.note(Access { at: crate::store::now_ms(), site: site.clone(), title: title.clone(), kind: kind.to_owned(), browser: label.to_owned(), refused });
        if !self.rate_ok() {
            note(true);
            return Err("rate_limited");
        }
        if kind != "copy" {
            let frame = matching::parse_page(frame_s).ok_or("origin")?;
            let confirm: Vec<&str> = req.get("confirm").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            if let Err(e) = self.check_origin(b, app, &target, &frame, &confirm) {
                note(true);
                return Err(e);
            }
        }
        let out = match (&target, kind) {
            (Target::Local { vault, entry }, "fill") => {
                let v = app.vault(vault).map_err(|_| "locked")?;
                let user = v.pm_reveal(entry, Secret::Username).map_err(|_| "not_found")?;
                let pw = v.pm_reveal(entry, Secret::Password).unwrap_or_default();
                let has_totp = v.pm_reveal(entry, Secret::Totp).is_ok();
                json!({ "username": *user, "password": *pw, "has_totp": has_totp, "title": title })
            }
            (Target::Local { vault, entry }, "totp") => {
                let v = app.vault(vault).map_err(|_| "locked")?;
                let uri = v.pm_reveal(entry, Secret::Totp).map_err(|_| "no_totp")?;
                totp_json(&uri)?
            }
            (Target::Local { vault, entry }, _) => {
                let field = match req.get("field").and_then(Value::as_str).unwrap_or("password") {
                    "username" => "username",
                    "totp" => "totp_code",
                    _ => "password",
                };
                let clip = b.host.clipboard().ok_or("clipboard")?;
                let c = app.pm_copy(clip, vault, entry, field, None).map_err(|_| "not_found")?;
                json!({ "clear_after": c.clear_after })
            }
            (Target::Space { space, entry }, k) => {
                let sp = b.spaces().ok_or("locked")?;
                let get = |f: &str| sp.secret(space, entry, f);
                match k {
                    "fill" => json!({ "username": *get("username").unwrap_or_default(), "password": *get("password").unwrap_or_default(), "has_totp": get("totp").is_some(), "title": title }),
                    "totp" => totp_json(&get("totp").ok_or("no_totp")?)?,
                    _ => {
                        let field = req.get("field").and_then(Value::as_str).unwrap_or("password");
                        let value = if field == "totp" {
                            Zeroizing::new(Totp::parse(&get("totp").ok_or("no_totp")?).map_err(|_| "no_totp")?.now().code)
                        } else {
                            get(if field == "username" { "username" } else { "password" }).ok_or("not_found")?
                        };
                        let clip = b.host.clipboard().ok_or("clipboard")?;
                        clip.write(&value).map_err(|_| "clipboard")?;
                        crate::passwords::schedule_clear(clip, value, Duration::from_secs(crate::passwords::DEFAULT_CLEAR_AFTER));
                        json!({ "clear_after": crate::passwords::DEFAULT_CLEAR_AFTER })
                    }
                }
            }
        };
        note(false);
        Ok(out)
    }

    fn unlock(&self, b: &Browser, app: &AppState, key: &Key, req: &Value) -> Result<Value, &'static str> {
        let vols = self.volumes(b, app, key);
        let want = req.get("alias").and_then(Value::as_str);
        let v = match want {
            Some(a) => vols.iter().find(|v| v.alias == a),
            None => vols.iter().find(|v| v.default && v.kind == "local").or_else(|| vols.iter().find(|v| v.kind == "local" && v.state == "locked")),
        }
        .ok_or("not_found")?;
        if v.kind != "local" {
            return Err("unsupported");
        }
        if v.state == "open" {
            return Ok(json!({ "shown": false, "open": true }));
        }
        b.show_unlock(&v.key);
        Ok(json!({ "shown": true }))
    }
}

fn entry_hit(v: &nv_core::Vault, s: &EntrySummary, page: &Origin) -> Option<Hit> {
    let hit = matching::match_entry(&s.urls, page, Rule::Site)?;
    let rule = entry_rule(v, &s.id);
    if rule == Rule::Site { Some(hit) } else { matching::match_entry(&s.urls, page, rule) }
}

fn entry_rule(v: &nv_core::Vault, id: &str) -> Rule {
    v.pm_get(id)
        .ok()
        .and_then(|d| d.fields.into_iter().find(|f| f.name == matching::MATCH_FIELD && !f.hidden).and_then(|f| f.value))
        .map(|r| Rule::parse(&r))
        .unwrap_or_default()
}

fn subtle_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn first_host(urls: &[String]) -> Option<String> {
    urls.iter().find_map(|u| matching::parse_entry(u)).map(|o| matching::unicode_host(&o.host))
}

fn space_matches(s: &SpaceRow, q: &str) -> bool {
    let hay = format!("{} {} {}", s.title, s.username, s.urls.join(" ")).to_lowercase();
    q.split_whitespace().all(|t| hay.contains(&t.to_lowercase()))
}

fn totp_json(uri: &str) -> Result<Value, &'static str> {
    let t = Totp::parse(uri).map_err(|_| "no_totp")?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut c = t.code_at_full(now);
    if c.remaining < 5 {
        c = t.code_at_full(now + c.remaining);
    }
    Ok(json!({ "code": c.code, "remaining": c.remaining, "period": c.period }))
}
