use crate::state::{AppState, lock_or_recover};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const ID: &str = "main";

#[derive(Default)]
pub struct Tray {
    forced: AtomicBool,
    active: AtomicBool,
}

impl Tray {
    pub fn new(launched_minimized: bool) -> Tray {
        Tray { forced: AtomicBool::new(launched_minimized), active: AtomicBool::new(false) }
    }

    pub fn active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    pub fn unforce(&self) {
        self.forced.store(false, Ordering::SeqCst);
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn supported() -> bool {
    ["libayatana-appindicator3.so.1", "libappindicator3.so.1"].iter().any(|name| {
        let c = std::ffi::CString::new(*name).expect("no NUL");
        !unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_LAZY) }.is_null()
    })
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn supported() -> bool {
    true
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn host_available() -> bool {
    let bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some_and(|v| !v.is_empty())
        || std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|d| std::path::Path::new(&d).join("bus").exists());
    if !bus {
        return false;
    }
    std::process::Command::new("gdbus")
        .args(["call", "--session", "--timeout", "2", "--dest", "org.freedesktop.DBus", "--object-path", "/org/freedesktop/DBus"])
        .args(["--method", "org.freedesktop.DBus.NameHasOwner", "org.kde.StatusNotifierWatcher"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .is_ok_and(|o| o.status.success() && name_owned(&String::from_utf8_lossy(&o.stdout)))
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn host_available() -> bool {
    true
}

pub fn name_owned(reply: &str) -> bool {
    reply.trim() == "(true,)"
}

pub fn hides_on_close(app: &AppHandle) -> bool {
    let (Some(tray), Some(state)) = (app.try_state::<Tray>(), app.try_state::<Arc<AppState>>()) else { return false };
    if !tray.active() {
        return false;
    }
    let want = {
        let s = lock_or_recover(&state.store);
        if s.installed() { s.close_to_tray() } else { tray.forced.load(Ordering::SeqCst) }
    };
    want && host_available()
}

pub fn show_unless_hosted(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        for _ in 0..16 {
            if host_available() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(w) = h.get_webview_window("main") {
                let _ = w.show();
                let _ = w.minimize();
            }
        });
    });
}

pub fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn sync(app: &AppHandle) {
    let (Some(tray), Some(state)) = (app.try_state::<Tray>(), app.try_state::<Arc<AppState>>()) else { return };
    let want = supported() && {
        let s = lock_or_recover(&state.store);
        (s.installed() && (s.startup().1 || s.close_to_tray())) || tray.forced.load(Ordering::SeqCst)
    };
    if want == tray.active() {
        if want {
            refresh(app);
        }
        return;
    }
    if !want {
        app.remove_tray_by_id(ID);
        tray.active.store(false, Ordering::SeqCst);
        return;
    }
    let Ok(menu) = build_menu(app, &state) else { return };
    let mut b = TrayIconBuilder::with_id(ID)
        .tooltip("NiceVault")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| on_menu(app, e.id().as_ref()))
        .on_tray_icon_event(|t, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_window(t.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        b = b.icon(icon.clone());
    }
    if b.build(app).is_ok() {
        tray.active.store(true, Ordering::SeqCst);
    }
}

pub fn refresh(app: &AppHandle) {
    let active = app.try_state::<Tray>().is_some_and(|t| t.active());
    if !active {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let (Some(state), Some(t)) = (app.try_state::<Arc<AppState>>(), app.tray_by_id(ID)) else { return };
        if let Ok(menu) = build_menu(&app, &state) {
            let _ = t.set_menu(Some(menu));
        }
    });
}

fn on_menu(app: &AppHandle, id: &str) {
    let lock: Option<Option<String>> = match id {
        "open" => return show_window(app),
        "quit" => return app.exit(0),
        "lock-all" => Some(None),
        _ => id.strip_prefix("lock:").map(|v| Some(v.to_owned())),
    };
    if let (Some(which), Some(state)) = (lock, app.try_state::<Arc<AppState>>()) {
        let state = state.inner().clone();
        std::thread::spawn(move || match which {
            Some(v) => {
                let _ = state.vault_lock(&v, "user");
            }
            None => state.lock_all("user"),
        });
    }
}

