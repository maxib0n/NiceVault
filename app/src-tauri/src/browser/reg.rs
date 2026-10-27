use nv_bridge::{FIREFOX_ID, HOST_NAME};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const CHROME_IDS: &[&str] = &["npegemkakgbadljnibnipnaleoffjfbf"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Chromium,
    Firefox,
}

struct Known {
    name: &'static str,
    family: Family,
    linux: &'static str,
    mac: &'static str,
    always: bool,
}

const KNOWN: &[Known] = &[
    Known { name: "Google Chrome", family: Family::Chromium, linux: "google-chrome", mac: "Library/Application Support/Google/Chrome", always: true },
    Known { name: "Chromium", family: Family::Chromium, linux: "chromium", mac: "Library/Application Support/Chromium", always: false },
    Known { name: "Microsoft Edge", family: Family::Chromium, linux: "microsoft-edge", mac: "Library/Application Support/Microsoft Edge", always: false },
    Known { name: "Brave", family: Family::Chromium, linux: "BraveSoftware/Brave-Browser", mac: "Library/Application Support/BraveSoftware/Brave-Browser", always: false },
    Known { name: "Vivaldi", family: Family::Chromium, linux: "vivaldi", mac: "Library/Application Support/Vivaldi", always: false },
    Known { name: "Firefox", family: Family::Firefox, linux: "~/.mozilla", mac: "Library/Application Support/Mozilla", always: true },
    Known { name: "Firefox", family: Family::Firefox, linux: "mozilla", mac: "", always: false },
];

#[derive(Clone, Debug)]
pub struct Homes {
    pub home: PathBuf,
    pub config: PathBuf,
}

impl Homes {
    pub fn of(home: &Path) -> Homes {
        Homes { home: home.to_owned(), config: home.join(".config") }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Written {
    pub browser: String,
    pub path: String,
}

pub fn homes() -> Option<Homes> {
    let home = dirs::home_dir()?;
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(".config"));
    Some(Homes { home, config })
}

pub fn chromium_manifest(bridge: &Path, ids: &[&str]) -> String {
    let origins: Vec<String> = ids.iter().map(|id| format!("chrome-extension://{id}/")).collect();
    let v = serde_json::json!({ "name": HOST_NAME, "description": "NiceVault", "path": bridge, "type": "stdio", "allowed_origins": origins });
    serde_json::to_string_pretty(&v).expect("json")
}

pub fn firefox_manifest(bridge: &Path) -> String {
    let v = serde_json::json!({ "name": HOST_NAME, "description": "NiceVault", "path": bridge, "type": "stdio", "allowed_extensions": [FIREFOX_ID] });
    serde_json::to_string_pretty(&v).expect("json")
}

fn manifest_dir(h: &Homes, k: &Known) -> Option<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        if k.mac.is_empty() {
            return None;
        }
        h.home.join(k.mac)
    } else {
        match k.linux.strip_prefix("~/") {
            Some(rel) => h.home.join(rel),
            None => h.config.join(k.linux),
        }
    };
    Some(match (k.family, cfg!(target_os = "macos")) {
        (Family::Firefox, false) => base.join("native-messaging-hosts"),
        _ => base.join("NativeMessagingHosts"),
    })
}

fn manifest_file(dir: &Path) -> PathBuf {
    dir.join(format!("{HOST_NAME}.json"))
}

