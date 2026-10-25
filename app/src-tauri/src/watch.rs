//! Auto-lock triggers from the OS: the drive holding a volume disappears, the system goes to
//! sleep, the session's screen locks. Which volumes lock is decided per volume by
//! `AppState::lock_event` (autolock.rs); drive removal always locks just that volume.

use crate::state::{AppState, lock_or_recover};
use std::path::{Path, PathBuf};
use std::sync::Weak;
use std::time::{Duration, Instant, SystemTime};

fn reachable(path: &Path) -> bool {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::read_dir(dir).is_ok() && std::fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// Checks every second that the volume file is still there; locks with "removed" if not.
pub fn spawn_removal_watcher(state: Weak<AppState>, id: String, path: PathBuf) {
    let _ = std::thread::Builder::new().name("nv-removal".into()).spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let Some(s) = state.upgrade() else { return };
            if !lock_or_recover(&s.vaults).contains_key(&id) {
                return;
            }
            if !reachable(&path) {
                let _ = s.vault_lock(&id, "removed");
                return;
            }
        }
    });
}

/// The app state the native callbacks (Windows window procedure, macOS C callbacks) reach.
#[cfg(any(windows, target_os = "macos"))]
static TRIGGER_STATE: std::sync::OnceLock<Weak<AppState>> = std::sync::OnceLock::new();

#[cfg(any(windows, target_os = "macos"))]
fn fire(reason: &str) {
    if let Some(s) = TRIGGER_STATE.get().and_then(Weak::upgrade) {
        s.lock_event(reason);
    }
}

/// Starts the sleep and screen-lock triggers ([`AppState::lock_event`] "sleep" / "screen"):
///  - Linux: `gdbus monitor` children (no D-Bus crate): logind's `PrepareForSleep(true)`
///    (before suspend), the session's `Lock` signal / `LockedHint = true`, and the session
///    bus screensavers' `ActiveChanged(true)` (org.freedesktop.ScreenSaver: KDE, Xfce, ...;
///    org.gnome.ScreenSaver: GNOME);
///  - Windows: a hidden top-level window: `WM_POWERBROADCAST / PBT_APMSUSPEND` (before sleep)
///    and `WM_WTSSESSION_CHANGE / WTS_SESSION_LOCK` (WTSRegisterSessionNotification);
///  - macOS: `IORegisterForSystemPower` `kIOMessageSystemWillSleep` (before sleep; the C side
///    of NSWorkspaceWillSleepNotification) and the distributed notification
///    `com.apple.screenIsLocked`;
///  - everywhere: a wall-clock vs monotonic-clock gap after resume (the monotonic clock stops
///    during suspend on Linux and macOS), as a fallback "sleep".
pub fn spawn_sleep_watcher(state: Weak<AppState>) {
    #[cfg(target_os = "linux")]
    linux::spawn(state.clone());
    #[cfg(any(windows, target_os = "macos"))]
    let _ = TRIGGER_STATE.set(state.clone());
    #[cfg(windows)]
    win::spawn();
    #[cfg(target_os = "macos")]
    mac::spawn();
    let _ = std::thread::Builder::new().name("nv-sleep".into()).spawn(move || {
        let (mut wall, mut mono) = (SystemTime::now(), Instant::now());
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let (w, m) = (SystemTime::now(), Instant::now());
            let wall_d = w.duration_since(wall).unwrap_or_default();
            let mono_d = m.duration_since(mono);
            (wall, mono) = (w, m);
            let Some(s) = state.upgrade() else { return };
            if wall_d > mono_d + Duration::from_secs(20) && !lock_or_recover(&s.vaults).is_empty() {
                s.lock_event("sleep");
            }
        }
    });
}

/// sd_bus_path_encode: every byte that is not `[A-Za-z0-9]` (and a leading digit) becomes `_xx`.
pub fn bus_path_escape(s: &str) -> String {
    if s.is_empty() {
        return "_".into();
    }
    let mut out = String::new();
    for (i, b) in s.bytes().enumerate() {
        if b.is_ascii_alphabetic() || (b.is_ascii_digit() && i > 0) {
            out.push(b as char);
        } else {
            out.push_str(&format!("_{b:02x}"));
        }
    }
    out
}

