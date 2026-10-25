//! App state and every blocking operation behind the Tauri commands. Commands (commands.rs)
//! are thin async wrappers; tests call these methods directly.

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

/// Where events go: the Tauri app in production, a recorder in tests.
pub trait Events: Send + Sync {
    fn emit(&self, event: &str, payload: serde_json::Value);
}

/// Discards events.
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
    /// Open order (higher = opened later): picks the volume `settings_*` act on without `vault`.
    pub seq: u64,
    /// This volume's idle clock (autolock.rs).
    pub activity: Arc<crate::autolock::Activity>,
    /// Cached copy of its settings (the auto-lock fields drive autolock.rs).
    pub autolock: Settings,
    /// Opened OLDER than this computer last saw it, until the user accepts it.
    pub rolled_back: Option<RolledBack>,
}

/// The volume's commit counter is behind the one this computer remembers for its id: the file was
/// replaced by an older copy of itself (a restored backup, a sync conflict,
/// or someone undoing changes on purpose). `seen` = remembered here, `now` = in the file.
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
    /// Serializes open/create/lock so the same file is never opened twice.
    lifecycle: Mutex<()>,
    open_seq: AtomicU64,
    /// "Apri con" has told this session once that its copies sit on the host's disk.
    host_copy_told: std::sync::atomic::AtomicBool,
    me: Weak<AppState>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VaultInfo {
    pub id: String,
    pub path: String,
    pub name: String,
    pub mountpoint: Option<String>,
    /// Argon2id parameters of the slot that unlocked this session (password slots only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kdf: Option<nv_core::KdfParams>,
    pub emblem: Emblem,
    /// The frame round the emblem (emblem.rs FRAMES).
    pub emblem_frame: String,
    /// = `visibility == "visible"` (kept for older callers).
    pub show_when_locked: bool,
    pub visibility: Visibility,
    /// This volume's settings (accent, theme, auto-lock): apply them while it is active.
    pub settings: Settings,
    /// The header's appearance section had been edited while locked (now rewritten).
    pub appearance_tampered: bool,
    /// The document number: 9 characters made from the header's volume id (store::volume_tag).
    pub volume_tag: String,
    /// Format v2 single-file volume (its journal lives inside the `.nvault`).
    pub single_file: bool,
    /// Older than this computer last saw it: warn, `vault_accept_rollback` to keep it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rolled_back: Option<RolledBack>,
    /// "Aggiorna formato" is available (`vault_upgrade_format`).
    pub can_upgrade_format: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct VaultStats {
    pub used_bytes: u64,
    pub file_count: u64,
    pub file_size_bytes: u64,
    pub free_bytes: u64,
    /// When the volume's file was last written (unix ms; 0 = unknown).
    pub modified_ms: u64,
}

/// `{ index, kind, label, kdf: {m_kib, t, p} | null, fingerprint, in_use }` — see types.ts.
pub use nv_core::SlotInfo;

/// WinFsp's releases page, offered when it is missing or too old.
pub const WINFSP_DOWNLOAD: &str = "https://winfsp.dev/rel/";

#[derive(Clone, Debug, Serialize)]
pub struct BackendInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub available: bool,
    /// English fallback; the UI translates `reason_code`.
    pub reason: Option<String>,
    /// fuse_missing | linux_only | nfs_admin | macos_only | winfsp_missing | winfsp_unsupported |
    /// winfsp_load_failed | windows_only
    pub reason_code: Option<&'static str>,
    /// This OS's own backend (the one whose reason the "Unità di sistema" switch shows).
    pub native: bool,
    /// Where to get what is missing (WinFsp's download page), opened via open_link.
    pub link: Option<&'static str>,
    /// Oldest supported version of what is missing (WinFsp: "2.1").
    pub min_version: Option<&'static str>,
}

/// `fs_open_external`'s answer.
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

