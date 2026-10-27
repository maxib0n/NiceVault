pub mod crypto;
mod handler;
pub mod reg;

use crate::error::{ApiError, ApiResult};
use crate::passwords::Clipboard;
use crate::state::{AppState, lock_or_recover};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use nv_core::passwords::matching::Rule;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub const PROTO: [u32; 2] = [1, 1];
pub const PAIRS_FILE: &str = "browsers.json";
const PAIR_TIMEOUT: Duration = Duration::from_secs(120);
const PAIR_GAP: Duration = Duration::from_secs(30);
const PAIR_BLOCK: Duration = Duration::from_secs(600);
const ACCESS_KEEP: usize = 50;
const MAX_PAIRS: usize = 32;

pub trait Host: Send + Sync {
    fn pair_prompt(&self, p: &PairPrompt);
    fn pair_closed(&self, id: &str);
    fn unlock_window(&self, path: &str);
    fn unlock_done(&self);
    fn clipboard(&self) -> Option<Arc<dyn Clipboard>>;
    fn show_app(&self);
    fn changed(&self);
}

pub trait Spaces: Send + Sync {
    fn open(&self) -> Vec<(String, String)>;
    fn rows(&self, id: &str) -> Vec<SpaceRow>;
    fn secret(&self, id: &str, entry: &str, field: &str) -> Option<Zeroizing<String>>;
}

#[derive(Clone, Debug)]
pub struct SpaceRow {
    pub id: String,
    pub title: String,
    pub username: String,
    pub urls: Vec<String>,
    pub has_totp: bool,
    pub rule: Rule,
}

#[derive(Clone, Debug, Serialize)]
pub struct PairPrompt {
    pub id: String,
    pub label: String,
    pub code: String,
    pub caller: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Accept,
    Refuse,
    Block,
}

