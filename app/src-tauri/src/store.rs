pub use crate::appearance::{ACCENTS, Settings, SettingsPatch};
use crate::emblem::Emblem;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

const KNOWN_FILE: &str = "known.json";
const GLOBAL_FILE: &str = "settings.json";
pub const LANGUAGES: [&str; 3] = ["system", "it", "en"];
pub const UI_SCALES: [u32; 5] = [90, 100, 110, 125, 150];
pub const VIDEO_ARROWS: [&str; 2] = ["seek", "step"];
pub const LEGACY_FILE: &str = "NiceVault.json";
const MAX_KNOWN: usize = 50;
const MAX_ANCHORS: usize = 256;
const MAX_DIR_ENTRIES: usize = 4096;

#[derive(Clone, Debug, Serialize)]
pub struct RecentEntry {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub last_opened: u64,
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlock_ms: Option<u64>,
    pub emblem: Option<Emblem>,
    pub show_when_locked: bool,
    pub accent: Option<String>,
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_slots: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kdf: Option<nv_core::KdfParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emblem_frame: Option<String>,
}

fn hex_id(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn volume_tag(id: &[u8; 16]) -> String {
    const A: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut v: u64 = 0;
    for b in &id[..6] {
        v = (v << 8) | u64::from(*b);
    }
    let v = v >> 3;
    (0..9).rev().map(|i| A[((v >> (i * 5)) & 31) as usize] as char).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct KnownRec {
    path: String,
    last_opened: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unlock_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frame: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct KnownFile {
    #[serde(default)]
    volumes: Vec<KnownRec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    anchors: BTreeMap<String, u64>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, Default)]
pub struct Locations {
    pub exe_dir: Option<PathBuf>,
    pub removable: Option<Vec<PathBuf>>,
    pub config_dir: Option<PathBuf>,
}

impl Locations {
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
        let config_dir = (!portable)
            .then(|| env("NICEVAULT_CONFIG_DIR").map(PathBuf::from).or_else(|| dirs::config_dir().map(|d| d.join("NiceVault"))))
            .flatten();
        Locations { exe_dir, removable, config_dir }
    }
}

pub struct Store {
    loc: Locations,
    known: KnownFile,
    session: Vec<KnownRec>,
    hidden: HashSet<PathBuf>,
    defaults: Settings,
    language: String,
    site_icons: bool,
    sounds: bool,
    video_arrows: String,
    sidebar_collapsed: bool,
    app_theme: String,
    app_accent: String,
    ui_scale: u32,
    new_like_app: bool,
    global_raw: serde_json::Map<String, serde_json::Value>,
    global_unreadable: bool,
    start_at_login: bool,
    start_minimized: bool,
    close_to_tray: bool,
    gallery_save: Option<bool>,
    browser_link: bool,
    pm_default_volume: Option<String>,
    autostart: Option<crate::autostart::Autostart>,
    scan_dirs: Vec<PathBuf>,
}

pub const GLOBAL_UNREADABLE: &str = "unsupported_version";

pub fn exe_dir() -> Option<PathBuf> {
    crate::updater::app_exe()?.parent().map(Path::to_path_buf)
}

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
    pub fn open_default() -> Store {
        let loc = Locations::detect();
        let autostart = loc.config_dir.is_some().then(crate::autostart::Autostart::detect).flatten();
        let scan = std::env::var_os("NICEVAULT_SCAN_DIRS").map(|v| std::env::split_paths(&v).filter(|p| !p.as_os_str().is_empty()).collect()).unwrap_or_default();
        let mut s = Store::new(loc).with_autostart(autostart).with_scan_dirs(scan);
        s.migrate_legacy();
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
        let mut browser_link = false;
        let mut pm_default_volume = None;
        if let Some(bytes) = loc.config_dir.as_ref().and_then(|d| std::fs::read(d.join(GLOBAL_FILE)).ok()) {
            match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(serde_json::Value::Object(map)) => {
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
                    if let Some(b) = map.get("browser_link").and_then(|v| v.as_bool()) {
                        browser_link = b;
                    }
                    pm_default_volume = map.get("pm_default_volume").and_then(|v| v.as_str()).filter(|v| !v.is_empty()).map(str::to_owned);
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
            browser_link,
            pm_default_volume,
            autostart: None,
            scan_dirs: Vec::new(),
        }
    }

    pub fn with_autostart(mut self, a: Option<crate::autostart::Autostart>) -> Store {
        self.autostart = a;
        self
    }

    pub fn with_scan_dirs(mut self, dirs: Vec<PathBuf>) -> Store {
        self.scan_dirs = dirs;
        self
    }

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
        self.global_raw.insert("browser_link".into(), serde_json::Value::Bool(self.browser_link));
        self.global_raw.insert("pm_default_volume".into(), self.pm_default_volume.clone().map_or(serde_json::Value::Null, serde_json::Value::String));
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

    pub fn site_icons(&self) -> bool {
        self.site_icons
    }

    pub fn set_site_icons(&mut self, on: bool) {
        if on != self.site_icons {
            self.site_icons = on;
            self.save_global();
        }
    }

    pub fn sounds(&self) -> bool {
        self.sounds
    }

    pub fn set_sounds(&mut self, on: bool) {
        if on != self.sounds {
            self.sounds = on;
            self.save_global();
        }
    }

    pub fn video_arrows(&self) -> &str {
        &self.video_arrows
    }

    pub fn set_video_arrows(&mut self, v: &str) {
        if VIDEO_ARROWS.contains(&v) && v != self.video_arrows {
            self.video_arrows = v.to_owned();
            self.save_global();
        }
    }

    pub fn sidebar_collapsed(&self) -> bool {
        self.sidebar_collapsed
    }

    pub fn set_sidebar_collapsed(&mut self, on: bool) {
        if on != self.sidebar_collapsed {
            self.sidebar_collapsed = on;
            self.save_global();
        }
    }

    pub fn global_unreadable(&self) -> bool {
        self.global_unreadable
    }

    pub fn set_language(&mut self, l: &str) -> String {
        if LANGUAGES.contains(&l) && l != self.language {
            self.language = l.to_owned();
            self.save_global();
        }
        self.language()
    }

    pub fn app_look(&self) -> (String, String, u32, bool) {
        (self.app_theme.clone(), self.app_accent.clone(), self.ui_scale, self.new_like_app)
    }

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

    pub fn startup(&self) -> (bool, bool) {
        (self.start_at_login, self.start_minimized)
    }

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

    pub fn gallery_save(&self) -> Option<bool> {
        self.gallery_save
    }

    pub fn set_gallery_save(&mut self, on: bool) {
        if self.gallery_save != Some(on) {
            self.gallery_save = Some(on);
            self.save_global();
        }
    }

    pub fn browser_link(&self) -> bool {
        self.installed() && self.browser_link
    }

    pub fn set_browser_link(&mut self, on: bool) {
        if self.installed() && on != self.browser_link {
            self.browser_link = on;
            self.save_global();
        }
    }

    pub fn pm_default_volume(&self) -> Option<String> {
        self.pm_default_volume.clone()
    }

    pub fn set_pm_default_volume(&mut self, path: Option<String>) {
        let path = path.filter(|p| !p.is_empty());
        if path != self.pm_default_volume {
            self.pm_default_volume = path;
            self.save_global();
        }
    }

    pub fn close_to_tray(&self) -> bool {
        self.installed() && self.close_to_tray
    }

    pub fn set_close_to_tray(&mut self, on: bool) {
        if !self.installed() || on == self.close_to_tray {
            return;
        }
        self.close_to_tray = on;
        self.save_global();
    }

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

    pub fn new_volume_settings(&self) -> Settings {
        let mut s = self.defaults();
        if self.new_like_app {
            s.theme = self.app_theme.clone();
            s.accent = self.app_accent.clone();
        }
        s
    }

    pub fn installed(&self) -> bool {
        self.loc.config_dir.is_some()
    }

    pub fn locations(&self) -> &Locations {
        &self.loc
    }

    fn save_known(&self) {
        let Some(dir) = &self.loc.config_dir else { return };
        let file = dir.join(KNOWN_FILE);
        if std::fs::read(&file).is_ok_and(|b| serde_json::from_slice::<KnownFile>(&b).is_err()) {
            return;
        }
        let Ok(json) = serde_json::to_vec_pretty(&self.known) else { return };
        let _ = write_private(&file, &json);
    }

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

    pub fn anchor(&self, id: &[u8; 16]) -> Option<u64> {
        self.known.anchors.get(&hex_id(id)).copied()
    }

    pub fn set_anchor(&mut self, id: &[u8; 16], seq: u64, reset: bool) {
        let k = hex_id(id);
        let old = self.known.anchors.get(&k).copied();
        let new = if reset { seq } else { old.map_or(seq, |o| o.max(seq)) };
        if old == Some(new) {
            return;
        }
        self.known.anchors.insert(k.clone(), new);
        while self.known.anchors.len() > MAX_ANCHORS {
            let Some(drop) = self.known.anchors.keys().find(|x| **x != k).cloned() else { break };
            self.known.anchors.remove(&drop);
        }
        if self.installed() {
            self.save_known();
        }
    }

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
        Err(_) => {}
    }
    Some(e)
}

pub fn removable_roots() -> Vec<PathBuf> {
    crate::drives::removable_roots()
}

pub fn on_removable(dir: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| dir.starts_with(r))
}

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
        assert!(r.iter().any(|e| e.path == gone.to_string_lossy() && !e.exists));
        assert!(!r.iter().any(|e| e.path == old.join("Mio.nvault").to_string_lossy()));
        let j: serde_json::Value = serde_json::from_slice(&std::fs::read(config.join(KNOWN_FILE)).unwrap()).unwrap();
        assert!(j["volumes"].as_array().unwrap().iter().any(|v| v["path"] == serde_json::json!(new.join("Mio.nvault").to_string_lossy())));
        assert!(new.join("Mio.nvault").is_file() && new.join("Altro.nvault").is_file());
        let mut s2 = Store::new(loc());
        s2.relocate_known(|p| crate::mobile_paths::rebase_container(p, &home));
        assert_eq!(s2.recent().iter().filter(|e| e.name == "Mio").count(), 1);
    }
}
