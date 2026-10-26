//! Benvenuto's volume list. Nothing is written on the stick: the list is a scan of
//!   (a) the executable's folder (plus one level of subfolders) and
//!   (b) the root of every mounted removable drive,
//! each `*.nvault` enriched from its header (`nv_core::peek`: appearance when visible, KDF
//! cost -> unlock estimate). Only when running installed (the executable is not on a writable
//! removable drive) a known-volumes list in the OS config dir remembers volumes opened
//! elsewhere (`known.json`: path, last opened, measured unlock time — no names, no emblems).
//! Volumes opened during this session are listed until the app quits either way.
//!
//! Test / e2e overrides (environment):
//!   NICEVAULT_EXE_DIR          folder scanned as (a) instead of the executable's
//!   NICEVAULT_REMOVABLE_ROOTS  drive roots scanned as (b), joined like PATH (empty = none)
//!   NICEVAULT_CONFIG_DIR       folder of known.json instead of the OS config dir
//!   NICEVAULT_PORTABLE         1 = portable (no known list), 0 = installed
//!
//! Global (not per-volume) settings — the UI language, the app's own appearance (theme, ink,
//! interface size) and the defaults for NEW volumes — follow the same rule: installed mode keeps them in `settings.json` in the OS config dir;
//! portable mode keeps them in memory for this session only (nothing on the stick).

pub use crate::appearance::{ACCENTS, Settings, SettingsPatch};
use crate::emblem::Emblem;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

const KNOWN_FILE: &str = "known.json";
/// Installed mode only: `{ language, defaults }` (global settings, see app::global).
const GLOBAL_FILE: &str = "settings.json";
/// UI languages: "system" follows the OS locale.
pub const LANGUAGES: [&str; 3] = ["system", "it", "en"];
/// Interface sizes (percent of the webview's natural zoom), Impostazioni > Aspetto dell'app.
pub const UI_SCALES: [u32; 5] = [90, 100, 110, 125, 150];
/// What ←/→ do in a full-screen video: seek inside it (default) or step to the previous / next file.
pub const VIDEO_ARROWS: [&str; 2] = ["seek", "step"];
/// The old per-stick file (recents + settings next to the exe). Imported once, then deleted.
pub const LEGACY_FILE: &str = "NiceVault.json";
const MAX_KNOWN: usize = 50;
/// Freshness anchors kept (one per volume id; the oldest ids go first beyond it).
const MAX_ANCHORS: usize = 256;
/// Directory entries looked at per folder (a folder full of photos must not stall Benvenuto).
const MAX_DIR_ENTRIES: usize = 4096;

#[derive(Clone, Debug, Serialize)]
pub struct RecentEntry {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    /// Unix ms: when this app last opened it (session / known list), else the file's mtime.
    pub last_opened: u64,
    pub exists: bool,
    /// Measured at the last unlock here, else estimated from the header's KDF parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlock_ms: Option<u64>,
    /// From the header's appearance section; `None` = not known (show the initial) or hidden.
    pub emblem: Option<Emblem>,
    /// `false` = the volume keeps its look encrypted: neutral rosette, default accent.
    pub show_when_locked: bool,
    /// Accent preset id from the header (visible volumes only).
    pub accent: Option<String>,
    /// "here" (next to the app) | "removable" (root of a removable drive) | "known" (installed-mode list) | "session".
    pub source: &'static str,
    /// The document number: 9 characters from the header's volume id ([`volume_tag`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_tag: Option<String>,
    /// Occupied key slots (header, plaintext).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_slots: Option<u32>,
    /// Argon2id parameters of the first password slot (header, plaintext).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kdf: Option<nv_core::KdfParams>,
    /// The emblem's frame as this computer last saw it unlocked (the frame lives inside the
    /// volume): only for a visible look; `None` = not known here (the default rosette).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emblem_frame: Option<String>,
}