#[derive(Clone, Serialize, Deserialize)]
struct Pairing {
    id: String,
    label: String,
    created: u64,
    last_used: u64,
    key: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PairedView {
    pub id: String,
    pub label: String,
    pub created: u64,
    pub last_used: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Access {
    pub at: u64,
    pub site: String,
    pub title: String,
    pub kind: String,
    pub browser: String,
    pub refused: bool,
}

#[derive(Default)]
struct Prompts {
    pending: Option<(PairPrompt, mpsc::Sender<Answer>)>,
    next_ok: Option<Instant>,
    blocked_until: Option<Instant>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RegState {
    pub bridge: Option<String>,
    pub registered: Vec<reg::Written>,
    pub error: Option<String>,
    pub sandboxed: Vec<String>,
}

pub struct Browser {
    app: Weak<AppState>,
    dir: Option<PathBuf>,
    pairs: Mutex<Vec<Pairing>>,
    pairs_unreadable: bool,
    prompts: Mutex<Prompts>,
    conns: Mutex<Vec<Weak<handler::Conn>>>,
    accesses: Mutex<VecDeque<Access>>,
    pub(crate) host: Box<dyn Host>,
    spaces: Mutex<Option<Arc<dyn Spaces>>>,
    reg: Mutex<RegState>,
    unlocking: Mutex<Option<String>>,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut k = [0u8; 32];
    for (i, c) in k.iter_mut().enumerate() {
        *c = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(k)
}

impl Browser {
    pub fn new(app: Weak<AppState>, dir: Option<PathBuf>, host: Box<dyn Host>) -> Arc<Browser> {
        let (pairs, pairs_unreadable) = match dir.as_ref().map(|d| std::fs::read(d.join(PAIRS_FILE))) {
            Some(Ok(b)) => match serde_json::from_slice::<Value>(&b) {
                Ok(v) => (v.get("browsers").cloned().and_then(|b| serde_json::from_value::<Vec<Pairing>>(b).ok()).unwrap_or_default(), false),
                Err(_) => (vec![], true),
            },
            _ => (vec![], false),
        };
        Arc::new(Browser {
            app,
            dir,
            pairs: Mutex::new(pairs),
            pairs_unreadable,
            prompts: Mutex::default(),
            conns: Mutex::default(),
            accesses: Mutex::default(),
            host,
            spaces: Mutex::default(),
            reg: Mutex::default(),
            unlocking: Mutex::default(),
        })
    }

    pub fn set_spaces(&self, s: Arc<dyn Spaces>) {
        *lock_or_recover(&self.spaces) = Some(s);
    }

    pub(crate) fn spaces(&self) -> Option<Arc<dyn Spaces>> {
        lock_or_recover(&self.spaces).clone()
    }

    pub(crate) fn app(&self) -> Option<Arc<AppState>> {
        self.app.upgrade()
    }

    fn save_pairs(&self, pairs: &[Pairing]) {
        let Some(d) = &self.dir else { return };
        if self.pairs_unreadable {
            return;
        }
        if let Ok(b) = serde_json::to_vec_pretty(&json!({ "browsers": pairs })) {
            let _ = crate::store::write_private(&d.join(PAIRS_FILE), &b);
        }
    }

    pub(crate) fn key_of(&self, pair_id: &str) -> Option<(crypto::Key, String)> {
        let mut g = lock_or_recover(&self.pairs);
        let p = g.iter_mut().find(|p| p.id == pair_id)?;
        let k = unhex32(&p.key)?;
        let now = crate::store::now_ms();
        if now.saturating_sub(p.last_used) > 3_600_000 {
            p.last_used = now;
            let all = g.clone();
            drop(g);
            self.save_pairs(&all);
            self.host.changed();
            let g = lock_or_recover(&self.pairs);
            let label = g.iter().find(|p| p.id == pair_id).map(|p| p.label.clone()).unwrap_or_default();
            return Some((Zeroizing::new(k), label));
        }
        Some((Zeroizing::new(k), p.label.clone()))
    }

    pub fn paired(&self) -> Vec<PairedView> {
        lock_or_recover(&self.pairs)
            .iter()
            .map(|p| PairedView { id: p.id.clone(), label: p.label.clone(), created: p.created, last_used: p.last_used })
            .collect()
    }

    pub fn unpair(&self, id: &str) {
        let all = {
            let mut g = lock_or_recover(&self.pairs);
            g.retain(|p| p.id != id);
            g.clone()
        };
        self.save_pairs(&all);
        for c in self.live_conns() {
            c.end_if_pair(id);
        }
        self.host.changed();
    }

    pub(crate) fn pair_request(&self, label: &str, caller: &str, offer: crypto::PairOffer) -> Result<String, &'static str> {
        let (tx, rx) = mpsc::channel();
        let prompt = {
            let mut g = lock_or_recover(&self.prompts);
            let now = Instant::now();
            if g.blocked_until.is_some_and(|t| now < t) {
                return Err("pair_blocked");
            }
            if g.pending.is_some() {
                return Err("pair_busy");
            }
            if g.next_ok.is_some_and(|t| now < t) {
                return Err("pair_later");
            }
            let label: String = label.chars().filter(|c| !c.is_control()).take(80).collect();
            let p = PairPrompt { id: crate::state::random_id(), label, code: offer.sas.clone(), caller: caller.chars().take(120).collect() };
            g.pending = Some((p.clone(), tx));
            p
        };
        self.host.pair_prompt(&prompt);
        let answer = rx.recv_timeout(PAIR_TIMEOUT).unwrap_or(Answer::Refuse);
        {
            let mut g = lock_or_recover(&self.prompts);
            g.pending = None;
            match answer {
                Answer::Accept => {}
                Answer::Refuse => g.next_ok = Some(Instant::now() + PAIR_GAP),
                Answer::Block => g.blocked_until = Some(Instant::now() + PAIR_BLOCK),
            }
        }
        self.host.pair_closed(&prompt.id);
        if answer != Answer::Accept {
            return Err("pair_refused");
        }
        let now = crate::store::now_ms();
        let id = format!("{}{}", crate::state::random_id(), crate::state::random_id());
        let all = {
            let mut g = lock_or_recover(&self.pairs);
            g.push(Pairing { id: id.clone(), label: prompt.label.clone(), created: now, last_used: now, key: hex(offer.key.as_ref()) });
            while g.len() > MAX_PAIRS {
                if let Some(i) = g.iter().enumerate().min_by_key(|(_, p)| p.last_used).map(|(i, _)| i) {
                    g.remove(i);
                }
            }
            g.clone()
        };
        self.save_pairs(&all);
        self.host.changed();
        Ok(id)
    }

    pub fn pair_answer(&self, id: &str, a: Answer) -> bool {
        let g = lock_or_recover(&self.prompts);
        match &g.pending {
            Some((p, tx)) if p.id == id => tx.send(a).is_ok(),
            _ => false,
        }
    }

    pub fn pending_prompt(&self) -> Option<PairPrompt> {
        lock_or_recover(&self.prompts).pending.as_ref().map(|(p, _)| p.clone())
    }

    pub(crate) fn note(&self, a: Access) {
        let mut g = lock_or_recover(&self.accesses);
        g.push_front(a);
        g.truncate(ACCESS_KEEP);
        drop(g);
        self.host.changed();
    }

    pub fn accesses(&self) -> Vec<Access> {
        lock_or_recover(&self.accesses).iter().cloned().collect()
    }

    fn live_conns(&self) -> Vec<Arc<handler::Conn>> {
        let mut g = lock_or_recover(&self.conns);
        g.retain(|w| w.strong_count() > 0);
        g.iter().filter_map(Weak::upgrade).collect()
    }

    pub fn notify(&self) {
        for c in self.live_conns() {
            c.push(&json!({ "type": "volumes_changed" }));
        }
    }

    pub fn opened(&self, path: Option<&str>) {
        let done = {
            let mut g = lock_or_recover(&self.unlocking);
            let hit = g.as_deref().is_some_and(|p| path.is_none_or(|x| crate::store::normalize(std::path::Path::new(x)) == crate::store::normalize(std::path::Path::new(p))));
            if hit {
                *g = None;
            }
            hit
        };
        if done {
            self.host.unlock_done();
        }
        self.notify();
    }

    pub(crate) fn show_unlock(&self, path: &str) {
        *lock_or_recover(&self.unlocking) = Some(path.to_owned());
        self.host.unlock_window(path);
    }

    pub fn unlock_cancelled(&self) {
        *lock_or_recover(&self.unlocking) = None;
        self.host.unlock_done();
    }

    pub fn reg_state(&self) -> RegState {
        lock_or_recover(&self.reg).clone()
    }

    pub fn set_reg_state(&self, r: RegState) {
        *lock_or_recover(&self.reg) = r;
        self.host.changed();
    }

    pub fn serve<R: std::io::Read, W: Write + Send + 'static>(self: &Arc<Self>, input: R, output: W) {
        let conn = handler::Conn::new(Arc::downgrade(self), Box::new(output));
        lock_or_recover(&self.conns).push(Arc::downgrade(&conn));
        conn.run(input);
    }
}

pub fn listen(b: &Arc<Browser>) {
    let b = Arc::downgrade(b);
    let _ = std::thread::Builder::new().name("nv-browser".into()).spawn(move || {
        let path = match nv_bridge::ipc::endpoint() {
            Ok(p) => p,
            Err(e) => return log::warn!("browser endpoint: {e}"),
        };
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut l = match nv_bridge::ipc::Listener::bind(&path) {
            Ok(l) => l,
            Err(e) => return log::warn!("browser endpoint {}: {e}", path.display()),
        };
        #[cfg(windows)]
        {
            l.client_image = std::env::current_exe().ok();
        }
        loop {
            let Ok(s) = l.accept() else { continue };
            let Some(br) = b.upgrade() else { return };
            let _ = std::thread::Builder::new().name("nv-browser-conn".into()).spawn(move || {
                let Ok(w) = s.try_clone() else { return };
                br.serve(s, w);
            });
        }
    });
}

#[derive(Clone, Debug, Serialize)]
pub struct KnownVolume {
    pub path: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BrowserInfo {
    pub link: bool,
    pub portable: bool,
    #[serde(flatten)]
    pub reg: RegState,
    pub paired: Vec<PairedView>,
    pub accesses: Vec<Access>,
    pub default_volume: Option<String>,
    pub volumes: Vec<KnownVolume>,
    pub pending: Option<PairPrompt>,
}

#[derive(Debug, Default, Deserialize)]
pub struct BrowserPatch {
    pub link: Option<bool>,
    pub default_volume: Option<String>,
}

impl Browser {
    pub fn info(&self) -> BrowserInfo {
        let app = self.app();
        let (link, portable, default_volume) = app.as_ref().map_or((false, true, None), |a| {
            let s = lock_or_recover(&a.store);
            (s.browser_link(), !s.installed(), s.pm_default_volume())
        });
        let mut volumes: Vec<KnownVolume> = vec![];
        if let Some(a) = &app {
            for (path, name) in handler::open_local(a) {
                volumes.push(KnownVolume { path, name });
            }
            for r in lock_or_recover(&a.store).recent() {
                if !volumes.iter().any(|v| v.path == r.path) {
                    volumes.push(KnownVolume { path: r.path, name: r.name });
                }
            }
        }
        BrowserInfo {
            link,
            portable,
            reg: self.reg_state(),
            paired: self.paired(),
            accesses: self.accesses(),
            default_volume,
            volumes,
            pending: self.pending_prompt(),
        }
    }
}

pub fn sync_registration(b: &Arc<Browser>) {
    let b = b.clone();
    std::thread::spawn(move || {
        let Some(app) = b.app() else { return };
        let (link, installed) = {
            let s = lock_or_recover(&app.store);
            (s.browser_link(), s.installed())
        };
        let home = reg::homes();
        let mut st = RegState { sandboxed: home.as_ref().map(|h| reg::sandboxed(&h.home)).unwrap_or_default(), ..Default::default() };
        if !installed {
            b.set_reg_state(st);
            return;
        }
        let bridge = reg::host_exe();
        st.bridge = bridge.as_ref().map(|p| p.to_string_lossy().into_owned());
        match (link, home, bridge) {
            (true, Some(h), Some(br)) => match reg::register(&h, &br) {
                Ok(w) => st.registered = w,
                Err(e) => st.error = Some(e.to_string()),
            },
            (true, _, None) => st.error = Some("no_executable".into()),
            (true, None, _) => st.error = Some("no home folder".into()),
            (false, Some(h), _) => {
                if let Err(e) = reg::unregister(&h) {
                    st.error = Some(e.to_string());
                }
            }
            (false, None, _) => {}
        }
        b.set_reg_state(st);
    });
}

pub fn set(app: &AppState, b: &Arc<Browser>, patch: BrowserPatch) -> ApiResult<BrowserInfo> {
    let mut relink = false;
    {
        let mut s = lock_or_recover(&app.store);
        if s.installed() && s.global_unreadable() {
            return Err(ApiError::new(crate::store::GLOBAL_UNREADABLE, "settings.json was written by a newer NiceVault (or is damaged): it is left as it is."));
        }
        if let Some(l) = patch.link {
            relink = l != s.browser_link();
            s.set_browser_link(l);
        }
        if let Some(d) = patch.default_volume {
            s.set_pm_default_volume(Some(d));
        }
    }
    if relink {
        sync_registration(b);
    }
    b.notify();
    Ok(b.info())
}

pub mod cmd {
    use super::{Answer, Browser, BrowserInfo, BrowserPatch};
    use crate::error::ApiResult;
    use crate::state::AppState;
    use std::sync::Arc;
    use tauri::State;

    type B<'a> = State<'a, Arc<Browser>>;

    #[tauri::command(rename_all = "snake_case")]
    pub async fn browser_info(b: B<'_>) -> ApiResult<BrowserInfo> {
        let b = b.inner().clone();
        tauri::async_runtime::spawn_blocking(move || Ok(b.info())).await.map_err(|e| crate::error::ApiError::new("error", e.to_string()))?
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn browser_set(st: State<'_, Arc<AppState>>, b: B<'_>, patch: BrowserPatch) -> ApiResult<BrowserInfo> {
        let (st, b) = (st.inner().clone(), b.inner().clone());
        tauri::async_runtime::spawn_blocking(move || super::set(&st, &b, patch)).await.map_err(|e| crate::error::ApiError::new("error", e.to_string()))?
    }

    #[tauri::command(rename_all = "snake_case")]
    pub fn browser_unpair(b: B<'_>, id: String) -> BrowserInfo {
        b.unpair(&id);
        b.info()
    }

    #[tauri::command(rename_all = "snake_case")]
    pub fn browser_pair_answer(b: B<'_>, id: String, answer: Answer) -> bool {
        b.pair_answer(&id, answer)
    }

    #[tauri::command(rename_all = "snake_case")]
    pub fn browser_unlock_cancel(b: B<'_>) {
        b.unlock_cancelled();
    }
}

#[cfg(all(test, unix))]
mod tests;

pub(crate) fn b64(b: &[u8]) -> String {
    B64.encode(b)
}

pub(crate) fn unb64(s: &str) -> Option<Vec<u8>> {
    B64.decode(s).ok()
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Target {
    Local { vault: String, entry: String },
    Space { space: String, entry: String },
}

#[derive(Default)]
pub(crate) struct Refs {
    by_ref: HashMap<String, Target>,
    by_target: HashMap<Target, String>,
}

impl Refs {
    pub(crate) fn get(&self, r: &str) -> Option<&Target> {
        self.by_ref.get(r)
    }
    pub(crate) fn of(&mut self, t: Target, new_ref: impl FnOnce() -> String) -> String {
        if let Some(r) = self.by_target.get(&t) {
            return r.clone();
        }
        let r = new_ref();
        self.by_ref.insert(r.clone(), t.clone());
        self.by_target.insert(t, r.clone());
        r
    }
    pub(crate) fn clear(&mut self) {
        self.by_ref.clear();
        self.by_target.clear();
    }
}
