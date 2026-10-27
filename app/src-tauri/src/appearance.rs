use crate::emblem::{Emblem, EmblemState};
use crate::error::{ApiError, ApiResult};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use nv_core::{Appearance, LockedEmblem, Vault};
use serde::{Deserialize, Serialize};

pub const SETTINGS_KEY: &str = "nicevault.settings";
pub const LOCKED_IMAGE_KEY: &str = "nicevault.emblem_locked";

pub const ACCENTS: [&str; 7] = ["velluto", "prugna", "prussia", "bottiglia", "seppia", "ardesia", "ottanio"];
pub const THEMES: [&str; 3] = ["system", "light", "dark"];
pub const DEFAULT_THEME: &str = "dark";

pub fn migrate_accent(id: &str) -> &str {
    match id {
        "bordeaux" => "velluto",
        other => other,
    }
}
pub const LOCK_SCOPES: [&str; 2] = ["volume", "all"];
pub const GRID_SIZES: [&str; 4] = ["s", "m", "l", "xl"];
pub const LIST_DENSITIES: [&str; 3] = ["compact", "normal", "comfortable"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub auto_lock_minutes: u32,
    pub accent: String,
    pub theme: String,
    pub lock_on_sleep: bool,
    pub lock_on_screen_lock: bool,
    pub lock_scope: String,
    pub grid_size: String,
    pub list_density: String,
    pub browser_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_lock_minutes: 15,
            accent: ACCENTS[0].into(),
            theme: DEFAULT_THEME.into(),
            lock_on_sleep: true,
            lock_on_screen_lock: true,
            lock_scope: LOCK_SCOPES[0].into(),
            grid_size: GRID_SIZES[1].into(),
            list_density: LIST_DENSITIES[1].into(),
            browser_enabled: true,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct SettingsPatch {
    pub auto_lock_minutes: Option<u32>,
    pub accent: Option<String>,
    pub theme: Option<String>,
    pub lock_on_sleep: Option<bool>,
    pub lock_on_screen_lock: Option<bool>,
    pub lock_scope: Option<String>,
    pub grid_size: Option<String>,
    pub list_density: Option<String>,
    pub browser_enabled: Option<bool>,
}

impl Settings {
    pub fn apply(&mut self, p: SettingsPatch) -> Vec<&'static str> {
        let mut set = vec![];
        if let Some(m) = p.auto_lock_minutes {
            self.auto_lock_minutes = m.min(24 * 60);
            set.push("auto_lock_minutes");
        }
        if let Some(a) = p.accent.map(|a| migrate_accent(&a).to_owned()).filter(|a| ACCENTS.contains(&a.as_str())) {
            self.accent = a;
            set.push("accent");
        }
        if let Some(t) = p.theme.filter(|t| THEMES.contains(&t.as_str())) {
            self.theme = t;
            set.push("theme");
        }
        if let Some(b) = p.lock_on_sleep {
            self.lock_on_sleep = b;
            set.push("lock_on_sleep");
        }
        if let Some(b) = p.lock_on_screen_lock {
            self.lock_on_screen_lock = b;
            set.push("lock_on_screen_lock");
        }
        if let Some(s) = p.lock_scope.filter(|s| LOCK_SCOPES.contains(&s.as_str())) {
            self.lock_scope = s;
            set.push("lock_scope");
        }
        if let Some(g) = p.grid_size.filter(|g| GRID_SIZES.contains(&g.as_str())) {
            self.grid_size = g;
            set.push("grid_size");
        }
        if let Some(d) = p.list_density.filter(|d| LIST_DENSITIES.contains(&d.as_str())) {
            self.list_density = d;
            set.push("list_density");
        }
        if let Some(b) = p.browser_enabled {
            self.browser_enabled = b;
            set.push("browser_enabled");
        }
        set
    }

    fn from_map(m: &serde_json::Map<String, serde_json::Value>) -> Settings {
        let mut s = Settings::default();
        let str_of = |k: &str| m.get(k).and_then(|v| v.as_str()).map(str::to_owned);
        let bool_of = |k: &str| m.get(k).and_then(|v| v.as_bool());
        if let Some(n) = m.get("auto_lock_minutes").and_then(|v| v.as_u64()) {
            s.auto_lock_minutes = n.min(u32::MAX as u64) as u32;
        }
        if let Some(x) = str_of("accent") {
            s.accent = x;
        }
        if let Some(x) = str_of("theme") {
            s.theme = x;
        }
        if let Some(x) = str_of("lock_scope") {
            s.lock_scope = x;
        }
        if let Some(x) = str_of("grid_size") {
            s.grid_size = x;
        }
        if let Some(x) = str_of("list_density") {
            s.list_density = x;
        }
        if let Some(b) = bool_of("lock_on_sleep") {
            s.lock_on_sleep = b;
        }
        if let Some(b) = bool_of("lock_on_screen_lock") {
            s.lock_on_screen_lock = b;
        }
        if let Some(b) = bool_of("browser_enabled") {
            s.browser_enabled = b;
        }
        s.sanitized()
    }

    pub fn locks_on(&self, reason: &str) -> bool {
        match reason {
            "sleep" => self.lock_on_sleep,
            "screen" => self.lock_on_screen_lock,
            _ => true,
        }
    }

    pub fn locks_all(&self) -> bool {
        self.lock_scope == "all"
    }

    fn sanitized(mut self) -> Self {
        let d = Settings::default();
        self.accent = migrate_accent(&self.accent).to_owned();
        if !ACCENTS.contains(&self.accent.as_str()) {
            self.accent = d.accent;
        }
        if !THEMES.contains(&self.theme.as_str()) {
            self.theme = d.theme;
        }
        if !LOCK_SCOPES.contains(&self.lock_scope.as_str()) {
            self.lock_scope = d.lock_scope;
        }
        if !GRID_SIZES.contains(&self.grid_size.as_str()) {
            self.grid_size = d.grid_size;
        }
        if !LIST_DENSITIES.contains(&self.list_density.as_str()) {
            self.list_density = d.list_density;
        }
        self.auto_lock_minutes = self.auto_lock_minutes.min(24 * 60);
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    #[default]
    Visible,
    Encrypted,
}

impl Visibility {
    pub fn from_show(show_when_locked: bool) -> Self {
        if show_when_locked { Visibility::Visible } else { Visibility::Encrypted }
    }
    pub fn shown(self) -> bool {
        self == Visibility::Visible
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AppearanceState {
    pub accent: String,
    pub emblem: Emblem,
    pub emblem_frame: String,
    pub visibility: Visibility,
    pub tampered: bool,
}

enum RawSettings {
    Absent,
    Object(serde_json::Map<String, serde_json::Value>),
    Unreadable,
}

fn raw_settings(v: &Vault) -> RawSettings {
    match v.meta_get(SETTINGS_KEY) {
        Ok(None) => RawSettings::Absent,
        Ok(Some(b)) => match serde_json::from_slice(&b) {
            Ok(serde_json::Value::Object(m)) => RawSettings::Object(m),
            _ => RawSettings::Unreadable,
        },
        Err(_) => RawSettings::Unreadable,
    }
}

pub fn load_settings(v: &Vault) -> Settings {
    match raw_settings(v) {
        RawSettings::Object(m) => Settings::from_map(&m),
        RawSettings::Absent | RawSettings::Unreadable => Settings::default(),
    }
}

pub fn save_settings(v: &Vault, s: &Settings) -> ApiResult<()> {
    let json = serde_json::to_vec(s).map_err(|e| ApiError::new("error", e.to_string()))?;
    Ok(v.meta_set(SETTINGS_KEY, Some(&json))?)
}

pub fn patch_settings(v: &Vault, patch: SettingsPatch) -> ApiResult<Settings> {
    let mut m = match raw_settings(v) {
        RawSettings::Absent => serde_json::Map::new(),
        RawSettings::Object(m) => m,
        RawSettings::Unreadable => {
            return Err(ApiError::new(
                "unsupported_version",
                "This volume's settings were saved by a newer version of NiceVault: change them with that version.",
            ));
        }
    };
    let mut s = Settings::from_map(&m);
    let keys = s.apply(patch);
    if !keys.is_empty() {
        let full = serde_json::to_value(&s).map_err(|e| ApiError::new("error", e.to_string()))?;
        for k in keys {
            m.insert(k.into(), full[k].clone());
        }
        let json = serde_json::to_vec(&m).map_err(|e| ApiError::new("error", e.to_string()))?;
        v.meta_set(SETTINGS_KEY, Some(&json))?;
    }
    Ok(s)
}

pub fn validate_locked_image(b64: &str) -> ApiResult<Vec<u8>> {
    let raw = b64.split_once(',').filter(|(p, _)| p.starts_with("data:")).map_or(b64, |(_, d)| d);
    let bytes = B64.decode(raw.trim()).map_err(|_| ApiError::new("invalid_emblem", "Unreadable thumbnail."))?;
    let a = Appearance { accent: None, emblem: Some(LockedEmblem::Image(bytes)) };
    a.validate().map_err(|e| ApiError::new("invalid_emblem", format!("Invalid thumbnail: {e}.")))?;
    let Some(LockedEmblem::Image(b)) = a.emblem else { unreachable!() };
    Ok(b)
}

pub fn load_locked_image(v: &Vault) -> Option<Vec<u8>> {
    v.meta_get(LOCKED_IMAGE_KEY).ok().flatten()
}

pub fn save_locked_image(v: &Vault, img: Option<&[u8]>) -> ApiResult<()> {
    Ok(v.meta_set(LOCKED_IMAGE_KEY, img)?)
}

pub fn initial_of(name: &str) -> String {
    name.chars().find(|c| c.is_alphanumeric()).and_then(|c| c.to_uppercase().next()).map(String::from).unwrap_or_default()
}

pub fn header_section(name: &str, e: &EmblemState, s: &Settings, locked_image: Option<Vec<u8>>) -> Option<Appearance> {
    if !e.show_when_locked {
        return None;
    }
    let emblem = match &e.emblem {
        Emblem::Initial => Some(LockedEmblem::Initial(initial_of(name))),
        Emblem::Icon { id } => Some(LockedEmblem::Icon(id.clone())),
        Emblem::Image { data_base64 } => {
            let fits = |b: Vec<u8>| {
                let a = Appearance { accent: None, emblem: Some(LockedEmblem::Image(b)) };
                a.validate().is_ok().then_some(a.emblem).flatten()
            };
            locked_image.and_then(fits).or_else(|| B64.decode(data_base64).ok().and_then(fits))
        }
    };
    let a = Appearance { accent: Some(s.accent.clone()), emblem };
    a.validate().is_ok().then_some(a)
}

pub fn sync_header(v: &Vault, name: &str) -> ApiResult<()> {
    let Some(e) = crate::emblem::stored(v) else { return Ok(()) };
    if matches!(raw_settings(v), RawSettings::Unreadable) {
        return Ok(());
    }
    let s = load_settings(v);
    let img = if matches!(e.emblem, Emblem::Image { .. }) { load_locked_image(v) } else { None };
    v.set_appearance(header_section(name, &e, &s, img).as_ref()).map_err(|e| match e {
        nv_core::AppearanceError::Fs(e) => e.into(),
        e => ApiError::new("invalid_emblem", e.to_string()),
    })
}

pub fn from_header(a: &Appearance) -> (Option<String>, Option<Emblem>) {
    let emblem = a.emblem.as_ref().map(|e| match e {
        LockedEmblem::Initial(_) => Emblem::Initial,
        LockedEmblem::Icon(id) => Emblem::Icon { id: id.clone() },
        LockedEmblem::Image(b) => Emblem::Image { data_base64: B64.encode(b) },
    });
    let accent = a.accent.as_deref().map(migrate_accent).filter(|x| ACCENTS.contains(x)).map(str::to_owned);
    (accent, emblem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_patch_and_sanitize() {
        let mut s = Settings::default();
        s.apply(SettingsPatch {
            accent: Some("#0047ff".into()),
            theme: Some("neon".into()),
            auto_lock_minutes: Some(99_999),
            lock_scope: Some("galaxy".into()),
            ..Default::default()
        });
        assert_eq!(s, Settings { auto_lock_minutes: 24 * 60, ..Settings::default() });
        s.apply(SettingsPatch {
            accent: Some("ottanio".into()),
            theme: Some("dark".into()),
            lock_on_sleep: Some(false),
            lock_scope: Some("all".into()),
            ..Default::default()
        });
        assert_eq!((s.accent.as_str(), s.theme.as_str()), ("ottanio", "dark"));
        assert!(!s.locks_on("sleep") && s.locks_on("screen") && s.locks_all());
        let old: Settings = serde_json::from_str(r#"{"auto_lock_minutes": 5, "accent": "nope", "lock_scope": "x"}"#).unwrap();
        assert_eq!(old.sanitized(), Settings { auto_lock_minutes: 5, ..Settings::default() });
        let legacy: Settings = serde_json::from_str(r#"{"accent": "bordeaux", "theme": "system"}"#).unwrap();
        assert_eq!((legacy.sanitized().accent.as_str(), Settings::default().theme.as_str()), ("velluto", "dark"));
        let hdr = Appearance { accent: Some("bordeaux".into()), emblem: None };
        assert_eq!(from_header(&hdr).0.as_deref(), Some("velluto"));
        assert_eq!(initial_of("  über Archiv"), "Ü");
        assert_eq!(initial_of("--"), "");
    }

    #[test]
    fn header_section_by_visibility() {
        let s = Settings::default();
        let shown = |emblem| EmblemState { emblem, show_when_locked: true, emblem_frame: crate::emblem::default_frame() };
        assert_eq!(
            header_section("casa", &shown(Emblem::Initial), &s, None),
            Some(Appearance { accent: Some("velluto".into()), emblem: Some(LockedEmblem::Initial("C".into())) })
        );
        let hidden = EmblemState { emblem: Emblem::Icon { id: "house".into() }, show_when_locked: false, emblem_frame: crate::emblem::default_frame() };
        assert_eq!(header_section("casa", &hidden, &s, None), None);
        let mut big = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\x01\0\0\0\x01\0".to_vec();
        big.resize(300, 0);
        let img = shown(Emblem::Image { data_base64: B64.encode(&big) });
        assert_eq!(header_section("x", &img, &s, None).unwrap().emblem, None);
        let mut small = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x80\0\0\0\x80".to_vec();
        small.resize(300, 0);
        assert_eq!(header_section("x", &img, &s, Some(small.clone())).unwrap().emblem, Some(LockedEmblem::Image(small.clone())));
        assert!(validate_locked_image(&format!("data:image/png;base64,{}", B64.encode(&small))).is_ok());
        assert!(validate_locked_image(&B64.encode(&big)).is_err_and(|e| e.code == "invalid_emblem"));
    }

    #[test]
    fn view_sizes() {
        let d = Settings::default();
        assert_eq!((d.grid_size.as_str(), d.list_density.as_str()), ("m", "normal"));
        let mut s = d.clone();
        let set = s.apply(SettingsPatch { grid_size: Some("xl".into()), list_density: Some("compact".into()), ..Default::default() });
        assert_eq!(set, ["grid_size", "list_density"]);
        s.apply(SettingsPatch { grid_size: Some("huge".into()), list_density: Some("airy".into()), ..Default::default() });
        assert_eq!((s.grid_size.as_str(), s.list_density.as_str()), ("xl", "compact"), "unknown values ignored");
        let m = serde_json::json!({ "accent": "prussia", "grid_size": 3, "list_density": "comfortable" });
        let r = Settings::from_map(m.as_object().unwrap());
        assert_eq!((r.accent.as_str(), r.grid_size.as_str(), r.list_density.as_str()), ("prussia", "m", "comfortable"));
    }
}
