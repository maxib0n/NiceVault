use super::crypto::{self, Half};
use super::*;
use crate::passwords::Clipboard;
use crate::store::{Locations, Store};
use nv_bridge::{read_frame, write_frame};
use nv_core::passwords::EntryPatch;
use secrecy::SecretString;
use serde_json::{Value, json};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use x25519_dalek::{EphemeralSecret, PublicKey};

#[derive(Default)]
struct FakeClip(Mutex<Option<String>>);
impl Clipboard for FakeClip {
    fn read(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }
    fn write(&self, t: &str) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(t.to_owned());
        Ok(())
    }
    fn clear(&self) {
        *self.0.lock().unwrap() = None;
    }
}

#[derive(Default)]
struct Fake {
    prompts: Mutex<Vec<PairPrompt>>,
    unlocks: Mutex<Vec<String>>,
    done: Mutex<u32>,
    clip: Arc<FakeClip>,
}
struct H(Arc<Fake>);
impl Host for H {
    fn pair_prompt(&self, p: &PairPrompt) {
        self.0.prompts.lock().unwrap().push(p.clone());
    }
    fn pair_closed(&self, _: &str) {}
    fn unlock_window(&self, path: &str) {
        self.0.unlocks.lock().unwrap().push(path.to_owned());
    }
    fn unlock_done(&self) {
        *self.0.done.lock().unwrap() += 1;
    }
    fn clipboard(&self) -> Option<Arc<dyn Clipboard>> {
        Some(self.0.clip.clone())
    }
    fn changed(&self) {}
    fn show_app(&self) {}
}

struct World {
    _tmp: tempfile::TempDir,
    app: Arc<AppState>,
    b: Arc<Browser>,
    fake: Arc<Fake>,
    vault: String,
    path: String,
    config: std::path::PathBuf,
}

fn patch(v: Value) -> EntryPatch {
    serde_json::from_value(v).unwrap()
}

fn world() -> World {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let app = AppState::new(
        Store::new(Locations { exe_dir: Some(tmp.path().join("none")), removable: Some(vec![]), config_dir: Some(config.clone()) }),
        Box::new(crate::NoEvents),
    );
    let path = tmp.path().join("Personale.nvault");
    let vault = app.vault_create(path.to_str().unwrap(), SecretString::from("segreto".to_owned()), Some("fast")).unwrap().id;
    app.pm_create(&vault, patch(json!({ "title": "Posta Example", "username": "mario@example.com", "password": "pw-posta", "urls": ["https://mail.example.com"], "totp": "JBSWY3DPEHPK3PXP" }))).unwrap();
    app.pm_create(&vault, patch(json!({ "title": "Example", "username": "mario.r", "password": "pw-example", "urls": ["example.com"] }))).unwrap();
    app.pm_create(&vault, patch(json!({ "title": "PayPal", "username": "m@paypal", "password": "pw-paypal", "urls": ["https://www.paypal.com"] }))).unwrap();
    app.pm_create(&vault, patch(json!({ "title": "Mai", "username": "never", "password": "x", "urls": ["https://never.example.org"], "fields": [{ "name": "nv:match", "value": "never", "hidden": false }] }))).unwrap();
    let fake = Arc::new(Fake::default());
    let b = Browser::new(Arc::downgrade(&app), Some(config.clone()), Box::new(H(fake.clone())));
    World { app, b, fake, vault, path: path.to_string_lossy().into_owned(), config, _tmp: tmp }
}

struct Client {
    s: UnixStream,
    send: Option<Half>,
    recv: Option<Half>,
    id: u64,
    pushes: Vec<Value>,
}