fn write_if_changed(p: &Path, data: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(p).is_ok_and(|s| s == data) {
        return Ok(());
    }
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension(format!("json.{}.tmp", crate::state::random_id()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, p).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(not(windows))]
pub fn register(h: &Homes, bridge: &Path) -> std::io::Result<Vec<Written>> {
    let mut out = vec![];
    for k in KNOWN {
        let Some(profile) = manifest_dir(h, k) else { continue };
        let exists = profile.parent().is_some_and(Path::is_dir);
        if !k.always && !exists {
            continue;
        }
        let data = match k.family {
            Family::Chromium => chromium_manifest(bridge, &ids()),
            Family::Firefox => firefox_manifest(bridge),
        };
        let f = manifest_file(&profile);
        write_if_changed(&f, &data)?;
        out.push(Written { browser: k.name.to_owned(), path: f.to_string_lossy().into_owned() });
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn unregister(h: &Homes) -> std::io::Result<()> {
    for k in KNOWN {
        let Some(d) = manifest_dir(h, k) else { continue };
        let f = manifest_file(&d);
        if std::fs::read_to_string(&f).is_ok_and(|s| s.contains(&format!("\"{HOST_NAME}\"")) && s.contains("\"NiceVault\"")) {
            std::fs::remove_file(&f)?;
        }
    }
    Ok(())
}

#[cfg(windows)]
const WIN_KEYS: &[(&str, Family, &str)] = &[
    (r"Software\Google\Chrome\NativeMessagingHosts", Family::Chromium, "Google Chrome, Chromium, Brave, Vivaldi"),
    (r"Software\Microsoft\Edge\NativeMessagingHosts", Family::Chromium, "Microsoft Edge"),
    (r"Software\Mozilla\NativeMessagingHosts", Family::Firefox, "Firefox"),
];

#[cfg(windows)]
fn win_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("NiceVault").join("browser"))
}

#[cfg(windows)]
pub fn register(_h: &Homes, bridge: &Path) -> std::io::Result<Vec<Written>> {
    let dir = win_dir().ok_or_else(|| std::io::Error::other("no LOCALAPPDATA"))?;
    let chromium = dir.join("chromium.json");
    let firefox = dir.join("firefox.json");
    write_if_changed(&chromium, &chromium_manifest(bridge, &ids()))?;
    write_if_changed(&firefox, &firefox_manifest(bridge))?;
    let mut out = vec![];
    for (key, fam, names) in WIN_KEYS {
        let json = if *fam == Family::Firefox { &firefox } else { &chromium };
        let k = format!(r"{key}\{HOST_NAME}");
        let v = json.to_string_lossy().into_owned();
        if crate::autostart::registry::get(&k, "").as_deref() != Some(v.as_str()) {
            crate::autostart::registry::set(&k, "", &v)?;
        }
        out.push(Written { browser: (*names).to_owned(), path: format!(r"HKCU\{k}") });
    }
    Ok(out)
}

#[cfg(windows)]
pub fn unregister(_h: &Homes) -> std::io::Result<()> {
    for (key, _, _) in WIN_KEYS {
        crate::autostart::registry::delete(&format!(r"{key}\{HOST_NAME}"), "")?;
    }
    if let Some(d) = win_dir() {
        let _ = std::fs::remove_file(d.join("chromium.json"));
        let _ = std::fs::remove_file(d.join("firefox.json"));
    }
    Ok(())
}

fn ids() -> Vec<&'static str> {
    CHROME_IDS.to_vec()
}

pub fn host_exe() -> Option<PathBuf> {
    if let Some(image) = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(image);
    }
    std::env::current_exe().ok().filter(|p| p.is_file())
}