/// Extensions Windows' ShellExecute runs as code rather than opening as a document.
const RUNS_ON_WINDOWS: &[&str] = &[
    "exe", "com", "bat", "cmd", "ps1", "psm1", "psd1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta", "msi", "msp", "mst",
    "scr", "pif", "lnk", "cpl", "reg", "jar", "url", "inf", "scf", "application", "appref-ms", "gadget", "msc", "msh",
    "xll", "settingcontent-ms", "library-ms", "search-ms", "chm", "dll", "sys", "appx", "msix", "appinstaller", "diagcab",
    "py", "pyw", "pyz", "pyc", "ws", "wsc", "sct", "ade", "adp", "mde", "accde", "xbap", "jnlp", "vb", "msixbundle", "appxbundle",
];
/// Linux desktops (xdg-open: a launcher, an installer, a script handed to an interpreter,
/// Wine for the Windows types above, Java for .jar).
const RUNS_ON_LINUX: &[&str] = &[
    "desktop", "kdesktop", "sh", "bash", "zsh", "csh", "ksh", "tcsh", "fish", "run", "bin", "appimage", "flatpak", "flatpakref",
    "flatpakrepo", "snap", "deb", "rpm", "pl", "rb", "so", "ko", "x86_64",
];
/// macOS (LaunchServices: Terminal for .command/.tool, an app bundle, a URL file, an installer,
/// AppleScript, Automator).
const RUNS_ON_MACOS: &[&str] = &[
    "app", "command", "tool", "terminal", "webloc", "inetloc", "fileloc", "pkg", "mpkg", "scpt", "scptd", "applescript", "workflow",
    "action", "prefpane", "qlgenerator", "kext", "osax", "saver", "plugin", "bundle", "dylib", "mobileconfig", "dmg", "shortcut",
];

/// A file the OS (or Wine, Java, a script interpreter) would run instead of showing, by name:
/// the lists of every OS, on every OS (a volume moves between them).
pub fn runs_code(name: &str) -> bool {
    let n = nv_fs::host_name(name).to_ascii_lowercase();
    let n = n.trim_end_matches(['.', ' ']);
    // (Windows drops trailing dots and spaces: "a.exe." runs as "a.exe")
    n.rsplit_once('.').is_some_and(|(_, e)| [RUNS_ON_WINDOWS, RUNS_ON_LINUX, RUNS_ON_MACOS].iter().any(|l| l.contains(&e)))
}