impl Client {
    fn connect(b: &Arc<Browser>) -> Client {
        let (a, s) = UnixStream::pair().unwrap();
        let b = b.clone();
        std::thread::spawn(move || {
            let w = a.try_clone().unwrap();
            b.serve(a, w);
        });
        let mut c = Client { s, send: None, recv: None, id: 0, pushes: vec![] };
        c.plain(&json!({ "bridge_hello": { "caller": "chrome-extension://test/" } }));
        c
    }
    fn plain(&mut self, v: &Value) {
        write_frame(&mut self.s, v.to_string().as_bytes()).unwrap();
    }
    fn read(&mut self) -> Value {
        serde_json::from_slice(&read_frame(&mut self.s, 1 << 22).unwrap().expect("a frame")).unwrap()
    }
    fn begin(&mut self, epk: &[u8; 32], label: &str) -> [u8; 32] {
        self.plain(&json!({ "t": "pair_begin", "commit": b64(&crypto::pair_commit(epk)), "browser": label }));
        let w = self.read();
        assert_eq!(w["t"], "pair_wait", "{w}");
        let epk_a: [u8; 32] = unb64(w["epk"].as_str().unwrap()).unwrap().try_into().unwrap();
        self.plain(&json!({ "t": "pair_reveal", "epk": b64(epk) }));
        epk_a
    }
    fn pair(&mut self, b: &Browser) -> (String, crypto::Key) {
        let esk = EphemeralSecret::random_from_rng(rand_core::OsRng);
        let epk = PublicKey::from(&esk).to_bytes();
        let epk_a = self.begin(&epk, "Chrome 156 · Linux");
        let code = crypto::sas(&epk, &epk_a);
        let p = loop {
            if let Some(p) = b.pending_prompt() {
                break p;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(p.code, code);
        assert_eq!(p.label, "Chrome 156 · Linux");
        assert_eq!(p.caller, "chrome-extension://test/");
        assert!(b.pair_answer(&p.id, Answer::Accept));
        let d = self.read();
        assert_eq!(d["t"], "pair_done", "{d}");
        let shared = esk.diffie_hellman(&PublicKey::from(epk_a));
        (d["pair_id"].as_str().unwrap().to_owned(), crypto::hkdf(shared.as_bytes(), &[epk, epk_a].concat(), crypto::PAIR_INFO))
    }
    fn hello(&mut self, pair_id: &str, key: &[u8; 32]) -> Value {
        let n_e = [7u8; 16];
        self.plain(&json!({ "t": "hello", "pair_id": pair_id, "n": b64(&n_e) }));
        let h = self.read();
        if h["t"] != "hello_ok" {
            return h;
        }
        let n_a = unb64(h["n"].as_str().unwrap()).unwrap();
        let (send, mut recv) = crypto::session(key, &n_e, &n_a);
        let info: Value = serde_json::from_slice(&recv.open(&unb64(h["c"].as_str().unwrap()).unwrap()).unwrap()).unwrap();
        self.send = Some(send);
        self.recv = Some(recv);
        info
    }
    fn sealed(&mut self, v: &Value) -> Vec<u8> {
        self.send.as_mut().unwrap().seal(v.to_string().as_bytes())
    }
    fn req(&mut self, mut v: Value) -> Value {
        self.id += 1;
        v["id"] = self.id.into();
        v["v"] = 1.into();
        let c = self.sealed(&v);
        self.plain(&json!({ "t": "m", "c": b64(&c) }));
        loop {
            let f = self.read();
            assert_eq!(f["t"], "m", "{f}");
            let p: Value = serde_json::from_slice(&self.recv.as_mut().unwrap().open(&unb64(f["c"].as_str().unwrap()).unwrap()).expect("opens")).unwrap();
            if p["id"] == self.id {
                return p;
            }
            self.pushes.push(p);
        }
    }
}

fn paired(w: &World) -> (Client, String, crypto::Key) {
    let mut c = Client::connect(&w.b);
    let (id, k) = c.pair(&w.b);
    let info = c.hello(&id, &k);
    assert_eq!(info["proto"], json!([1, 1]));
    (c, id, k)
}

fn titles(r: &Value) -> Vec<String> {
    r["entries"].as_array().unwrap().iter().map(|e| e["title"].as_str().unwrap().to_owned()).collect()
}

fn ref_of(r: &Value, title: &str) -> String {
    r["entries"].as_array().unwrap().iter().find(|e| e["title"] == title).unwrap()["ref"].as_str().unwrap().to_owned()
}

#[test]
fn pairing_is_kept_private_and_survives_a_restart() {
    let w = world();
    let (_c, id, k) = paired(&w);
    let f = w.config.join(PAIRS_FILE);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
    let b2 = Browser::new(Arc::downgrade(&w.app), Some(w.config.clone()), Box::new(H(w.fake.clone())));
    assert_eq!(b2.paired().len(), 1);
    let mut c2 = Client::connect(&b2);
    assert_eq!(c2.hello(&id, &k)["label"], "Chrome 156 · Linux");
    let mut c3 = Client::connect(&b2);
    c3.plain(&json!({ "t": "hello", "pair_id": id, "n": b64(&[1u8; 16]) }));
    let h = c3.read();
    let (_, mut recv) = crypto::session(&[0u8; 32], &[1u8; 16], &unb64(h["n"].as_str().unwrap()).unwrap());
    assert!(recv.open(&unb64(h["c"].as_str().unwrap()).unwrap()).is_none());
    let mut c4 = Client::connect(&b2);
    assert_eq!(c4.hello("nope", &k)["t"], "unpaired");
}

#[test]
fn refused_pairing_then_wait_then_block() {
    let w = world();
    let mut c = Client::connect(&w.b);
    let esk = EphemeralSecret::random_from_rng(rand_core::OsRng);
    c.begin(&PublicKey::from(&esk).to_bytes(), "Firefox");
    let p = loop {
        if let Some(p) = w.b.pending_prompt() {
            break p;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    w.b.pair_answer(&p.id, Answer::Refuse);
    assert_eq!(c.read()["code"], "pair_refused");
    c.begin(&PublicKey::from(&esk).to_bytes(), "Firefox");
    assert_eq!(c.read()["code"], "pair_later");
    assert!(w.b.paired().is_empty());
    c.begin(&[0u8; 32], "x");
    assert_eq!(c.read()["code"], "bad_key");
}

#[test]
fn status_match_and_fill() {
    let w = world();
    let (mut c, _, _) = paired(&w);
    let s = c.req(json!({ "type": "status" }));
    let vols = s["volumes"].as_array().unwrap();
    assert_eq!(vols.len(), 1);
    assert_eq!(vols[0]["state"], "open");
    assert_eq!(vols[0]["name"], "Personale");
    assert_eq!(vols[0]["default"], true, "the only volume is the default");
    assert_eq!(vols[0]["alias"].as_str().unwrap().len(), 20);

    let m = c.req(json!({ "type": "match", "top": "https://mail.example.com/inbox", "frame": "https://mail.example.com" }));
    assert_eq!(titles(&m), ["Posta Example", "Example"], "exact host first");
    assert_eq!(m["entries"][0]["quality"], "host");
    assert_eq!(m["entries"][1]["quality"], "site");
    assert_eq!(m["site"], "example.com");
    assert!(m["entries"][0].get("password").is_none(), "no secrets in a match");

    let f = c.req(json!({ "type": "fill", "ref": ref_of(&m, "Posta Example"), "frame": "https://mail.example.com", "top": "https://mail.example.com" }));
    assert_eq!(f["username"], "mario@example.com");
    assert_eq!(f["password"], "pw-posta");
    assert_eq!(f["has_totp"], true);
    let t = c.req(json!({ "type": "totp", "ref": ref_of(&m, "Posta Example"), "frame": "https://mail.example.com" }));
    assert_eq!(t["code"].as_str().unwrap().len(), 6);
    assert!(t["remaining"].as_u64().unwrap() >= 5);
    let a = w.b.accesses();
    assert_eq!(a.len(), 2);
    assert_eq!((a[0].kind.as_str(), a[0].site.as_str(), a[0].refused), ("totp", "mail.example.com", false));

    let n = c.req(json!({ "type": "match", "frame": "https://never.example.org" }));
    assert!(titles(&n).is_empty());
    let (mut c2, _, _) = paired_again(&w);
    assert_eq!(c2.req(json!({ "type": "fill", "ref": ref_of(&m, "Example"), "frame": "https://example.com" }))["error"]["code"], "not_found");
}

fn paired_again(w: &World) -> (Client, String, crypto::Key) {
    let id = w.b.paired()[0].id.clone();
    let k = crypto::Key::new(unhex32(&lock_or_recover(&w.b.pairs)[0].key).unwrap());
    let mut c = Client::connect(&w.b);
    c.hello(&id, &k);
    (c, id, k)
}

#[test]
fn secrets_follow_the_frame_origin() {
    let w = world();
    let (mut c, _, _) = paired(&w);
    let m = c.req(json!({ "type": "match", "top": "https://shop.other.net", "frame": "https://accounts.example.com" }));
    assert_eq!(m["cross_site"], true);
    let r = ref_of(&m, "Example");
    assert_eq!(c.req(json!({ "type": "fill", "ref": r, "frame": "https://accounts.example.com" }))["password"], "pw-example");
    assert_eq!(c.req(json!({ "type": "fill", "ref": r, "frame": "https://shop.other.net" }))["error"]["code"], "mismatch");
    assert_eq!(c.req(json!({ "type": "fill", "ref": r, "frame": "https://shop.other.net", "confirm": ["mismatch"] }))["password"], "pw-example");
    assert_eq!(c.req(json!({ "type": "fill", "ref": r, "frame": "http://example.com" }))["error"]["code"], "insecure");
    let mh = c.req(json!({ "type": "match", "frame": "http://www.example.com" }));
    assert_eq!(mh["insecure_page"], true);
    let la = c.req(json!({ "type": "match", "frame": "https://www.paypa1.com" }));
    assert!(titles(&la).is_empty());
    assert_eq!(la["lookalike"]["of"], "paypal.com");
    let s = c.req(json!({ "type": "search", "query": "paypal", "frame": "https://www.paypa1.com" }));
    let pr = ref_of(&s, "PayPal");
    assert_eq!(c.req(json!({ "type": "fill", "ref": pr, "frame": "https://www.paypa1.com", "confirm": ["mismatch"] }))["error"]["code"], "lookalike");
    assert_eq!(c.req(json!({ "type": "match", "frame": "null" }))["blocked"], "origin");
    assert_eq!(c.req(json!({ "type": "fill", "ref": r, "frame": "chrome://settings", "confirm": ["mismatch"] }))["error"]["code"], "origin");
    assert!(w.b.accesses().iter().any(|a| a.refused));
}

#[test]
fn copy_goes_to_the_app_clipboard() {
    let w = world();
    let (mut c, _, _) = paired(&w);
    let s = c.req(json!({ "type": "search", "query": "posta" }));
    let r = ref_of(&s, "Posta Example");
    assert_eq!(c.req(json!({ "type": "copy", "ref": r, "field": "password" }))["clear_after"], 30);
    assert_eq!(w.fake.clip.read().as_deref(), Some("pw-posta"));
    c.req(json!({ "type": "copy", "ref": r, "field": "username" }));
    assert_eq!(w.fake.clip.read().as_deref(), Some("mario@example.com"));
}

#[test]
fn rate_limit_per_session() {
    let w = world();
    let (mut c, _, _) = paired(&w);
    let m = c.req(json!({ "type": "match", "frame": "https://example.com" }));
    let r = ref_of(&m, "Example");
    for _ in 0..30 {
        assert_eq!(c.req(json!({ "type": "fill", "ref": r, "frame": "https://example.com" }))["ok"], true);
    }
    assert_eq!(c.req(json!({ "type": "fill", "ref": r, "frame": "https://example.com" }))["error"]["code"], "rate_limited");
    assert!(w.b.accesses()[0].refused);
}

#[test]
fn lookups_are_not_activity_fills_are() {
    let w = world();
    let (mut c, _, _) = paired(&w);
    let idle = || lock_or_recover(&w.app.vaults).get(&w.vault).unwrap().activity.idle(std::time::Instant::now());
    std::thread::sleep(std::time::Duration::from_millis(300));
    let m = c.req(json!({ "type": "match", "frame": "https://example.com" }));
    c.req(json!({ "type": "search", "query": "" }));
    c.req(json!({ "type": "status" }));
    assert!(idle() >= std::time::Duration::from_millis(250), "lookups do not reset the idle clock");
    c.req(json!({ "type": "fill", "ref": ref_of(&m, "Example"), "frame": "https://example.com" }));
    assert!(idle() < std::time::Duration::from_millis(250), "a fill does");
}

#[test]
fn per_volume_switch_and_locked_volumes() {
    let w = world();
    let (mut c, _, _) = paired(&w);
    w.app.settings_set(serde_json::from_value(json!({ "browser_enabled": false })).unwrap(), Some(&w.vault)).unwrap();
    assert!(titles(&c.req(json!({ "type": "match", "frame": "https://example.com" }))).is_empty());
    let st = c.req(json!({ "type": "status" }));
    assert_eq!(st["volumes"][0]["browser_enabled"], false);
    w.app.settings_set(serde_json::from_value(json!({ "browser_enabled": true })).unwrap(), Some(&w.vault)).unwrap();

    let alias = st["volumes"][0]["alias"].as_str().unwrap().to_owned();
    w.app.vault_lock(&w.vault, "user").unwrap();
    w.b.notify();
    let m = c.req(json!({ "type": "match", "frame": "https://example.com" }));
    assert!(c.pushes.iter().any(|p| p["type"] == "volumes_changed"));
    assert!(titles(&m).is_empty());
    assert_eq!(m["locked"], json!([alias]));
    let st = c.req(json!({ "type": "status" }));
    assert_eq!(st["volumes"][0]["state"], "locked");
    assert_eq!(st["volumes"][0]["alias"], alias.as_str(), "the alias stays the same while locked");
    assert_eq!(c.req(json!({ "type": "unlock_request", "alias": alias }))["shown"], true);
    assert_eq!(*w.fake.unlocks.lock().unwrap(), vec![w.path.clone()]);
    w.b.opened(Some(&w.path));
    assert_eq!(*w.fake.done.lock().unwrap(), 1);
}

#[test]
fn default_volume_setting() {
    let w = world();
    let (mut c, _, _) = paired(&w);
    let other = w._tmp.path().join("Lavoro.nvault");
    let id2 = w.app.vault_create(other.to_str().unwrap(), SecretString::from("x".to_owned()), Some("fast")).unwrap().id;
    let st = c.req(json!({ "type": "status" }));
    assert!(st["volumes"].as_array().unwrap().iter().all(|v| v["default"] == false), "two volumes, none chosen");
    set(&w.app, &w.b, BrowserPatch { link: None, default_volume: Some(other.to_string_lossy().into_owned()) }).unwrap();
    let st = c.req(json!({ "type": "status" }));
    let d: Vec<&Value> = st["volumes"].as_array().unwrap().iter().filter(|v| v["default"] == true).collect();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0]["name"], "Lavoro");
    w.app.vault_lock(&id2, "user").unwrap();
    w.app.vault_lock(&w.vault, "user").unwrap();
    assert_eq!(c.req(json!({ "type": "unlock_request" }))["shown"], true);
    assert_eq!(w.fake.unlocks.lock().unwrap().last().unwrap(), &other.to_string_lossy().into_owned());
}

#[test]
fn replayed_or_forged_frames_end_the_session_and_unpair_ends_it_too() {
    let w = world();
    let (mut c, id, k) = paired(&w);
    let v = json!({ "id": 99, "type": "status" });
    let sealed = c.sealed(&v);
    c.plain(&json!({ "t": "m", "c": b64(&sealed) }));
    let _ = c.read();
    c.plain(&json!({ "t": "m", "c": b64(&sealed) }));
    assert_eq!(c.read()["t"], "bad_frame");
    assert!(read_frame(&mut c.s, 1 << 20).unwrap().is_none(), "closed");

    let mut c2 = Client::connect(&w.b);
    c2.hello(&id, &k);
    assert_eq!(c2.req(json!({ "type": "nope" }))["error"]["code"], "unsupported");
    w.b.unpair(&id);
    assert_eq!(c2.read()["t"], "unpaired");
    let mut c3 = Client::connect(&w.b);
    assert_eq!(c3.hello(&id, &k)["t"], "unpaired");
}

#[test]
fn never_and_disabled_volumes_release_nothing_even_when_confirmed() {
    let w = world();
    let (mut c, _, _) = paired(&w);
    let s = c.req(json!({ "type": "search", "query": "Mai", "frame": "https://x.example" }));
    let never = ref_of(&s, "Mai");
    let f = c.req(json!({ "type": "fill", "ref": never, "frame": "https://x.example", "confirm": ["mismatch"] }));
    assert_eq!(f["error"]["code"], "never", "{f}");
    let f = c.req(json!({ "type": "fill", "ref": never, "frame": "https://never.example.org", "confirm": ["mismatch", "insecure"] }));
    assert_eq!(f["error"]["code"], "never", "{f}");
    assert_eq!(c.req(json!({ "type": "totp", "ref": never, "frame": "https://x.example", "confirm": ["mismatch"] }))["error"]["code"], "never");

    let m = c.req(json!({ "type": "match", "frame": "https://example.com" }));
    let r = ref_of(&m, "Example");
    w.app.settings_set(serde_json::from_value(json!({ "browser_enabled": false })).unwrap(), Some(&w.vault)).unwrap();
    for kind in ["fill", "totp", "copy"] {
        let f = c.req(json!({ "type": kind, "ref": r, "frame": "https://example.com" }));
        assert!(f.get("password").is_none() && f.get("code").is_none() && f["error"]["code"].is_string(), "{kind}: {f}");
    }
}

#[test]
fn refs_do_not_grow_with_repeated_lookups() {
    let w = world();
    let (mut c, _, _) = paired(&w);
    let first = c.req(json!({ "type": "match", "frame": "https://mail.example.com" }));
    let mut again = first.clone();
    for _ in 0..50 {
        again = c.req(json!({ "type": "match", "frame": "https://mail.example.com" }));
        c.req(json!({ "type": "search", "query": "e", "frame": "https://mail.example.com" }));
    }
    assert_eq!(ref_of(&first, "Posta Example"), ref_of(&again, "Posta Example"), "a new ref per lookup");
    let s = c.req(json!({ "type": "search", "query": "Posta", "frame": "https://mail.example.com" }));
    assert_eq!(ref_of(&s, "Posta Example"), ref_of(&first, "Posta Example"));
    let f = c.req(json!({ "type": "fill", "ref": ref_of(&first, "Posta Example"), "frame": "https://mail.example.com" }));
    assert_eq!(f["password"], "pw-posta");
}

#[test]
fn pairing_commits_before_the_app_key() {
    let w = world();
    let mut c = Client::connect(&w.b);
    let esk = EphemeralSecret::random_from_rng(rand_core::OsRng);
    let epk = PublicKey::from(&esk).to_bytes();
    c.plain(&json!({ "t": "pair_begin", "epk": b64(&epk), "browser": "Old" }));
    let r = c.read();
    assert_eq!((r["t"].as_str(), r["code"].as_str()), (Some("pair_refused"), Some("old_extension")), "{r}");
    c.plain(&json!({ "t": "pair_begin", "commit": b64(&crypto::pair_commit(&epk)), "browser": "x" }));
    assert_eq!(c.read()["t"], "pair_wait");
    let other = PublicKey::from(&EphemeralSecret::random_from_rng(rand_core::OsRng)).to_bytes();
    c.plain(&json!({ "t": "pair_reveal", "epk": b64(&other) }));
    assert_eq!(c.read()["code"], "bad_commit");
    c.plain(&json!({ "t": "pair_reveal", "epk": b64(&epk) }));
    assert_eq!(c.read()["code"], "bad_commit");
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(w.b.pending_prompt().is_none(), "a prompt was shown");
    assert!(w.b.paired().is_empty());
}