/// What one `gdbus monitor` line means for auto-lock: `Some("sleep")`, `Some("screen")` or
/// nothing. `session` = this process's logind session object path (`None` = unknown: any
/// session's lock counts, the safe direction).
pub fn classify_dbus_line(line: &str, session: Option<&str>) -> Option<&'static str> {
    let ours = || session.is_none_or(|p| line.starts_with(p) && line[p.len()..].starts_with(':'));
    if line.contains(".PrepareForSleep (true") {
        return Some("sleep");
    }
    let session_locked = line.contains("org.freedesktop.login1.Session.Lock ")
        || (line.contains("org.freedesktop.login1.Session") && line.contains("'LockedHint': <true>"));
    let screensaver_on = line.contains("ScreenSaver.ActiveChanged (true");
    ((session_locked && ours()) || screensaver_on).then_some("screen")
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::io::BufRead;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};

    pub(super) static MONITORS: std::sync::Mutex<Vec<Child>> = std::sync::Mutex::new(Vec::new());

    pub(super) fn spawn(state: Weak<AppState>) {
        let session = std::env::var("XDG_SESSION_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|id| format!("/org/freedesktop/login1/session/{}", bus_path_escape(&id)));
        monitor(&["--system", "--dest", "org.freedesktop.login1"], state.clone(), session.clone());
        monitor(&["--session", "--dest", "org.freedesktop.ScreenSaver"], state.clone(), session.clone());
        monitor(&["--session", "--dest", "org.gnome.ScreenSaver"], state, session);
    }

    fn monitor(args: &[&str], state: Weak<AppState>, session: Option<String>) {
        let mut cmd = Command::new("gdbus");
        cmd.arg("monitor").args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        // SAFETY: prctl is async-signal-safe; the child dies with us even if we abort.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
        let Ok(mut child) = cmd.spawn() else { return };
        let Some(out) = child.stdout.take() else { return };
        lock_or_recover(&MONITORS).push(child);
        let _ = std::thread::Builder::new().name("nv-dbus".into()).spawn(move || {
            for line in std::io::BufReader::new(out).lines() {
                let Ok(line) = line else { return };
                if let Some(reason) = classify_dbus_line(&line, session.as_deref()) {
                    let Some(s) = state.upgrade() else { return };
                    s.lock_event(reason);
                }
            }
        });
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    type Hwnd = *mut c_void;
    type WndProc = unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize;

    #[repr(C)]
    struct WndClassW {
        style: u32,
        wnd_proc: Option<WndProc>,
        cls_extra: i32,
        wnd_extra: i32,
        instance: *mut c_void,
        icon: *mut c_void,
        cursor: *mut c_void,
        background: *mut c_void,
        menu_name: *const u16,
        class_name: *const u16,
    }

    #[repr(C)]
    struct Msg {
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt: [i32; 2],
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn RegisterClassW(c: *const WndClassW) -> u16;
        #[allow(clippy::too_many_arguments)]
        fn CreateWindowExW(
            ex_style: u32,
            class: *const u16,
            name: *const u16,
            style: u32,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            parent: Hwnd,
            menu: *mut c_void,
            instance: *mut c_void,
            param: *mut c_void,
        ) -> Hwnd;
        fn DefWindowProcW(h: Hwnd, m: u32, w: usize, l: isize) -> isize;
        fn GetMessageW(m: *mut Msg, h: Hwnd, min: u32, max: u32) -> i32;
        fn TranslateMessage(m: *const Msg) -> i32;
        fn DispatchMessageW(m: *const Msg) -> isize;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    }
    #[link(name = "wtsapi32")]
    unsafe extern "system" {
        fn WTSRegisterSessionNotification(h: Hwnd, flags: u32) -> i32;
    }

    const WM_POWERBROADCAST: u32 = 0x0218;
    const PBT_APMSUSPEND: usize = 0x0004;
    const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
    const WTS_SESSION_LOCK: usize = 0x7;
    const NOTIFY_FOR_THIS_SESSION: u32 = 0;
    const WM_DEVICECHANGE: u32 = 0x0219;
    const DBT_DEVICEARRIVAL: usize = 0x8000;
    const DBT_DEVICEREMOVECOMPLETE: usize = 0x8004;

    unsafe extern "system" fn wnd_proc(h: Hwnd, m: u32, w: usize, l: isize) -> isize {
        match (m, w) {
            // Runs before the system suspends (the lock completes inside the callback).
            (WM_POWERBROADCAST, PBT_APMSUSPEND) => {
                super::fire("sleep");
                1
            }
            (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK) => {
                super::fire("screen");
                0
            }
            // a volume (drive letter) arrived or went: drives.rs re-enumerates
            (WM_DEVICECHANGE, DBT_DEVICEARRIVAL | DBT_DEVICEREMOVECOMPLETE) => {
                crate::drives::wake();
                1
            }
            // SAFETY: forwarding the arguments the system gave us.
            _ => unsafe { DefWindowProcW(h, m, w, l) },
        }
    }

    pub(super) fn spawn() {
        let _ = std::thread::Builder::new().name("nv-oswatch".into()).spawn(|| {
            let class: Vec<u16> = "NiceVaultAutoLock".encode_utf16().chain([0]).collect();
            // SAFETY: plain Win32 calls with NUL-terminated strings that outlive them; the
            // window is never shown (top-level so it gets WM_POWERBROADCAST, which
            // message-only windows do not) and lives as long as this thread.
            unsafe {
                let instance = GetModuleHandleW(std::ptr::null());
                let wc = WndClassW {
                    style: 0,
                    wnd_proc: Some(wnd_proc),
                    cls_extra: 0,
                    wnd_extra: 0,
                    instance,
                    icon: std::ptr::null_mut(),
                    cursor: std::ptr::null_mut(),
                    background: std::ptr::null_mut(),
                    menu_name: std::ptr::null(),
                    class_name: class.as_ptr(),
                };
                RegisterClassW(&wc);
                let null = std::ptr::null_mut();
                let hwnd = CreateWindowExW(0, class.as_ptr(), class.as_ptr(), 0, 0, 0, 0, 0, null, null, instance, null);
                if hwnd.is_null() {
                    return;
                }
                WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
                let mut msg: Msg = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicU32, Ordering};

    type CfRef = *const c_void;
    type NotifyCb = extern "C" fn(CfRef, *mut c_void, CfRef, *const c_void, CfRef);
    type PowerCb = extern "C" fn(*mut c_void, u32, u32, *mut c_void);

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFNotificationCenterGetDistributedCenter() -> CfRef;
        fn CFNotificationCenterAddObserver(center: CfRef, observer: *const c_void, cb: NotifyCb, name: CfRef, object: *const c_void, behavior: isize);
        fn CFStringCreateWithCString(alloc: CfRef, s: *const c_char, encoding: u32) -> CfRef;
        fn CFRunLoopGetCurrent() -> CfRef;
        fn CFRunLoopAddSource(rl: CfRef, source: CfRef, mode: CfRef);
        fn CFRunLoopRun();
        static kCFRunLoopDefaultMode: CfRef;
    }
    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IORegisterForSystemPower(refcon: *mut c_void, port: *mut *mut c_void, cb: PowerCb, notifier: *mut u32) -> u32;
        fn IONotificationPortGetRunLoopSource(port: *mut c_void) -> CfRef;
        fn IOAllowPowerChange(kernel_port: u32, notification_id: isize) -> i32;
    }

    const UTF8: u32 = 0x0800_0100;
    const DELIVER_IMMEDIATELY: isize = 4;
    const SYSTEM_WILL_SLEEP: u32 = 0xE000_0280; // kIOMessageSystemWillSleep
    const CAN_SYSTEM_SLEEP: u32 = 0xE000_0270; // kIOMessageCanSystemSleep
    static ROOT_PORT: AtomicU32 = AtomicU32::new(0);
    static OBSERVER: u8 = 0;

    extern "C" fn on_screen_locked(_: CfRef, _: *mut c_void, _: CfRef, _: *const c_void, _: CfRef) {
        super::fire("screen");
    }

    extern "C" fn on_power(_: *mut c_void, _: u32, msg: u32, arg: *mut c_void) {
        match msg {
            SYSTEM_WILL_SLEEP => {
                super::fire("sleep"); // done before we allow the sleep
                // SAFETY: acknowledging the notification IOKit just sent us.
                unsafe { IOAllowPowerChange(ROOT_PORT.load(Ordering::Relaxed), arg as isize) };
            }
            // SAFETY: as above (idle sleep asks first; never veto it).
            CAN_SYSTEM_SLEEP => unsafe {
                IOAllowPowerChange(ROOT_PORT.load(Ordering::Relaxed), arg as isize);
            },
            _ => {}
        }
    }

    pub(super) fn spawn() {
        let _ = std::thread::Builder::new().name("nv-oswatch".into()).spawn(|| {
            // SAFETY: CoreFoundation / IOKit C calls with valid arguments; the string and the
            // port live for the whole process; the run loop never returns while sources exist.
            unsafe {
                let name = CFStringCreateWithCString(std::ptr::null(), c"com.apple.screenIsLocked".as_ptr(), UTF8);
                let observer = &OBSERVER as *const u8 as *const c_void;
                CFNotificationCenterAddObserver(
                    CFNotificationCenterGetDistributedCenter(),
                    observer,
                    on_screen_locked,
                    name,
                    std::ptr::null(),
                    DELIVER_IMMEDIATELY,
                );
                let mut port: *mut c_void = std::ptr::null_mut();
                let mut notifier = 0u32;
                let root = IORegisterForSystemPower(std::ptr::null_mut(), &mut port, on_power, &mut notifier);
                if root != 0 && !port.is_null() {
                    ROOT_PORT.store(root, Ordering::Relaxed);
                    CFRunLoopAddSource(CFRunLoopGetCurrent(), IONotificationPortGetRunLoopSource(port), kCFRunLoopDefaultMode);
                }
                CFRunLoopRun();
            }
        });
    }
}

/// Stops helper processes (app exit).
pub fn shutdown() {
    #[cfg(target_os = "linux")]
    for mut c in lock_or_recover(&linux::MONITORS).drain(..) {
        let _ = c.kill();
        let _ = c.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dbus_lines() {
        assert_eq!(bus_path_escape("2"), "_32");
        assert_eq!(bus_path_escape("c12"), "c12");
        let me = "/org/freedesktop/login1/session/_32";
        let c = |l: &str| classify_dbus_line(l, Some(me));
        assert_eq!(c("/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (true,)"), Some("sleep"));
        assert_eq!(c("/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (false,)"), None);
        assert_eq!(c("/org/freedesktop/login1/session/_32: org.freedesktop.login1.Session.Lock ()"), Some("screen"));
        assert_eq!(c("/org/freedesktop/login1/session/_32: org.freedesktop.login1.Session.Unlock ()"), None);
        assert_eq!(c("/org/freedesktop/login1/session/_35: org.freedesktop.login1.Session.Lock ()"), None, "another session");
        assert_eq!(classify_dbus_line("/org/freedesktop/login1/session/_35: org.freedesktop.login1.Session.Lock ()", None), Some("screen"));
        assert_eq!(
            c("/org/freedesktop/login1/session/_32: org.freedesktop.DBus.Properties.PropertiesChanged ('org.freedesktop.login1.Session', {'LockedHint': <true>}, @as [])"),
            Some("screen")
        );
        assert_eq!(
            c("/org/freedesktop/login1/session/_32: org.freedesktop.DBus.Properties.PropertiesChanged ('org.freedesktop.login1.Session', {'LockedHint': <false>}, @as [])"),
            None
        );
        assert_eq!(c("/ScreenSaver: org.freedesktop.ScreenSaver.ActiveChanged (true,)"), Some("screen"));
        assert_eq!(c("/org/gnome/ScreenSaver: org.gnome.ScreenSaver.ActiveChanged (true,)"), Some("screen"));
        assert_eq!(c("/org/gnome/ScreenSaver: org.gnome.ScreenSaver.ActiveChanged (false,)"), None);
    }
}
