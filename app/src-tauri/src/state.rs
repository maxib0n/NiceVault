use crate::appearance::{self as look, AppearanceState, Settings, SettingsPatch, Visibility};
use crate::emblem::{Emblem, EmblemState};
use crate::error::{ApiError, ApiResult};
use crate::external::External;
use crate::store::{RecentEntry, Store};
use nv_core::{KdfPreset, Vault};
use nv_fs::{Fs, Kind, Node, NodeId, ROOT};
use secrecy::SecretString;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

pub trait Events: Send + Sync {
    fn emit(&self, event: &str, payload: serde_json::Value);
}

pub struct NoEvents;
impl Events for NoEvents {
    fn emit(&self, _: &str, _: serde_json::Value) {}
}

pub struct OpenVault {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub vault: Arc<Vault>,
    pub mount: Option<nv_mount::MountHandle>,
    pub seq: u64,
    pub activity: Arc<crate::autolock::Activity>,
    pub autolock: Settings,
    pub rolled_back: Option<RolledBack>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct RolledBack {
    pub seen: u64,
    pub now: u64,
}

pub struct Job {
    pub vault: String,
    pub cancel: Arc<AtomicBool>,
}

pub struct AppState {
    pub(crate) vaults: Mutex<HashMap<String, OpenVault>>,
    pub(crate) jobs: Mutex<HashMap<String, Job>>,
    pub(crate) store: Mutex<Store>,
    pub(crate) external: External,
    pub(crate) events: Box<dyn Events>,
    pub(crate) last_copy: Mutex<crate::passwords::LastCopy>,
    lifecycle: Mutex<()>,
    open_seq: AtomicU64,
    host_copy_told: std::sync::atomic::AtomicBool,
    me: Weak<AppState>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VaultInfo {
    pub id: String,
    pub path: String,
    pub name: String,
    pub mountpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kdf: Option<nv_core::KdfParams>,
    pub emblem: Emblem,
    pub emblem_frame: String,
    pub show_when_locked: bool,
    pub visibility: Visibility,
    pub settings: Settings,
    pub appearance_tampered: bool,
    pub volume_tag: String,
    pub single_file: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rolled_back: Option<RolledBack>,
    pub can_upgrade_format: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct VaultStats {
    pub used_bytes: u64,
    pub file_count: u64,
    pub file_size_bytes: u64,
    pub free_bytes: u64,
    pub modified_ms: u64,
}

pub use nv_core::SlotInfo;

pub const WINFSP_DOWNLOAD: &str = "https://winfsp.dev/rel/";

#[derive(Clone, Debug, Serialize)]
pub struct BackendInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub available: bool,
    pub reason: Option<String>,
    pub reason_code: Option<&'static str>,
    pub native: bool,
    pub link: Option<&'static str>,
    pub min_version: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct OpenedExternal {
    pub host_disk_notice: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Mounted {
    pub mountpoint: String,
}

pub const LOCK_REASONS: [&str; 5] = ["user", "idle", "removed", "sleep", "screen"];

pub(crate) fn lock_or_recover<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn random_id() -> String {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).expect("OS random");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

const RUNS_ON_WINDOWS: &[&str] = &[
    "exe", "com", "bat", "cmd", "ps1", "psm1", "psd1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta", "msi", "msp", "mst",
    "scr", "pif", "lnk", "cpl", "reg", "jar", "url", "inf", "scf", "application", "appref-ms", "gadget", "msc", "msh",
    "xll", "settingcontent-ms", "library-ms", "search-ms", "chm", "dll", "sys", "appx", "msix", "appinstaller", "diagcab",
    "py", "pyw", "pyz", "pyc", "ws", "wsc", "sct", "ade", "adp", "mde", "accde", "xbap", "jnlp", "vb", "msixbundle", "appxbundle",
    "rdp", "theme", "themepack", "deskthemepack", "msu", "wsb", "ps1xml", "psc1", "msh1", "msh2", "mshxml", "mdb", "accdb",
    "cab", "iso", "img", "vhd", "vhdx",
];
const RUNS_ON_LINUX: &[&str] = &[
    "desktop", "kdesktop", "sh", "bash", "zsh", "csh", "ksh", "tcsh", "fish", "run", "bin", "appimage", "flatpak", "flatpakref",
    "flatpakrepo", "snap", "deb", "rpm", "pl", "rb", "so", "ko", "x86_64",
];
const RUNS_ON_MACOS: &[&str] = &[
    "app", "command", "tool", "terminal", "webloc", "inetloc", "fileloc", "pkg", "mpkg", "scpt", "scptd", "applescript", "workflow",
    "action", "prefpane", "qlgenerator", "kext", "osax", "saver", "plugin", "bundle", "dylib", "mobileconfig", "dmg", "shortcut",
    "afploc", "ftploc",
];

pub fn runs_code(name: &str) -> bool {
    let n = nv_fs::host_name(name).to_ascii_lowercase();
    let n = n.trim_end_matches(['.', ' ']);
    n.rsplit_once('.').is_some_and(|(_, e)| [RUNS_ON_WINDOWS, RUNS_ON_LINUX, RUNS_ON_MACOS].iter().any(|l| l.contains(&e)))
}

pub fn sniffs_runnable(h: &[u8]) -> bool {
    const MAGIC: &[&[u8]] = &[b"\x7fELF", b"MZ", b"#!", b"\xfe\xed\xfa\xce", b"\xfe\xed\xfa\xcf", b"\xce\xfa\xed\xfe", b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca"];
    MAGIC.iter().any(|m| h.starts_with(m)) || (h.starts_with(b"PK\x03\x04") && h.windows(20).any(|w| w == b"META-INF/MANIFEST.MF"))
        || [&b"[Desktop Entry]"[..], b"[KDE Desktop Entry]", b"[Desktop Action"].iter().any(|m| h.starts_with(m))
}

pub fn refuse_runnable(v: &Vault, n: &Node) -> ApiResult<()> {
    let mut head = [0u8; 512];
    let k = if n.kind == Kind::File { v.read(n.id, 0, &mut head).unwrap_or(0) } else { 0 };
    if runs_code(&n.name) || sniffs_runnable(&head[..k]) {
        return Err(ApiError::new("unsafe_type", "Programs and scripts are not opened from a volume: export the file first."));
    }
    Ok(())
}

fn file_stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Volume".into())
}

fn abs_path(p: &str) -> ApiResult<PathBuf> {
    if p.trim().is_empty() {
        return Err(ApiError::new("invalid_name", "Percorso mancante."));
    }
    std::path::absolute(p).map_err(ApiError::from)
}

impl AppState {
    pub fn new(store: Store, events: Box<dyn Events>) -> Arc<AppState> {
        Arc::new_cyclic(|me| AppState {
            vaults: Mutex::default(),
            jobs: Mutex::default(),
            store: Mutex::new(store),
            external: External::new(),
            events,
            last_copy: Mutex::default(),
            lifecycle: Mutex::new(()),
            open_seq: AtomicU64::new(0),
            host_copy_told: Default::default(),
            me: me.clone(),
        })
    }

    pub(crate) fn arc(&self) -> Arc<AppState> {
        self.me.upgrade().expect("AppState alive")
    }

    pub fn emit<T: Serialize>(&self, event: &str, payload: T) {
        if let Ok(v) = serde_json::to_value(payload) {
            self.events.emit(event, v);
        }
    }

    pub fn vault(&self, id: &str) -> ApiResult<Arc<Vault>> {
        lock_or_recover(&self.vaults)
            .get(id)
            .map(|o| {
                o.activity.touch();
                o.vault.clone()
            })
            .ok_or_else(ApiError::locked)
    }

    fn info(o: &OpenVault) -> VaultInfo {
        Self::snap(o).info()
    }

    fn snap(o: &OpenVault) -> InfoSnap {
        InfoSnap {
            id: o.id.clone(),
            path: o.path.clone(),
            name: o.name.clone(),
            mountpoint: o.mount.as_ref().map(|m| m.mountpoint().to_string_lossy().into_owned()),
            vault: o.vault.clone(),
            rolled_back: o.rolled_back,
        }
    }
}

struct InfoSnap {
    id: String,
    path: PathBuf,
    name: String,
    mountpoint: Option<String>,
    vault: Arc<Vault>,
    rolled_back: Option<RolledBack>,
}

impl InfoSnap {
    fn info(self) -> VaultInfo {
        let o = self;
        let e = crate::emblem::load(&o.vault);
        VaultInfo {
            id: o.id.clone(),
            path: o.path.to_string_lossy().into_owned(),
            name: o.name.clone(),
            mountpoint: o.mountpoint.clone(),
            kdf: o.vault.slots().into_iter().find(|s| s.in_use).and_then(|s| s.kdf),
            emblem: e.emblem,
            emblem_frame: e.emblem_frame,
            show_when_locked: e.show_when_locked,
            visibility: Visibility::from_show(e.show_when_locked),
            settings: look::load_settings(&o.vault),
            appearance_tampered: o.vault.appearance_tampered(),
            volume_tag: o.vault.volume_id().map(|id| crate::store::volume_tag(&id)).unwrap_or_default(),
            single_file: o.vault.is_single_file(),
            rolled_back: o.rolled_back,
            can_upgrade_format: o.vault.can_upgrade_format(),
        }
    }
}

impl AppState {

    pub fn vault_create(&self, path: &str, password: SecretString, kdf: Option<&str>) -> ApiResult<VaultInfo> {
        self.vault_create_sized(path, password, kdf, None)
    }

    pub fn vault_create_sized(&self, path: &str, password: SecretString, kdf: Option<&str>, fixed_size: Option<u64>) -> ApiResult<VaultInfo> {
        let path = abs_path(path)?;
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("nvault")) {
            return Err(ApiError::new("invalid_name", "Il file deve finire con .nvault."));
        }
        if secrecy::ExposeSecret::expose_secret(&password).is_empty() {
            return Err(ApiError::new("invalid_password", "Serve una password."));
        }
        let kdf = match kdf {
            Some("fast") if cfg!(debug_assertions) => KdfPreset::Fast,
            _ => KdfPreset::Strong,
        };
        let _g = lock_or_recover(&self.lifecycle);
        let t0 = Instant::now();
        if let Some(n) = fixed_size
            && let Some(dir) = path.parent()
            && fs4::available_space(dir).is_ok_and(|free| free < n)
        {
            return Err(ApiError::new("no_space", "There is not enough free space for a volume of this size."));
        }
        let made = match fixed_size {
            Some(n) => Vault::create_fixed(&path, &password, kdf, n),
            None => Vault::create(&path, &password, kdf),
        };
        let v = made.map_err(|e| match e {
            nv_fs::Error::Exists => ApiError::new("exists", "A file with this name already exists."),
            e => e.into(),
        })?;
        drop(password);
        let defaults = lock_or_recover(&self.store).new_volume_settings();
        look::save_settings(&v, &defaults)?;
        look::sync_header(&v, &file_stem(&path))?;
        Ok(self.register(path, v, t0.elapsed()))
    }

    pub fn vault_open(&self, path: &str, password: SecretString) -> ApiResult<VaultInfo> {
        let path = abs_path(path)?;
        let _g = lock_or_recover(&self.lifecycle);
        let open = lock_or_recover(&self.vaults).values().find(|o| o.path == path).map(Self::snap);
        if let Some(o) = open {
            return Ok(o.info());
        }
        if !path.is_file() {
            return Err(ApiError::new("not_found", "The volume file is not reachable."));
        }
        let t0 = Instant::now();
        let v = Vault::open(&path, &password)?;
        drop(password);
        let _ = look::sync_header(&v, &file_stem(&path));
        Ok(self.register(path, v, t0.elapsed()))
    }

    fn register(&self, path: PathBuf, v: Vault, took: Duration) -> VaultInfo {
        let seq = self.open_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let autolock = look::load_settings(&v);
        let rolled_back = self.check_fresh(&v);
        let o = OpenVault {
            id: random_id(),
            name: file_stem(&path),
            path,
            vault: Arc::new(v),
            mount: None,
            seq,
            activity: Arc::default(),
            autolock,
            rolled_back,
        };
        let info = Self::info(&o);
        {
            let mut store = lock_or_recover(&self.store);
            store.touch(&o.path, Some(took.as_millis() as u64));
            store.set_frame(&o.path, info.show_when_locked.then(|| info.emblem_frame.clone()));
        }
        crate::watch::spawn_removal_watcher(self.me.clone(), o.id.clone(), o.path.clone());
        let id = o.id.clone();
        lock_or_recover(&self.vaults).insert(o.id.clone(), o);
        self.emit("vault-opened", serde_json::json!({ "vault": id }));
        info
    }

    fn check_fresh(&self, v: &Vault) -> Option<RolledBack> {
        let id = v.volume_id()?;
        let now = v.commit_seq().ok()?;
        let mut store = lock_or_recover(&self.store);
        match store.anchor(&id) {
            Some(seen) if seen > now => Some(RolledBack { seen, now }),
            _ => {
                store.set_anchor(&id, now, false);
                None
            }
        }
    }

    pub fn vault_accept_rollback(&self, id: &str) -> ApiResult<VaultInfo> {
        let mut g = lock_or_recover(&self.vaults);
        let o = g.get_mut(id).ok_or_else(ApiError::locked)?;
        if let (Some(vid), Ok(now)) = (o.vault.volume_id(), o.vault.commit_seq()) {
            lock_or_recover(&self.store).set_anchor(&vid, now, true);
        }
        o.rolled_back = None;
        Ok(Self::info(o))
    }

    pub fn vault_lock(&self, id: &str, reason: &str) -> ApiResult<()> {
        let reason = if LOCK_REASONS.contains(&reason) { reason } else { "user" };
        let _g = lock_or_recover(&self.lifecycle);
        let Some(mut o) = lock_or_recover(&self.vaults).remove(id) else { return Ok(()) };
        for j in lock_or_recover(&self.jobs).values().filter(|j| j.vault == id) {
            j.cancel.store(true, Ordering::Relaxed);
        }
        self.external.close_vault(id, (reason != "removed").then_some(&*o.vault));
        crate::thumbs::forget_vault(id);
        self.clear_copied(Some(id));
        if let Some(m) = o.mount.take() {
            let _ = m.unmount();
        }
        if reason != "removed" && nv_fs::Fs::flush(&*o.vault).is_ok()
            && let (Some(vid), Ok(now)) = (o.vault.volume_id(), o.vault.commit_seq())
        {
            lock_or_recover(&self.store).set_anchor(&vid, now, false);
        }
        let mut arc = o.vault;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match Arc::try_unwrap(arc) {
                Ok(v) => {
                    let _ = v.close();
                    break;
                }
                Err(a) if Instant::now() < deadline => {
                    arc = a;
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
        self.emit("vault-locked", serde_json::json!({ "vault": id, "reason": reason }));
        Ok(())
    }

    pub fn lock_all(&self, reason: &str) {
        let ids: Vec<String> = lock_or_recover(&self.vaults).keys().cloned().collect();
        for id in ids {
            let _ = self.vault_lock(&id, reason);
        }
        self.external.wipe_all();
        self.clear_copied(None);
    }

    pub fn vault_upgrade_format(&self, id: &str) -> ApiResult<VaultInfo> {
        let v = self.vault(id)?;
        let full = || ApiError::new("volume_full", "The fixed-size volume is too full for the new format: extend it first.");
        if !v.is_single_file() && !v.can_upgrade_format() {
            return Err(if v.capacity().is_some() { full() } else { ApiError::new("unsupported", "This volume's format cannot be upgraded.") });
        }
        v.upgrade_format().map_err(|e| match e {
            nv_fs::Error::Io(m) if m == nv_core::UPGRADE_NO_ROOM => full(),
            e => e.into(),
        })?;
        let snap = lock_or_recover(&self.vaults).get(id).map(Self::snap);
        snap.map(InfoSnap::info).ok_or_else(ApiError::locked)
    }

    pub fn is_open(&self, id: &str) -> bool {
        lock_or_recover(&self.vaults).contains_key(id)
    }

    pub fn vault_list_open(&self) -> Vec<VaultInfo> {
        let snaps: Vec<InfoSnap> = lock_or_recover(&self.vaults).values().map(Self::snap).collect();
        let mut v: Vec<VaultInfo> = snaps.into_iter().map(InfoSnap::info).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn vault_stats(&self, id: &str) -> ApiResult<VaultStats> {
        let v = self.vault(id)?;
        let st = v.stats()?;
        let mut size = std::fs::metadata(v.path()).map(|m| m.len()).unwrap_or(0);
        let mut wal = v.path().as_os_str().to_owned();
        wal.push("-wal");
        size += std::fs::metadata(wal).map(|m| m.len()).unwrap_or(0);
        let modified_ms = std::fs::metadata(v.path())
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as u64);
        Ok(VaultStats { used_bytes: st.used_bytes, file_count: st.files, file_size_bytes: size, free_bytes: st.free_bytes, modified_ms })
    }

    pub fn vault_emblem_get(&self, id: &str) -> ApiResult<EmblemState> {
        Ok(crate::emblem::load(&*self.vault(id)?))
    }

    pub fn vault_emblem_set(
        &self,
        id: &str,
        emblem: Emblem,
        show_when_locked: bool,
        locked_image: Option<String>,
        emblem_frame: Option<String>,
    ) -> ApiResult<EmblemState> {
        let st = self.vault_appearance_set(id, None, Some(emblem), Some(Visibility::from_show(show_when_locked)), locked_image, emblem_frame)?;
        Ok(EmblemState { emblem: st.emblem, show_when_locked: st.visibility.shown(), emblem_frame: st.emblem_frame })
    }

    pub fn vault_appearance_get(&self, id: &str) -> ApiResult<AppearanceState> {
        let v = self.vault(id)?;
        let e = crate::emblem::load(&v);
        Ok(AppearanceState {
            accent: look::load_settings(&v).accent,
            emblem: e.emblem,
            emblem_frame: e.emblem_frame,
            visibility: Visibility::from_show(e.show_when_locked),
            tampered: v.appearance_tampered(),
        })
    }

    pub fn vault_appearance_set(
        &self,
        id: &str,
        accent: Option<String>,
        emblem: Option<Emblem>,
        visibility: Option<Visibility>,
        locked_image: Option<String>,
        emblem_frame: Option<String>,
    ) -> ApiResult<AppearanceState> {
        let (v, name) = self.open_named(id)?;
        let emblem = emblem.map(crate::emblem::validate).transpose()?;
        let frame = emblem_frame.as_deref().map(crate::emblem::validate_frame).transpose()?;
        let thumb = locked_image.filter(|s| !s.is_empty()).map(|b| look::validate_locked_image(&b)).transpose()?;
        if accent.is_some() {
            look::patch_settings(&v, SettingsPatch { accent, ..Default::default() })?;
        }
        if emblem.is_some() || visibility.is_some() || frame.is_some() {
            let prev = crate::emblem::stored(&v).map(|p| p.emblem);
            crate::emblem::save(&v, emblem.as_ref(), visibility.map(Visibility::shown), frame.as_deref())?;
            if let Some(e) = &emblem {
                if !matches!(e, Emblem::Image { .. }) {
                    look::save_locked_image(&v, None)?;
                } else if thumb.is_some() {
                    look::save_locked_image(&v, thumb.as_deref())?;
                } else if prev.as_ref() != Some(e) {
                    look::save_locked_image(&v, None)?;
                }
            }
        }
        look::sync_header(&v, &name)?;
        let st = self.vault_appearance_get(id)?;
        lock_or_recover(&self.store).set_frame(v.path(), st.visibility.shown().then(|| st.emblem_frame.clone()));
        Ok(st)
    }

    pub fn recent_list(&self) -> Vec<RecentEntry> {
        if cfg!(debug_assertions)
            && let Some(ms) = std::env::var("NICEVAULT_TEST_RECENTS_DELAY_MS").ok().and_then(|v| v.parse::<u64>().ok())
        {
            std::thread::sleep(std::time::Duration::from_millis(ms.min(5000)));
        }
        lock_or_recover(&self.store).recent()
    }

    pub fn recent_forget(&self, path: &str) -> ApiResult<Vec<RecentEntry>> {
        let p = abs_path(path)?;
        let mut s = lock_or_recover(&self.store);
        s.forget(&p);
        Ok(s.recent())
    }

    fn open_named(&self, id: &str) -> ApiResult<(Arc<Vault>, String)> {
        let v = self.vault(id)?;
        lock_or_recover(&self.vaults).get(id).map(|o| (v, o.name.clone())).ok_or_else(ApiError::locked)
    }

    fn settings_target(&self, vault: Option<&str>) -> ApiResult<Option<(Arc<Vault>, String)>> {
        if let Some(id) = vault {
            return self.open_named(id).map(Some);
        }
        Ok(lock_or_recover(&self.vaults).values().max_by_key(|o| o.seq).map(|o| (o.vault.clone(), o.name.clone())))
    }

    pub fn settings_get(&self, vault: Option<&str>) -> ApiResult<Settings> {
        Ok(match self.settings_target(vault)? {
            Some((v, _)) => look::load_settings(&v),
            None => lock_or_recover(&self.store).defaults(),
        })
    }

    pub fn settings_set(&self, patch: SettingsPatch, vault: Option<&str>) -> ApiResult<Settings> {
        let Some((v, name)) = self.settings_target(vault)? else { return Ok(lock_or_recover(&self.store).set_defaults(patch)) };
        let s = look::patch_settings(&v, patch)?;
        look::sync_header(&v, &name)?;
        if let Some(o) = lock_or_recover(&self.vaults).values_mut().find(|o| Arc::ptr_eq(&o.vault, &v)) {
            o.autolock = s.clone();
        }
        self.emit("vault-settings", serde_json::json!({ "name": name }));
        Ok(s)
    }

    pub fn default_volume_dir(&self) -> String {
        #[cfg(mobile)]
        if let Some(d) = std::env::var_os("NICEVAULT_VOLUME_DIR") {
            return d.to_string_lossy().into_owned();
        }
        let dir = crate::store::exe_dir()
            .filter(|d| crate::store::dir_writable(d))
            .or_else(dirs::document_dir)
            .or_else(dirs::home_dir)
            .unwrap_or_else(std::env::temp_dir);
        dir.to_string_lossy().into_owned()
    }

    pub fn fs_list(&self, id: &str, dir: NodeId) -> ApiResult<Vec<Node>> {
        if cfg!(debug_assertions)
            && let Some(ms) = std::env::var("NICEVAULT_TEST_LIST_DELAY_MS").ok().and_then(|v| v.parse::<u64>().ok())
        {
            std::thread::sleep(std::time::Duration::from_millis(ms.min(5000)));
        }
        Ok(self.vault(id)?.readdir(dir)?)
    }

    pub fn fs_mkdir(&self, id: &str, parent: NodeId, name: &str) -> ApiResult<Node> {
        let v = self.vault(id)?;
        let name = name.trim();
        if !nv_fs::valid_name(name) {
            return Err(nv_fs::Error::InvalidName.into());
        }
        let n = v.create(parent, name, Kind::Dir)?;
        v.flush()?;
        Ok(n)
    }

    pub fn fs_rename(&self, id: &str, node: NodeId, new_parent: NodeId, new_name: &str) -> ApiResult<()> {
        let v = self.vault(id)?;
        let name = new_name.trim();
        if node == ROOT || !nv_fs::valid_name(name) {
            return Err(nv_fs::Error::InvalidName.into());
        }
        let mut p = new_parent;
        loop {
            if p == node {
                return Err(ApiError::new("invalid_name", "A folder cannot go inside itself."));
            }
            if p == ROOT {
                break;
            }
            p = v.getattr(p)?.parent;
        }
        match v.lookup(new_parent, name) {
            Ok(n) if n.id != node => return Err(nv_fs::Error::Exists.into()),
            Ok(_) | Err(nv_fs::Error::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        v.rename(node, new_parent, name)?;
        v.flush()?;
        Ok(())
    }

    pub fn fs_delete(&self, id: &str, nodes: &[NodeId]) -> ApiResult<()> {
        let v = self.vault(id)?;
        if nodes.contains(&ROOT) {
            return Err(ApiError::new("invalid_name", "The root cannot be deleted."));
        }
        for &n in nodes {
            self.external.forget_node(id, n);
        }
        let out = crate::delete::delete_now(&*v, nodes)?;
        match out.errors.into_iter().next() {
            Some(e) => Err(ApiError::new(&e.code, e.message)),
            None => Ok(()),
        }
    }

    pub fn extract_external(&self, id: &str, node: NodeId) -> ApiResult<PathBuf> {
        let v = self.vault(id)?;
        self.external.extract(&self.arc(), id, &v, node)
    }

    pub fn poll_external(&self) {
        self.external.poll(self)
    }

    pub fn fs_open_external(&self, id: &str, node: NodeId) -> ApiResult<OpenedExternal> {
        {
            let v = self.vault(id)?;
            refuse_runnable(&v, &v.getattr(node)?)?;
        }
        let path = self.extract_external(id, node)?;
        open::that_detached(&path).map_err(|e| ApiError::new("open_failed", format!("Could not open the file: {e}")))?;
        let on_disk = !crate::external::copies_in_memory() && !lock_or_recover(&self.store).installed();
        Ok(OpenedExternal { host_disk_notice: on_disk && !self.host_copy_told.swap(true, Ordering::Relaxed) })
    }

    pub fn mount_available(&self) -> Vec<BackendInfo> {
        let avail = nv_mount::available();
        let linux = cfg!(target_os = "linux");
        let mac = cfg!(target_os = "macos");
        let win = cfg!(windows);
        type Why = (&'static str, String);
        let mk = |id, label, b: nv_mount::Backend, native: bool, missing: Why, other: Why| {
            let ok = avail.contains(&b);
            let why = (!ok).then_some(if native { missing } else { other });
            let winfsp = b == nv_mount::Backend::WinFsp && native && !ok;
            BackendInfo {
                id,
                label,
                available: ok,
                reason_code: why.as_ref().map(|w| w.0),
                reason: why.map(|w| w.1),
                native,
                link: winfsp.then_some(WINFSP_DOWNLOAD),
                min_version: winfsp.then_some(nv_mount::WINFSP_MIN_VERSION),
            }
        };
        let s = |code, msg: &str| (code, msg.to_owned());
        let why = nv_mount::unavailable_reason();
        let (win_why, mac_why) = if win { (why, None) } else { (None, why) };
        vec![
            mk("fuse", "FUSE", nv_mount::Backend::Fuse, linux, s("fuse_missing", "fusermount3 or /dev/fuse is missing on this computer."), s("linux_only", "Linux only.")),
            mk("nfs", "NFS", nv_mount::Backend::Nfs, mac, mac_why.unwrap_or_else(|| s("nfs_admin", "On this Mac, NFS mounting needs administrator rights.")), s("macos_only", "macOS only.")),
            mk(
                "winfsp",
                "WinFsp",
                nv_mount::Backend::WinFsp,
                win,
                win_why.unwrap_or_else(|| s("winfsp_missing", "WinFsp is not installed (winfsp.dev).")),
                s("windows_only", "Windows only."),
            ),
        ]
    }

    pub fn mount(&self, id: &str) -> ApiResult<Mounted> {
        let (v, name) = {
            let g = lock_or_recover(&self.vaults);
            let o = g.get(id).ok_or_else(ApiError::locked)?;
            if let Some(m) = &o.mount {
                return Ok(Mounted { mountpoint: m.mountpoint().to_string_lossy().into_owned() });
            }
            o.activity.touch();
            (Arc::new(crate::autolock::Touching { fs: o.vault.clone(), activity: o.activity.clone() }), o.name.clone())
        };
        log::info!("mount: requested");
        let h = nv_mount::mount(crate::fs_events::for_vault(v, self.me.clone(), id.to_owned()), nv_mount::MountOpts::new(name))
            .inspect_err(|e| log::warn!("mount: {e}"))?;
        log::info!("mount: ok");
        let mp = h.mountpoint().to_string_lossy().into_owned();
        let mut g = lock_or_recover(&self.vaults);
        match g.get_mut(id) {
            Some(o) => o.mount = Some(h),
            None => {
                drop(g);
                let _ = h.unmount();
                return Err(ApiError::locked());
            }
        }
        Ok(Mounted { mountpoint: mp })
    }

    pub fn unmount(&self, id: &str) -> ApiResult<()> {
        let h = {
            let mut g = lock_or_recover(&self.vaults);
            g.get_mut(id).ok_or_else(ApiError::locked)?.mount.take()
        };
        if let Some(h) = h {
            h.unmount()?;
        }
        Ok(())
    }

    pub fn slots_list(&self, id: &str) -> ApiResult<Vec<SlotInfo>> {
        let s = self.vault(id)?.slots();
        if s.is_empty() {
            return Err(ApiError::locked());
        }
        Ok(s)
    }

    pub fn slot_add_password(&self, id: &str, password: SecretString, label: Option<String>) -> ApiResult<Vec<SlotInfo>> {
        if secrecy::ExposeSecret::expose_secret(&password).is_empty() {
            return Err(ApiError::new("invalid_password", "Serve una password."));
        }
        self.vault(id)?.add_password(&password, KdfPreset::Strong, label.as_deref())?;
        self.slots_list(id)
    }

    pub fn slot_change_password(
        &self,
        id: &str,
        slot: usize,
        old_password: SecretString,
        new_password: SecretString,
        label: Option<String>,
    ) -> ApiResult<Vec<SlotInfo>> {
        if secrecy::ExposeSecret::expose_secret(&new_password).is_empty() {
            return Err(ApiError::new("invalid_password", "Serve una password."));
        }
        let v = self.vault(id)?;
        match v.slots().iter().find(|s| s.index == slot) {
            None => return Err(nv_core::SlotError::NotFound.into()),
            Some(s) if s.kind != "password" => return Err(nv_core::SlotError::NotPassword.into()),
            Some(_) => {}
        }
        if !v.verify_password(slot, &old_password) {
            return Err(ApiError::new("wrong_password", "The current password is not correct."));
        }
        drop(old_password);
        v.change_password(slot, &new_password, KdfPreset::Strong, label.as_deref())?;
        self.slots_list(id)
    }

    pub fn slot_add_duress(&self, id: &str, password: SecretString, label: Option<String>) -> ApiResult<Vec<SlotInfo>> {
        if secrecy::ExposeSecret::expose_secret(&password).is_empty() {
            return Err(ApiError::new("invalid_password", "Serve una password."));
        }
        self.vault(id)?.add_duress(&password, KdfPreset::Strong, label.as_deref())?;
        self.slots_list(id)
    }

    pub fn slot_rotate_key(&self, id: &str, slot: usize, password: SecretString) -> ApiResult<Vec<SlotInfo>> {
        self.vault(id)?.rotate_master(slot, &password)?;
        self.slots_list(id)
    }

    pub fn slot_remove(&self, id: &str, slot: usize) -> ApiResult<Vec<SlotInfo>> {
        self.vault(id)?.remove_slot(slot)?;
        self.slots_list(id)
    }

    pub fn job_cancel(&self, job: &str) {
        if let Some(j) = lock_or_recover(&self.jobs).get(job) {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }
}