fn build_menu(app: &AppHandle, state: &AppState) -> tauri::Result<Menu<tauri::Wry>> {
    let l = Labels::for_lang(&lock_or_recover(&state.store).language());
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(app, "open", l.open, true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let open = state.vault_list_open();
    for v in &open {
        let text = l.lock_one.replace("{name}", &v.name.replace('&', "&&"));
        menu.append(&MenuItem::with_id(app, format!("lock:{}", v.id), text, true, None::<&str>)?)?;
    }
    if open.is_empty() {
        menu.append(&MenuItem::with_id(app, "none", l.none_open, false, None::<&str>)?)?;
    }
    menu.append(&MenuItem::with_id(app, "lock-all", l.lock_all, !open.is_empty(), None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", l.quit, true, None::<&str>)?)?;
    Ok(menu)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Labels {
    pub open: &'static str,
    pub lock_one: &'static str,
    pub none_open: &'static str,
    pub lock_all: &'static str,
    pub quit: &'static str,
}

impl Labels {
    pub fn for_lang(lang: &str) -> Labels {
        let it = match lang {
            "it" => true,
            "en" => false,
            _ => system_lang().is_some_and(|l| l.to_ascii_lowercase().starts_with("it")),
        };
        if it {
            Labels { open: "Apri NiceVault", lock_one: "Blocca “{name}”", none_open: "Nessun volume aperto", lock_all: "Blocca tutto", quit: "Esci" }
        } else {
            Labels { open: "Open NiceVault", lock_one: "Lock “{name}”", none_open: "No volume open", lock_all: "Lock all", quit: "Quit" }
        }
    }
}

fn system_lang() -> Option<String> {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
        }
        let id = unsafe { GetUserDefaultUILanguage() };
        Some(if id & 0x3ff == 0x10 { "it".into() } else { "en".into() })
    }
    #[cfg(target_os = "macos")]
    {
        mac_lang()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.is_empty())
    }
}

#[cfg(target_os = "macos")]
fn mac_lang() -> Option<String> {
    use std::ffi::c_void;
    type CFTypeRef = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFLocaleCopyPreferredLanguages() -> CFTypeRef;
        fn CFArrayGetCount(a: CFTypeRef) -> isize;
        fn CFArrayGetValueAtIndex(a: CFTypeRef, i: isize) -> CFTypeRef;
        fn CFStringGetCString(s: CFTypeRef, buf: *mut u8, len: isize, encoding: u32) -> u8;
        fn CFRelease(o: CFTypeRef);
    }
    const UTF8: u32 = 0x0800_0100;
    unsafe {
        let a = CFLocaleCopyPreferredLanguages();
        if a.is_null() {
            return None;
        }
        let mut out = None;
        if CFArrayGetCount(a) > 0 {
            let s = CFArrayGetValueAtIndex(a, 0);
            let mut buf = [0u8; 64];
            if !s.is_null() && CFStringGetCString(s, buf.as_mut_ptr(), buf.len() as isize, UTF8) != 0 {
                let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                out = Some(String::from_utf8_lossy(&buf[..end]).into_owned());
            }
        }
        CFRelease(a);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_owner_reply() {
        assert!(name_owned("(true,)\n"));
        assert!(!name_owned("(false,)\n"));
        assert!(!name_owned(""));
    }

    #[test]
    fn labels_follow_the_setting() {
        assert_eq!(Labels::for_lang("it").quit, "Esci");
        assert_eq!(Labels::for_lang("en").open, "Open NiceVault");
        assert!(Labels::for_lang("it").lock_one.contains("{name}"));
    }
}