fn hex_id(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

/// A volume's document number: the first 45 bits of its header id in Crockford's base32
/// (0-9, A-Z without I L O U), 9 characters, the alphabet an MRZ can print.
pub fn volume_tag(id: &[u8; 16]) -> String {
    const A: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut v: u64 = 0;
    for b in &id[..6] {
        v = (v << 8) | u64::from(*b);
    }
    let v = v >> 3; // 48 -> 45 bits
    (0..9).rev().map(|i| A[((v >> (i * 5)) & 31) as usize] as char).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct KnownRec {
    path: String,
    last_opened: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unlock_ms: Option<u64>,
    /// The emblem's frame, recorded when the volume was open here (a visible look only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frame: Option<String>,
    /// Keys of a newer build, kept when the list is rewritten.
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct KnownFile {
    #[serde(default)]
    volumes: Vec<KnownRec>,
    /// Freshness anchors: hex volume id -> the highest commit counter
    /// (`Vault::commit_seq`) seen here. A volume that opens with a smaller one was replaced by
    /// an older copy of itself. Kept in memory only in portable mode (this session).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    anchors: BTreeMap<String, u64>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

/// Where to look. `removable: None` = detect the mounted removable drives at every scan.
#[derive(Clone, Debug, Default)]
pub struct Locations {
    pub exe_dir: Option<PathBuf>,
    pub removable: Option<Vec<PathBuf>>,
    /// Folder of known.json; `None` = portable (no list persisted anywhere).
    pub config_dir: Option<PathBuf>,
}

impl Locations {
    /// The real machine (with the env overrides above).
    pub fn detect() -> Locations {
        let env = |k: &str| std::env::var_os(k);
        let exe_dir = env("NICEVAULT_EXE_DIR").map(PathBuf::from).or_else(exe_dir);
        let removable = env("NICEVAULT_REMOVABLE_ROOTS").map(|v| std::env::split_paths(&v).filter(|p| !p.as_os_str().is_empty()).collect());
        let portable = match std::env::var("NICEVAULT_PORTABLE").ok().as_deref() {
            Some("1") => true,
            Some("0") => false,
            _ => exe_dir.as_deref().is_some_and(|d| {
                let roots = removable.clone().unwrap_or_else(removable_roots);
                on_removable(d, &roots) && dir_writable(d)
            }),
        };
        // No config dir: portable (never a shared, predictable temp folder).
        let config_dir = (!portable)
            .then(|| env("NICEVAULT_CONFIG_DIR").map(PathBuf::from).or_else(|| dirs::config_dir().map(|d| d.join("NiceVault"))))
            .flatten();
        Locations { exe_dir, removable, config_dir }
    }
}

pub struct Store {
    loc: Locations,
    known: KnownFile,
    /// Opened in this session: (path, unix ms, unlock ms).
    session: Vec<KnownRec>,
    /// `recent_forget` of a scanned volume: hidden until the app quits (or it is opened again).
    hidden: HashSet<PathBuf>,
    /// Defaults for new volumes (and the settings used while no volume is open).
    defaults: Settings,
    /// UI language: one of [`LANGUAGES`].
    language: String,
    /// Password manager: fetch site icons from the entries' websites (default on).
    site_icons: bool,
    /// Subtle UI sounds (default on; played by the webview, nothing is written for them).
    sounds: bool,
    /// ←/→ in a full-screen video: "seek" | "step".
    video_arrows: String,
    /// The volume's sidebar folded to a slim rail (default off).
    sidebar_collapsed: bool,
    /// The app's own look, worn wherever no volume's look applies (Benvenuto, Impostazioni, the
    /// wizard, Sblocca of a volume whose look is encrypted): one of `THEMES`, one of [`ACCENTS`].
    app_theme: String,
    app_accent: String,
    /// One of [`UI_SCALES`].
    ui_scale: u32,
    /// New volumes start with the app's theme and ink (instead of `defaults.theme / accent`).
    new_like_app: bool,
    /// settings.json as read (installed mode): written back with only the known keys patched,
    /// so keys a newer build added survive.
    global_raw: serde_json::Map<String, serde_json::Value>,
    /// settings.json exists but could not be read as an object (newer or damaged): never overwritten.
    global_unreadable: bool,
    /// "Avvia all'accesso" / "Avvia ridotto" (installed mode only; portable: always off).
    start_at_login: bool,
    start_minimized: bool,
    /// "Resta nel vassoio": closing the window hides it (tray icon) instead of quitting.
    /// Installed: default on; portable: always off.
    close_to_tray: bool,
    /// Phones: "Salva foto e video nella Galleria" for Esporta (None = not asked yet: the UI asks once).
    gallery_save: Option<bool>,
    /// Where the start-at-login entry goes: the real one from `open_default`, a temp one in
    /// tests (`with_autostart`), none = the setting is kept but no entry is written.
    autostart: Option<crate::autostart::Autostart>,
    /// More folders scanned like the executable's (`NICEVAULT_SCAN_DIRS`, joined like PATH; mobile:
    /// the app's Volumes and Documents folders, so a volume there is listed even when the known
    /// list lost track of it).
    scan_dirs: Vec<PathBuf>,
}

/// `unsupported_version`: settings.json is there but this build cannot read it, so it is left alone.
pub const GLOBAL_UNREADABLE: &str = "unsupported_version";

pub fn exe_dir() -> Option<PathBuf> {
    // AppImage: the folder of the .AppImage (a stick), not the read-only mount it runs from
    crate::updater::app_exe()?.parent().map(Path::to_path_buf)
}

/// Atomic, private write of a config file: dir 0700, a fresh randomly named 0600 temp file,
/// fsync, rename over `file`, fsync the dir.
pub fn write_private(file: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = file.parent().ok_or_else(|| std::io::Error::other("no parent"))?;
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        b.mode(0o700);
        b.create(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    b.create(dir)?;
    let name = file.file_name().ok_or_else(|| std::io::Error::other("no file name"))?.to_string_lossy();
    let tmp = dir.join(format!(".{name}.{}.tmp", crate::state::random_id()));
    let mut o = std::fs::File::options();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let r = (|| {
        let mut f = o.open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, file)?;
        #[cfg(unix)]
        std::fs::File::open(dir)?.sync_all()?;
        Ok(())
    })();
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

pub fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".nicevault-probe-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

impl Store {
    /// The real machine; imports and deletes a legacy NiceVault.json once.
    pub fn open_default() -> Store {
        let loc = Locations::detect();
        let autostart = loc.config_dir.is_some().then(crate::autostart::Autostart::detect).flatten();
        let scan = std::env::var_os("NICEVAULT_SCAN_DIRS").map(|v| std::env::split_paths(&v).filter(|p| !p.as_os_str().is_empty()).collect()).unwrap_or_default();
        let mut s = Store::new(loc).with_autostart(autostart).with_scan_dirs(scan);
        s.migrate_legacy();
        // iOS: an update moves the app's container; volumes recorded under the old one follow
        #[cfg(target_os = "ios")]
        if let Some(home) = std::env::var_os("HOME") {
            s.relocate_known(|p| crate::mobile_paths::rebase_container(p, Path::new(&home)));
        }
        s.startup_check();
        s
    }

    pub fn new(loc: Locations) -> Store {
        let known = loc
            .config_dir
            .as_ref()
            .and_then(|d| std::fs::read(d.join(KNOWN_FILE)).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut defaults = Settings::default();
        let mut language = LANGUAGES[0].to_owned();
        let mut site_icons = true;
        let mut sounds = true;
        let mut video_arrows = VIDEO_ARROWS[0].to_owned();
        let mut sidebar_collapsed = false;
        let mut app_theme = crate::appearance::DEFAULT_THEME.to_owned();
        let mut app_accent = ACCENTS[0].to_owned();
        let mut ui_scale = 100;
        let mut new_like_app = true;
        let mut global_raw = serde_json::Map::new();
        let mut global_unreadable = false;
        let (mut start_at_login, mut start_minimized) = (false, false);
        let mut close_to_tray = loc.config_dir.is_some();
        let mut gallery_save = None;
        if let Some(bytes) = loc.config_dir.as_ref().and_then(|d| std::fs::read(d.join(GLOBAL_FILE)).ok()) {
            match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(serde_json::Value::Object(map)) => {
                    // field by field, through apply: unknown values from a newer / edited file are ignored
                    if let Some(d) = map.get("defaults").cloned().and_then(|v| serde_json::from_value::<SettingsPatch>(v).ok()) {
                        defaults.apply(d);
                    }
                    if let Some(l) = map.get("language").and_then(|v| v.as_str()).filter(|l| LANGUAGES.contains(l)) {
                        language = l.to_owned();
                    }
                    if let Some(b) = map.get("site_icons").and_then(|v| v.as_bool()) {
                        site_icons = b;
                    }
                    if let Some(b) = map.get("sounds").and_then(|v| v.as_bool()) {
                        sounds = b;
                    }
                    if let Some(v) = map.get("video_arrows").and_then(|v| v.as_str()).filter(|v| VIDEO_ARROWS.contains(v)) {
                        video_arrows = v.to_owned();
                    }
                    if let Some(b) = map.get("sidebar_collapsed").and_then(|v| v.as_bool()) {
                        sidebar_collapsed = b;
                    }
                    let str_in = |k: &str, ok: &[&str]| map.get(k).and_then(|v| v.as_str()).filter(|v| ok.contains(v)).map(str::to_owned);
                    if let Some(t) = str_in("app_theme", &crate::appearance::THEMES) {
                        app_theme = t;
                    }
                    if let Some(a) = map.get("app_accent").and_then(|v| v.as_str()).map(crate::appearance::migrate_accent).filter(|a| ACCENTS.contains(a)) {
                        app_accent = a.to_owned();
                    }
                    if let Some(z) = map.get("ui_scale").and_then(|v| v.as_u64()).and_then(|z| u32::try_from(z).ok()).filter(|z| UI_SCALES.contains(z)) {
                        ui_scale = z;
                    }
                    if let Some(b) = map.get("new_like_app").and_then(|v| v.as_bool()) {
                        new_like_app = b;
                    }
                    if let Some(b) = map.get("start_at_login").and_then(|v| v.as_bool()) {
                        start_at_login = b;
                    }
                    if let Some(b) = map.get("start_minimized").and_then(|v| v.as_bool()) {
                        start_minimized = b;
                    }
                    if let Some(b) = map.get("close_to_tray").and_then(|v| v.as_bool()) {
                        close_to_tray = b;
                    }
                    gallery_save = map.get("gallery_save").and_then(|v| v.as_bool());
                    global_raw = map;
                }
                _ => global_unreadable = true,
            }
        }
        Store {
            loc,
            known,
            session: vec![],
            hidden: HashSet::new(),
            defaults,
            language,
            app_theme,
            app_accent,
            ui_scale,
            new_like_app,
            site_icons,
            sounds,
            video_arrows,
            sidebar_collapsed,
            global_raw,
            global_unreadable,
            start_at_login,
            start_minimized,
            close_to_tray,
            gallery_save,
            autostart: None,
            scan_dirs: Vec::new(),
        }
    }

    /// Where "Avvia all'accesso" writes its entry (see `autostart.rs`).
    pub fn with_autostart(mut self, a: Option<crate::autostart::Autostart>) -> Store {
        self.autostart = a;
        self
    }

    /// Installed mode: writes `settings.json` (language + new-volume defaults), patching only the
    /// keys this build knows. Portable: nothing. An unreadable file is never overwritten.
    pub fn with_scan_dirs(mut self, dirs: Vec<PathBuf>) -> Store {
        self.scan_dirs = dirs;
        self
    }

    /// Known volumes whose file is gone but which `to` maps to a file that exists (iOS: the same
    /// file in the app's new container) are recorded under the new path, keeping their history.
    /// Only the list changes (written atomically); no volume file is moved or deleted.
    pub fn relocate_known(&mut self, to: impl Fn(&Path) -> Option<PathBuf>) {
        let mut changed = false;
        let mut moved: Vec<KnownRec> = Vec::new();
        self.known.volumes.retain(|r| {
            let p = Path::new(&r.path);
            if p.exists() {
                return true;
            }
            match to(p).filter(|n| n.is_file()) {
                Some(n) => {
                    moved.push(KnownRec { path: n.to_string_lossy().into_owned(), ..r.clone() });
                    changed = true;
                    false
                }
                None => true,
            }
        });
        for m in moved {
            match self.known.volumes.iter_mut().find(|r| r.path == m.path) {
                Some(r) => {
                    r.last_opened = r.last_opened.max(m.last_opened);
                    r.unlock_ms = r.unlock_ms.or(m.unlock_ms);
                    r.frame = r.frame.take().or(m.frame);
                }
                None => self.known.volumes.push(m),
            }
        }
        if changed {
            self.known.volumes.sort_by_key(|r| std::cmp::Reverse(r.last_opened));
            self.save_known();
        }
    }

    fn save_global(&mut self) {
        let Some(dir) = self.loc.config_dir.clone() else { return };
        if self.global_unreadable {
            return;
        }
        self.global_raw.insert("language".into(), serde_json::Value::String(self.language.clone()));
        self.global_raw.insert("site_icons".into(), serde_json::Value::Bool(self.site_icons));
        self.global_raw.insert("sounds".into(), serde_json::Value::Bool(self.sounds));
        self.global_raw.insert("video_arrows".into(), serde_json::Value::String(self.video_arrows.clone()));
        self.global_raw.insert("sidebar_collapsed".into(), serde_json::Value::Bool(self.sidebar_collapsed));
        self.global_raw.insert("app_theme".into(), serde_json::Value::String(self.app_theme.clone()));
        self.global_raw.insert("app_accent".into(), serde_json::Value::String(self.app_accent.clone()));
        self.global_raw.insert("ui_scale".into(), serde_json::Value::from(self.ui_scale));
        self.global_raw.insert("new_like_app".into(), serde_json::Value::Bool(self.new_like_app));
        self.global_raw.insert("start_at_login".into(), serde_json::Value::Bool(self.start_at_login));
        self.global_raw.insert("start_minimized".into(), serde_json::Value::Bool(self.start_minimized));
        self.global_raw.insert("close_to_tray".into(), serde_json::Value::Bool(self.close_to_tray));
        if let Some(b) = self.gallery_save {
            self.global_raw.insert("gallery_save".into(), serde_json::Value::Bool(b));
        }
        let mut d = match self.global_raw.remove("defaults") {
            Some(serde_json::Value::Object(o)) => o,
            _ => serde_json::Map::new(),
        };
        if let Ok(serde_json::Value::Object(known)) = serde_json::to_value(&self.defaults) {
            d.extend(known);
        }
        self.global_raw.insert("defaults".into(), serde_json::Value::Object(d));
        let Ok(json) = serde_json::to_vec_pretty(&self.global_raw) else { return };
        let _ = write_private(&dir.join(GLOBAL_FILE), &json);
    }

    pub fn language(&self) -> String {
        self.language.clone()
    }

    /// Password manager: fetch site icons (global setting, default on).
    pub fn site_icons(&self) -> bool {
        self.site_icons
    }

    pub fn set_site_icons(&mut self, on: bool) {
        if on != self.site_icons {
            self.site_icons = on;
            self.save_global();
        }
    }

    /// Subtle UI sounds (global setting, default on).
    pub fn sounds(&self) -> bool {
        self.sounds
    }

    pub fn set_sounds(&mut self, on: bool) {
        if on != self.sounds {
            self.sounds = on;
            self.save_global();
        }
    }

    /// ←/→ in a full-screen video: "seek" (default) or "step" (global setting).
    pub fn video_arrows(&self) -> &str {
        &self.video_arrows
    }

    pub fn set_video_arrows(&mut self, v: &str) {
        if VIDEO_ARROWS.contains(&v) && v != self.video_arrows {
            self.video_arrows = v.to_owned();
            self.save_global();
        }
    }

    /// The volume's sidebar folded to a slim rail (global setting, default off).
    pub fn sidebar_collapsed(&self) -> bool {
        self.sidebar_collapsed
    }

    pub fn set_sidebar_collapsed(&mut self, on: bool) {
        if on != self.sidebar_collapsed {
            self.sidebar_collapsed = on;
            self.save_global();
        }
    }

    /// settings.json exists but cannot be read by this build (it is left untouched).
    pub fn global_unreadable(&self) -> bool {
        self.global_unreadable
    }

    /// Unknown values are ignored.
    pub fn set_language(&mut self, l: &str) -> String {
        if LANGUAGES.contains(&l) && l != self.language {
            self.language = l.to_owned();
            self.save_global();
        }
        self.language()
    }

    /// The app's own theme / ink / interface size, and whether new volumes start with that look.
    pub fn app_look(&self) -> (String, String, u32, bool) {
        (self.app_theme.clone(), self.app_accent.clone(), self.ui_scale, self.new_like_app)
    }

    /// Unknown values are ignored (the current one stays).
    pub fn set_app_look(&mut self, theme: Option<&str>, accent: Option<&str>, scale: Option<u32>, new_like_app: Option<bool>) {
        if let Some(t) = theme.filter(|t| crate::appearance::THEMES.contains(t)) {
            self.app_theme = t.to_owned();
        }
        if let Some(a) = accent.map(crate::appearance::migrate_accent).filter(|a| ACCENTS.contains(a)) {
            self.app_accent = a.to_owned();
        }
        if let Some(z) = scale.filter(|z| UI_SCALES.contains(z)) {
            self.ui_scale = z;
        }
        if let Some(b) = new_like_app {
            self.new_like_app = b;
        }
        self.save_global();
    }

    /// "Avvia all'accesso", "Avvia ridotto". Portable: both off.
    pub fn startup(&self) -> (bool, bool) {
        (self.start_at_login, self.start_minimized)
    }

    /// Changes them; the start-at-login entry is written (or removed) first, so a failure
    /// leaves both the entry and the setting as they were. Portable: ignored.
    pub fn set_startup(&mut self, at_login: Option<bool>, minimized: Option<bool>) -> std::io::Result<()> {
        if !self.installed() {
            return Ok(());
        }
        let (a, m) = (at_login.unwrap_or(self.start_at_login), minimized.unwrap_or(self.start_minimized));
        if (a, m) == (self.start_at_login, self.start_minimized) {
            return Ok(());
        }
        if let Some(t) = &self.autostart
            && (a || self.start_at_login)
        {
            t.apply(a, m)?;
        }
        (self.start_at_login, self.start_minimized) = (a, m);
        self.save_global();
        Ok(())
    }

    /// "Resta nel vassoio quando chiudi la finestra" (installed; portable: always off).
    /// Phones: exported photos / videos go to the gallery (None = ask once).
    pub fn gallery_save(&self) -> Option<bool> {
        self.gallery_save
    }

    pub fn set_gallery_save(&mut self, on: bool) {
        if self.gallery_save != Some(on) {
            self.gallery_save = Some(on);
            self.save_global();
        }
    }

    pub fn close_to_tray(&self) -> bool {
        self.installed() && self.close_to_tray
    }

    /// Changes it (installed only; portable: ignored).
    pub fn set_close_to_tray(&mut self, on: bool) {
        if !self.installed() || on == self.close_to_tray {
            return;
        }
        self.close_to_tray = on;
        self.save_global();
    }

    /// At start: "Avvia all'accesso" on and its entry still there -> rewritten (the binary may
    /// have moved, the flag follows "Avvia ridotto"); the entry removed outside the app (the OS's
    /// startup settings) -> the setting follows and turns off.
    pub fn startup_check(&mut self) {
        let Some(t) = self.autostart.clone() else { return };
        if !self.start_at_login || self.global_unreadable {
            return;
        }
        if t.exists() {
            let _ = t.apply(true, self.start_minimized);
        } else {
            self.start_at_login = false;
            self.save_global();
        }
    }

    /// What a volume created now starts with: the defaults, wearing the app's theme and ink
    /// when "Come l'app" is on.
    pub fn new_volume_settings(&self) -> Settings {
        let mut s = self.defaults();
        if self.new_like_app {
            s.theme = self.app_theme.clone();
            s.accent = self.app_accent.clone();
        }
        s
    }

    /// Installed mode: the known-volumes list is kept in the OS config dir.
    pub fn installed(&self) -> bool {
        self.loc.config_dir.is_some()
    }

    pub fn locations(&self) -> &Locations {
        &self.loc
    }

    fn save_known(&self) {
        let Some(dir) = &self.loc.config_dir else { return };
        let file = dir.join(KNOWN_FILE);
        // A list this build cannot read (a newer build's, or damaged) is not replaced by ours.
        if std::fs::read(&file).is_ok_and(|b| serde_json::from_slice::<KnownFile>(&b).is_err()) {
            return;
        }
        let Ok(json) = serde_json::to_vec_pretty(&self.known) else { return };
        let _ = write_private(&file, &json);
    }

    /// Imports the useful part of an old NiceVault.json (next to the exe, or in the config
    /// dir where it went when the exe's folder was read-only): volume paths (installed: into the known list; portable: for
    /// this session) and the settings (session defaults). Then deletes it.
    pub fn migrate_legacy(&mut self) {
        #[derive(Deserialize)]
        struct OldRec {
            path: String,
            #[serde(default)]
            last_opened: u64,
            #[serde(default)]
            unlock_ms: Option<u64>,
        }
        #[derive(Deserialize)]
        struct Old {
            #[serde(default)]
            recent: Vec<OldRec>,
            #[serde(default)]
            settings: Option<serde_json::Value>,
        }
        let mut files: Vec<(PathBuf, Option<PathBuf>)> = vec![];
        if let Some(d) = &self.loc.exe_dir {
            files.push((d.join(LEGACY_FILE), Some(d.clone())));
        }
        if let Some(d) = &self.loc.config_dir {
            files.push((d.join(LEGACY_FILE), None));
        }
        files.dedup_by(|a, b| a.0 == b.0);
        for (file, base) in files {
            let Ok(raw) = std::fs::read(&file) else { continue };
            if let Ok(old) = serde_json::from_slice::<Old>(&raw) {
                for r in old.recent {
                    let p = PathBuf::from(&r.path);
                    let abs = match &base {
                        Some(b) if p.is_relative() => normalize(&b.join(p)),
                        _ => p,
                    };
                    if !abs.is_absolute() {
                        continue;
                    }
                    let rec = KnownRec { path: abs.to_string_lossy().into_owned(), last_opened: r.last_opened, unlock_ms: r.unlock_ms, frame: None, extra: Default::default() };
                    let list = if self.installed() { &mut self.known.volumes } else { &mut self.session };
                    if !list.iter().any(|k| k.path == rec.path) {
                        list.push(rec);
                    }
                }
                if let Some(s) = old.settings.and_then(|v| serde_json::from_value::<Settings>(v).ok()) {
                    let mut d = Settings::default();
                    d.apply(SettingsPatch { auto_lock_minutes: Some(s.auto_lock_minutes), accent: Some(s.accent), ..Default::default() });
                    self.defaults = d;
                }
            }
            let _ = std::fs::remove_file(&file);
        }
        self.known.volumes.sort_by_key(|k| std::cmp::Reverse(k.last_opened));
        self.known.volumes.truncate(MAX_KNOWN);
        self.save_known();
    }

    pub fn defaults(&self) -> Settings {
        self.defaults.clone()
    }

    pub fn set_defaults(&mut self, patch: SettingsPatch) -> Settings {
        self.defaults.apply(patch);
        self.save_global();
        self.defaults()
    }

    /// A volume was opened or created here.
    pub fn touch(&mut self, abs: &Path, unlock_ms: Option<u64>) {
        let rec = KnownRec { path: abs.to_string_lossy().into_owned(), last_opened: now_ms(), unlock_ms, frame: None, extra: Default::default() };
        self.hidden.remove(abs);
        let up = |list: &mut Vec<KnownRec>| {
            let old = list.iter().position(|r| r.path == rec.path).map(|i| list.remove(i));
            let mut r = rec.clone();
            if let Some(o) = old {
                r.unlock_ms = r.unlock_ms.or(o.unlock_ms);
                r.frame = o.frame;
                r.extra = o.extra;
            }
            list.insert(0, r);
        };
        up(&mut self.session);
        if self.installed() {
            up(&mut self.known.volumes);
            self.known.volumes.truncate(MAX_KNOWN);
            self.save_known();
        }
    }

    /// Records the emblem's frame of a volume opened here (`None`: its look is encrypted, or the
    /// default): what the locked screens of this computer show until it is opened again.
    pub fn set_frame(&mut self, abs: &Path, frame: Option<String>) {
        let key = abs.to_string_lossy();
        let frame = frame.filter(|f| f != crate::emblem::FRAMES[0]);
        for r in self.session.iter_mut().filter(|r| r.path == key) {
            r.frame = frame.clone();
        }
        if self.installed() {
            let mut changed = false;
            for r in self.known.volumes.iter_mut().filter(|r| r.path == key) {
                changed |= r.frame != frame;
                r.frame = frame.clone();
            }
            if changed {
                self.save_known();
            }
        }
    }

    /// The freshness anchor of a volume: the highest commit counter seen here.
    pub fn anchor(&self, id: &[u8; 16]) -> Option<u64> {
        self.known.anchors.get(&hex_id(id)).copied()
    }

    /// Raises the anchor to `seq` (never lowers it), or with `reset` sets it to `seq` (the user
    /// said the older copy is the one to keep). Saved at once in installed mode.
    pub fn set_anchor(&mut self, id: &[u8; 16], seq: u64, reset: bool) {
        let k = hex_id(id);
        let old = self.known.anchors.get(&k).copied();
        let new = if reset { seq } else { old.map_or(seq, |o| o.max(seq)) };
        if old == Some(new) {
            return;
        }
        self.known.anchors.insert(k.clone(), new);
        // over the cap: drop some other id (a rare case: hundreds of volumes opened here)
        while self.known.anchors.len() > MAX_ANCHORS {
            let Some(drop) = self.known.anchors.keys().find(|x| **x != k).cloned() else { break };
            self.known.anchors.remove(&drop);
        }
        if self.installed() {
            self.save_known();
        }
    }

    /// Drops `abs` from the known list and hides it for the rest of the session.
    pub fn forget(&mut self, abs: &Path) {
        let key = abs.to_string_lossy();
        self.session.retain(|r| r.path != key);
        let before = self.known.volumes.len();
        self.known.volumes.retain(|r| r.path != key);
        if self.known.volumes.len() != before {
            self.save_known();
        }
        self.hidden.insert(abs.to_path_buf());
    }

    fn removable(&self) -> Vec<PathBuf> {
        self.loc.removable.clone().unwrap_or_else(removable_roots)
    }

    /// Scan + known list + session, newest first. Never writes anything.
    pub fn recent(&self) -> Vec<RecentEntry> {
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut out: Vec<RecentEntry> = vec![];
        let recorded = |p: &Path| {
            let k = p.to_string_lossy();
            let a = self.session.iter().find(|r| r.path == k);
            let b = self.known.volumes.iter().find(|r| r.path == k);
            let last = a.map(|r| r.last_opened).max(b.map(|r| r.last_opened));
            let frame = a.map(|r| r.frame.clone()).or_else(|| b.map(|r| r.frame.clone())).flatten();
            (last, a.and_then(|r| r.unlock_ms).or(b.and_then(|r| r.unlock_ms)), frame)
        };
        let mut add = |p: PathBuf, source: &'static str, out: &mut Vec<RecentEntry>| {
            let key = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
            if self.hidden.contains(&p) || self.hidden.contains(&key) || !seen.insert(key) {
                return;
            }
            let (last, measured, frame) = recorded(&p);
            if let Some(e) = entry(&p, source, last, measured, frame) {
                out.push(e);
            }
        };
        if let Some(d) = &self.loc.exe_dir {
            for p in scan(d, 1) {
                add(p, "here", &mut out);
            }
        }
        for d in &self.scan_dirs {
            for p in scan(d, 1) {
                add(p, "here", &mut out);
            }
        }
        for root in self.removable() {
            for p in scan(&root, 0) {
                add(p, "removable", &mut out);
            }
        }
        for r in &self.session {
            add(PathBuf::from(&r.path), "session", &mut out);
        }
        for r in &self.known.volumes {
            add(PathBuf::from(&r.path), "known", &mut out);
        }
        out.sort_by_key(|e| std::cmp::Reverse(e.last_opened));
        out
    }
}

fn is_nvault(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nvault"))
}

/// `*.nvault` files in `dir` and, with `depth` 1, in its direct subfolders (no symlinked
/// folders, no hidden ones).
fn scan(dir: &Path, depth: u32) -> Vec<PathBuf> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten().take(MAX_DIR_ENTRIES) {
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        if ft.is_file() && is_nvault(&p) {
            out.push(p);
        } else if ft.is_dir() && depth > 0 && !e.file_name().to_string_lossy().starts_with('.') {
            out.extend(scan(&p, depth - 1));
        }
    }
    out.sort();
    out
}

fn mtime_ms(m: &std::fs::Metadata) -> u64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

/// One list entry. A reachable file that is not a NiceVault volume is skipped; a missing
/// file (other stick, moved) is listed with `exists = false` and no look.
fn entry(p: &Path, source: &'static str, last: Option<u64>, measured: Option<u64>, frame: Option<String>) -> Option<RecentEntry> {
    let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Volume".into());
    let md = std::fs::metadata(p).ok().filter(|m| m.is_file());
    let mut e = RecentEntry {
        path: p.to_string_lossy().into_owned(),
        name,
        size_bytes: md.as_ref().map_or(0, |m| m.len()),
        last_opened: last.unwrap_or(0),
        exists: md.is_some(),
        unlock_ms: measured,
        emblem: None,
        show_when_locked: false,
        accent: None,
        source,
        volume_tag: None,
        key_slots: None,
        kdf: None,
        emblem_frame: None,
    };
    let Some(md) = md else { return Some(e) };
    e.last_opened = last.unwrap_or_else(|| mtime_ms(&md));
    match nv_core::peek(p) {
        Ok(peek) => {
            e.unlock_ms = e.unlock_ms.or(peek.unlock_ms);
            e.volume_tag = Some(volume_tag(&peek.volume_id));
            e.key_slots = u32::try_from(peek.slots).ok();
            e.kdf = peek.password_kdf.first().copied();
            if let Some(a) = &peek.appearance {
                let (accent, emblem) = crate::appearance::from_header(a);
                e.accent = accent;
                e.emblem = Some(emblem.unwrap_or(Emblem::Initial));
                e.show_when_locked = true;
                e.emblem_frame = frame;
            }
        }
        Err(nv_core::OpenError::NotAVault) => return None,
        Err(_) => {} // unreadable / damaged: still listed, Sblocca reports the problem
    }
    Some(e)
}

/// Roots of the mounted removable drives on this machine: the removable / external entries of
/// the drive list (drives.rs), so Benvenuto scans exactly the drives "Nuovo volume" offers.
pub fn removable_roots() -> Vec<PathBuf> {
    crate::drives::removable_roots()
}

/// `dir` sits on one of the removable drive `roots`.
pub fn on_removable(dir: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| dir.starts_with(r))
}

/// Lexically drops `.` and resolves `..` (no filesystem access, symlinks untouched).
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_depth_and_filters() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        for p in ["a.nvault", "B.NVAULT", "c.txt", "sub/d.nvault", "sub/deeper/e.nvault", ".hidden/f.nvault"] {
            let p = d.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        let names = |v: Vec<PathBuf>| v.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        let mut one = names(scan(d, 1));
        one.sort();
        assert_eq!(one, ["B.NVAULT", "a.nvault", "d.nvault"]);
        assert_eq!(names(scan(d, 0)).len(), 2);
        assert!(on_removable(&d.join("x"), &[d.to_path_buf()]));
        assert_eq!(normalize(Path::new("/a/./b/../c")), Path::new("/a/c"));
    }

    /// An old settings.json without `video_arrows` loads with "seek"; junk values are ignored;
    /// a chosen value persists.
    #[test]
    fn video_arrows_default_seek_and_round_trip() {
        let t = tempfile::tempdir().unwrap();
        let loc = || Locations { exe_dir: None, removable: Some(vec![]), config_dir: Some(t.path().to_path_buf()) };
        std::fs::write(t.path().join(GLOBAL_FILE), r#"{"language":"it","sounds":false}"#).unwrap();
        let mut st = Store::new(loc());
        assert_eq!(st.video_arrows(), "seek");
        st.set_video_arrows("nonsense");
        assert_eq!(st.video_arrows(), "seek");
        st.set_video_arrows("step");
        assert_eq!(Store::new(loc()).video_arrows(), "step");
        std::fs::write(t.path().join(GLOBAL_FILE), r#"{"video_arrows":"junk"}"#).unwrap();
        assert_eq!(Store::new(loc()).video_arrows(), "seek");
    }

    /// known.json from a newer build: unknown keys survive a rewrite; a file this build cannot
    /// read is not replaced.
    #[test]
    fn known_list_round_trips_unknown_keys() {
        let t = tempfile::tempdir().unwrap();
        let loc = || Locations { exe_dir: None, removable: Some(vec![]), config_dir: Some(t.path().to_path_buf()) };
        let file = t.path().join(KNOWN_FILE);
        std::fs::write(&file, r#"{"volumes":[{"path":"/a.nvault","last_opened":1,"pinned":true}],"sync":{"v":2}}"#).unwrap();
        Store::new(loc()).touch(Path::new("/b.nvault"), None);
        Store::new(loc()).touch(Path::new("/a.nvault"), Some(5));
        let j: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(j["sync"], serde_json::json!({"v": 2}));
        assert_eq!((j["volumes"][0]["path"].as_str(), &j["volumes"][0]["pinned"]), (Some("/a.nvault"), &serde_json::json!(true)));
        std::fs::write(&file, b"[not the list]").unwrap();
        Store::new(loc()).touch(Path::new("/c.nvault"), None);
        assert_eq!(std::fs::read(&file).unwrap(), b"[not the list]");
    }

    /// iOS update: the container moved. The volume recorded under the old container shows up
    /// again under the new one (history kept, list rewritten), a volume only in the app's
    /// Volumes folder is listed by the scan, and nothing is deleted.
    #[test]
    fn known_volumes_follow_a_moved_container() {
        let t = tempfile::tempdir().unwrap();
        let old = t.path().join("Containers/Data/Application/OLD-1/Library/Volumes");
        let new = t.path().join("Containers/Data/Application/NEW-2/Library/Volumes");
        std::fs::create_dir_all(&new).unwrap();
        let config = t.path().join("Containers/Data/Application/NEW-2/Library/Preferences");
        std::fs::create_dir_all(&config).unwrap();
        let rec = |p: &Path| format!(r#"{{"path":{},"last_opened":7,"unlock_ms":900,"frame":"rope"}}"#, serde_json::to_string(&p.to_string_lossy()).unwrap());
        let gone = t.path().join("elsewhere/Gone.nvault");
        std::fs::write(config.join(KNOWN_FILE), format!(r#"{{"volumes":[{},{}]}}"#, rec(&old.join("Mio.nvault")), rec(&gone))).unwrap();
        let pw = secrecy::SecretString::from("pw".to_owned());
        for n in ["Mio.nvault", "Altro.nvault"] {
            drop(nv_core::Vault::create(&new.join(n), &pw, nv_core::KdfPreset::Fast).unwrap());
        }
        let loc = || Locations { exe_dir: None, removable: Some(vec![]), config_dir: Some(config.clone()) };
        let home = t.path().join("Containers/Data/Application/NEW-2/Library");
        let mut s = Store::new(loc()).with_scan_dirs(vec![new.clone()]);
        s.relocate_known(|p| crate::mobile_paths::rebase_container(p, &home));
        let r = s.recent();
        let mio = r.iter().find(|e| e.path == new.join("Mio.nvault").to_string_lossy()).expect("relocated");
        assert!(mio.exists);
        assert_eq!(mio.unlock_ms, Some(900));
        assert!(r.iter().any(|e| e.path == new.join("Altro.nvault").to_string_lossy()), "scanned");
        // the unrelated missing one is still listed (as missing), not dropped
        assert!(r.iter().any(|e| e.path == gone.to_string_lossy() && !e.exists));
        assert!(!r.iter().any(|e| e.path == old.join("Mio.nvault").to_string_lossy()));
        // persisted: a fresh start sees the new path without relocating again
        let j: serde_json::Value = serde_json::from_slice(&std::fs::read(config.join(KNOWN_FILE)).unwrap()).unwrap();
        assert!(j["volumes"].as_array().unwrap().iter().any(|v| v["path"] == serde_json::json!(new.join("Mio.nvault").to_string_lossy())));
        assert!(new.join("Mio.nvault").is_file() && new.join("Altro.nvault").is_file());
        // idempotent
        let mut s2 = Store::new(loc());
        s2.relocate_known(|p| crate::mobile_paths::rebase_container(p, &home));
        assert_eq!(s2.recent().iter().filter(|e| e.name == "Mio").count(), 1);
    }
}