pub fn sandboxed(home: &Path) -> Vec<String> {
    const FLATPAK: &[(&str, &str)] = &[
        ("org.mozilla.firefox", "Firefox (Flatpak)"),
        ("com.google.Chrome", "Google Chrome (Flatpak)"),
        ("org.chromium.Chromium", "Chromium (Flatpak)"),
        ("io.github.ungoogled_software.ungoogled_chromium", "Ungoogled Chromium (Flatpak)"),
        ("com.brave.Browser", "Brave (Flatpak)"),
        ("com.microsoft.Edge", "Microsoft Edge (Flatpak)"),
        ("com.vivaldi.Vivaldi", "Vivaldi (Flatpak)"),
        ("io.gitlab.librewolf-community", "LibreWolf (Flatpak)"),
    ];
    const SNAP: &[(&str, &str)] = &[("firefox", "Firefox (Snap)"), ("chromium", "Chromium (Snap)"), ("brave", "Brave (Snap)")];
    let mut out = vec![];
    if cfg!(target_os = "linux") {
        for (id, name) in FLATPAK {
            if home.join(".var/app").join(id).is_dir() {
                out.push((*name).to_owned());
            }
        }
        for (id, name) in SNAP {
            if home.join("snap").join(id).is_dir() {
                out.push((*name).to_owned());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_name_the_bridge_and_only_our_extension() {
        let c: serde_json::Value = serde_json::from_str(&chromium_manifest(Path::new("/opt/nv/nicevault"), &["abc"])).unwrap();
        assert_eq!(c["name"], HOST_NAME);
        assert_eq!(c["type"], "stdio");
        assert_eq!(c["path"], "/opt/nv/nicevault");
        assert_eq!(c["allowed_origins"], serde_json::json!(["chrome-extension://abc/"]));
        let f: serde_json::Value = serde_json::from_str(&firefox_manifest(Path::new("/opt/nv/nicevault"))).unwrap();
        assert_eq!(f["allowed_extensions"], serde_json::json!([FIREFOX_ID]));
        assert!(f.get("allowed_origins").is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn register_writes_existing_browsers_plus_chrome_and_firefox_then_removes() {
        let h = tempfile::tempdir().unwrap();
        let homes = Homes::of(h.path());
        let brave = h.path().join(if cfg!(target_os = "macos") { "Library/Application Support/BraveSoftware/Brave-Browser" } else { ".config/BraveSoftware/Brave-Browser" });
        std::fs::create_dir_all(&brave).unwrap();
        let bridge = h.path().join("nicevault");
        let w = register(&homes, &bridge).unwrap();
        let names: Vec<&str> = w.iter().map(|w| w.browser.as_str()).collect();
        assert_eq!(names, ["Google Chrome", "Brave", "Firefox"]);
        for x in &w {
            assert!(Path::new(&x.path).is_file(), "{}", x.path);
        }
        let ff = if cfg!(target_os = "macos") { "Library/Application Support/Mozilla/NativeMessagingHosts" } else { ".mozilla/native-messaging-hosts" };
        assert!(h.path().join(ff).join("app.nicevault.bridge.json").is_file());
        assert!(!h.path().join(".config/mozilla").exists());
        if !cfg!(target_os = "macos") {
            std::fs::create_dir_all(h.path().join(".config/mozilla")).unwrap();
            register(&homes, &bridge).unwrap();
            assert!(h.path().join(".config/mozilla/native-messaging-hosts/app.nicevault.bridge.json").is_file());
        }
        let p = Path::new(&w[0].path);
        let before = std::fs::metadata(p).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        register(&homes, &bridge).unwrap();
        assert_eq!(std::fs::metadata(p).unwrap().modified().unwrap(), before);
        let other = manifest_file(&brave.join("NativeMessagingHosts"));
        std::fs::write(&other, r#"{"name":"app.nicevault.bridge","description":"Other","path":"/usr/bin/other"}"#).unwrap();
        unregister(&homes).unwrap();
        assert!(!p.exists());
        assert!(other.exists());
    }

    #[test]
    fn sandboxed_browsers_are_found() {
        let h = tempfile::tempdir().unwrap();
        assert!(sandboxed(h.path()).is_empty());
        std::fs::create_dir_all(h.path().join(".var/app/org.mozilla.firefox")).unwrap();
        std::fs::create_dir_all(h.path().join("snap/chromium")).unwrap();
        std::fs::create_dir_all(h.path().join(".var/app/org.kde.kdenlive")).unwrap();
        let s = sandboxed(h.path());
        if cfg!(target_os = "linux") {
            assert_eq!(s, ["Firefox (Flatpak)", "Chromium (Snap)"]);
        }
    }
}