/// The first bytes of a program or script: ELF, PE ("MZ"), Mach-O (thin or fat), a `#!`
/// script, a Java archive. The desktop may go by content when the name says nothing.
pub fn sniffs_runnable(h: &[u8]) -> bool {
    const MAGIC: &[&[u8]] = &[b"\x7fELF", b"MZ", b"#!", b"\xfe\xed\xfa\xce", b"\xfe\xed\xfa\xcf", b"\xce\xfa\xed\xfe", b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca"];
    MAGIC.iter().any(|m| h.starts_with(m)) || (h.starts_with(b"PK\x03\x04") && h.windows(20).any(|w| w == b"META-INF/MANIFEST.MF"))
        || h.starts_with(b"[Desktop Entry]")
}

/// Refuses to hand a file to the OS when it would run (by name or by its first bytes).
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

    /// The open volume `id`. Counts as activity on it (idle auto-lock).
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

    /// What `info` needs from the map entry: taken with `vaults` held, the volume's own queries
    /// then made without it (they wait for that volume's store lock, which an import holds for
    /// its spills and commits; the other volumes and every `state.vault(..)` must not wait too).
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
    // ---------------- volumes ----------------

    pub fn vault_create(&self, path: &str, password: SecretString, kdf: Option<&str>) -> ApiResult<VaultInfo> {
        self.vault_create_sized(path, password, kdf, None)
    }

    /// `fixed_size`: a preallocated volume of that many bytes (it never grows; "Crea
    /// dispositivo" > "Tutta la partizione" / a fixed "Un pezzo"). None = grows as needed.
    pub fn vault_create_sized(&self, path: &str, password: SecretString, kdf: Option<&str>, fixed_size: Option<u64>) -> ApiResult<VaultInfo> {
        let path = abs_path(path)?;
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("nvault")) {
            return Err(ApiError::new("invalid_name", "Il file deve finire con .nvault."));
        }
        if secrecy::ExposeSecret::expose_secret(&password).is_empty() {
            return Err(ApiError::new("invalid_password", "Serve una password."));
        }
        let kdf = match kdf {
            Some("fast") if cfg!(debug_assertions) => KdfPreset::Fast, // tests only
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
        // New volumes start from the defaults (in the app's look when "Come l'app"), look visible while locked.
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
        // Bring the header section in line with the volume (repairs a tampered or stale one,
        // gives volumes made before it existed their visible default).
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
            // the locked screens of this computer may show its frame (a visible look only)
            store.set_frame(&o.path, info.show_when_locked.then(|| info.emblem_frame.clone()));
        }
        crate::watch::spawn_removal_watcher(self.me.clone(), o.id.clone(), o.path.clone());
        let id = o.id.clone();
        lock_or_recover(&self.vaults).insert(o.id.clone(), o);
        self.emit("vault-opened", serde_json::json!({ "vault": id }));
        info
    }

    /// Compares the volume's commit counter with this computer's anchor for its id; raises
    /// the anchor when the volume is as new or newer. An older one is reported and the anchor
    /// is left alone (it is lowered only by `vault_accept_rollback`).
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

    /// "Tieni questa versione": the user knows the volume is an older copy (they restored a
    /// backup): the anchor follows it from now on and the warning goes.
    pub fn vault_accept_rollback(&self, id: &str) -> ApiResult<VaultInfo> {
        let mut g = lock_or_recover(&self.vaults);
        let o = g.get_mut(id).ok_or_else(ApiError::locked)?;
        if let (Some(vid), Ok(now)) = (o.vault.volume_id(), o.vault.commit_seq()) {
            lock_or_recover(&self.store).set_anchor(&vid, now, true);
        }
        o.rolled_back = None;
        Ok(Self::info(o))
    }

    /// Unmount, cancel jobs, sync + wipe external copies, close the store. Idempotent.
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
        // The anchor learns how far this session took the volume (a durability point
        // first, so the counter is the one the closed file keeps). A volume reported as rolled
        // back keeps its higher anchor until the user accepts it.
        if reason != "removed" && nv_fs::Fs::flush(&*o.vault).is_ok()
            && let (Some(vid), Ok(now)) = (o.vault.volume_id(), o.vault.commit_seq())
        {
            lock_or_recover(&self.store).set_anchor(&vid, now, false);
        }
        // Jobs and preview requests hold short-lived clones; wait briefly so the store is
        // closed (WAL checkpointed, keys wiped) before we report it locked.
        // (A holder still running after the 5 s keeps the keys in memory until it finishes:
        // bounded by that job or request, and the volume is already unregistered.)
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
                Err(_) => break, // the last holder's drop closes it
            }
        }
        self.emit("vault-locked", serde_json::json!({ "vault": id, "reason": reason }));
        Ok(())
    }

    /// Locks everything (app exit, system sleep).
    pub fn lock_all(&self, reason: &str) {
        let ids: Vec<String> = lock_or_recover(&self.vaults).keys().cloned().collect();
        for id in ids {
            let _ = self.vault_lock(&id, reason);
        }
        self.external.wipe_all();
        self.clear_copied(None);
    }

    /// "Aggiorna formato": makes the volume single-file in place (nv_core::Vault::upgrade_format,
    /// crash-safe; builds before this one cannot open it afterwards). The session goes on.
    /// Growable and fixed-size volumes alike; `volume_full`: a fixed-size volume without room
    /// for the journal region inside it (extend it first). Already single-file: nothing happens.
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

    /// Whether the volume is unlocked (without counting as activity for the auto-lock).
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

    // ---------------- emblem ----------------

    pub fn vault_emblem_get(&self, id: &str) -> ApiResult<EmblemState> {
        Ok(crate::emblem::load(&*self.vault(id)?))
    }

    /// Stores the emblem inside the volume; `show_when_locked` is the appearance visibility
    /// (true = visible: the header section is rewritten, false = encrypted: it is cleared).
    /// `locked_image`: the UI's <= 24 KiB, <= 128 px copy of an image emblem for the header.
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

    /// Changes any of accent / emblem / visibility (`None` keeps it), then rewrites the
    /// header section accordingly. Unknown accents are ignored, like `settings_set`.
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
            // Only the parts given are written; a record from a newer build keeps the rest
            // (or the change is refused when it cannot be completed, see emblem::save).
            let prev = crate::emblem::stored(&v).map(|p| p.emblem);
            crate::emblem::save(&v, emblem.as_ref(), visibility.map(Visibility::shown), frame.as_deref())?;
            if let Some(e) = &emblem {
                if !matches!(e, Emblem::Image { .. }) {
                    look::save_locked_image(&v, None)?;
                } else if thumb.is_some() {
                    look::save_locked_image(&v, thumb.as_deref())?;
                } else if prev.as_ref() != Some(e) {
                    look::save_locked_image(&v, None)?; // a new image without its small copy
                }
            }
        }
        look::sync_header(&v, &name)?;
        let st = self.vault_appearance_get(id)?;
        lock_or_recover(&self.store).set_frame(v.path(), st.visibility.shown().then(|| st.emblem_frame.clone()));
        Ok(st)
    }

    // ---------------- recent / settings ----------------

    pub fn recent_list(&self) -> Vec<RecentEntry> {
        // test hook (e2e/real/reveal.mjs): a slow scan (USB sticks waking up), so Benvenuto is
        // shown well before the volumes it decodes with are known
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

    /// The volume `settings_*` act on: `vault` when given (must be open), else the most
    /// recently opened one still open, else `None` (session defaults).
    fn settings_target(&self, vault: Option<&str>) -> ApiResult<Option<(Arc<Vault>, String)>> {
        if let Some(id) = vault {
            return self.open_named(id).map(Some);
        }
        Ok(lock_or_recover(&self.vaults).values().max_by_key(|o| o.seq).map(|o| (o.vault.clone(), o.name.clone())))
    }

    /// Settings of `vault` (or see [`Self::settings_target`]).
    pub fn settings_get(&self, vault: Option<&str>) -> ApiResult<Settings> {
        Ok(match self.settings_target(vault)? {
            Some((v, _)) => look::load_settings(&v),
            None => lock_or_recover(&self.store).defaults(),
        })
    }

    /// Patches the settings of `vault` (stored inside it; an accent change also refreshes
    /// the header section when visible). With no volume open: the session defaults, which
    /// new volumes start from.
    pub fn settings_set(&self, patch: SettingsPatch, vault: Option<&str>) -> ApiResult<Settings> {
        let Some((v, name)) = self.settings_target(vault)? else { return Ok(lock_or_recover(&self.store).set_defaults(patch)) };
        let s = look::patch_settings(&v, patch)?;
        look::sync_header(&v, &name)?;
        if let Some(o) = lock_or_recover(&self.vaults).values_mut().find(|o| Arc::ptr_eq(&o.vault, &v)) {
            o.autolock = s.clone();
        }
        Ok(s)
    }

    pub fn default_volume_dir(&self) -> String {
        // Android / iOS: <app data>/Volumes (mobile.rs)
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

    // ---------------- files ----------------

    pub fn fs_list(&self, id: &str, dir: NodeId) -> ApiResult<Vec<Node>> {
        // test hook (e2e/real/reveal.mjs): a slow drive, so the loading state is seen
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

    /// Deletes on the calling thread, in committed batches (delete.rs); the `fs_delete` command
    /// runs the same as a job (`delete::fs_delete`). An item that could not go is an error here.
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

    /// Decrypts `node` into the private temp dir (watched, wiped on lock); returns the path.
    pub fn extract_external(&self, id: &str, node: NodeId) -> ApiResult<PathBuf> {
        let v = self.vault(id)?;
        self.external.extract(&self.arc(), id, &v, node)
    }

    /// One write-back pass over the external copies (the watcher thread does this every 1 s).
    pub fn poll_external(&self) {
        self.external.poll(self)
    }

    /// Opens a temp copy with the OS default app. `host_disk_notice`: the first time in a
    /// portable session where the copy is on the host's disk (not Linux's tmpfs
    /// `$XDG_RUNTIME_DIR`), the UI says so once (wiped on lock, but a crash or a locked
    /// file can leave it there, and on SSD / copy-on-write file systems an overwrite is best effort).
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

    // ---------------- mount ----------------

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
        // the native backend's precise reason (Windows: missing / too old / failed to load)
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
                let _ = h.unmount(); // locked meanwhile
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

    // ---------------- keyslots ----------------

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

    /// Re-keys a password slot after checking `old_password` against that same slot.
    /// The session already holds the master key, so the check is a guard against a
    /// walk-up change on an unlocked machine, not a cryptographic requirement.
    /// `label`: `Some` replaces the slot's label (empty clears), `None` keeps it.
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

    /// Adds a duress ("autodistruzione") password: entering it at unlock destroys the volume's
    /// keys and answers "wrong_password" (nv_core::Vault::add_duress). Removed with `slot_remove`.
    pub fn slot_add_duress(&self, id: &str, password: SecretString, label: Option<String>) -> ApiResult<Vec<SlotInfo>> {
        if secrecy::ExposeSecret::expose_secret(&password).is_empty() {
            return Err(ApiError::new("invalid_password", "Serve una password."));
        }
        self.vault(id)?.add_duress(&password, KdfPreset::Strong, label.as_deref())?;
        self.slots_list(id)
    }

    /// Re-encrypts the volume under a new master key, keeping only password slot `slot`
    /// (`password` must open it); every other slot is dropped and must be added again
    /// (nv_core::Vault::rotate_master). Slow: rewrites the whole volume, needs its size free.
    pub fn slot_rotate_key(&self, id: &str, slot: usize, password: SecretString) -> ApiResult<Vec<SlotInfo>> {
        self.vault(id)?.rotate_master(slot, &password)?;
        self.slots_list(id)
    }

    pub fn slot_remove(&self, id: &str, slot: usize) -> ApiResult<Vec<SlotInfo>> {
        self.vault(id)?.remove_slot(slot)?;
        self.slots_list(id)
    }

    // ---------------- jobs ----------------

    pub fn job_cancel(&self, job: &str) {
        if let Some(j) = lock_or_recover(&self.jobs).get(job) {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }
}
