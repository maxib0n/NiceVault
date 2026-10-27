use nicevault_app::autostart::{self, Autostart, Target};
use nicevault_app::global::AppSettingsPatch;
use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, NoEvents};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn patch(json: &str) -> AppSettingsPatch {
    serde_json::from_str(json).unwrap()
}

fn installed(cfg: &Path) -> Locations {
    Locations { exe_dir: None, removable: Some(vec![]), config_dir: Some(cfg.to_path_buf()) }
}

fn app(store: Store) -> Arc<AppState> {
    AppState::new(store, Box::new(NoEvents))
}

fn entry(dir: &Path, exe: &str) -> Autostart {
    Autostart { target: Target::Desktop(dir.join("autostart/nicevault.desktop")), exe: PathBuf::from(exe) }
}

fn exec_line(file: &Path) -> String {
    let body = std::fs::read_to_string(file).unwrap();
    body.lines().find_map(|l| l.strip_prefix("Exec=")).unwrap().to_owned()
}

fn exec_args(exec: &str) -> Vec<String> {
    let unescaped = exec.replace("\\\\", "\\");
    let (mut out, mut cur, mut quoted, mut chars) = (vec![], String::new(), false, unescaped.chars());
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' if quoted => cur.extend(chars.next()),
            ' ' if !quoted => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    out.push(cur);
    out
}

#[test]
fn toggles_write_the_entry_and_persist() {
    let t = tempfile::tempdir().unwrap();
    let cfg = t.path().join("cfg");
    let file = t.path().join("autostart/nicevault.desktop");
    let st = app(Store::new(installed(&cfg)).with_autostart(Some(entry(t.path(), "/opt/Nice Vault/nicevault"))));
    let a = st.app_settings_get();
    assert_eq!((a.start_at_login, a.start_minimized), (false, false));

    let a = st.app_settings_set(patch(r#"{"start_minimized": true}"#)).unwrap();
    assert_eq!((a.start_at_login, a.start_minimized), (false, true));
    assert!(!file.exists());

    st.app_settings_set(patch(r#"{"start_at_login": true}"#)).unwrap();
    let args = exec_args(&exec_line(&file));
    assert_eq!(args, ["/opt/Nice Vault/nicevault", "--minimized"]);
    assert!(autostart::launched_minimized(&args), "the binary started by the entry starts hidden");

    st.app_settings_set(patch(r#"{"start_minimized": false}"#)).unwrap();
    let args = exec_args(&exec_line(&file));
    assert_eq!(args, ["/opt/Nice Vault/nicevault"]);
    assert!(!autostart::launched_minimized(&args));

    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(cfg.join("settings.json")).unwrap()).unwrap();
    assert_eq!((raw["start_at_login"].as_bool(), raw["start_minimized"].as_bool()), (Some(true), Some(false)));

    let a = st.app_settings_set(patch(r#"{"start_at_login": false}"#)).unwrap();
    assert!(!a.start_at_login);
    assert!(!file.exists());
}

#[test]
fn a_new_run_reads_them_back_and_follows_the_entry() {
    let t = tempfile::tempdir().unwrap();
    let cfg = t.path().join("cfg");
    let file = t.path().join("autostart/nicevault.desktop");
    let st = app(Store::new(installed(&cfg)).with_autostart(Some(entry(t.path(), "/old/nicevault"))));
    st.app_settings_set(patch(r#"{"start_at_login": true, "start_minimized": true}"#)).unwrap();

    let mut s = Store::new(installed(&cfg)).with_autostart(Some(entry(t.path(), "/new/nicevault")));
    s.startup_check();
    assert_eq!(s.startup(), (true, true));
    assert_eq!(exec_args(&exec_line(&file)), ["/new/nicevault", "--minimized"]);

    std::fs::remove_file(&file).unwrap();
    let mut s = Store::new(installed(&cfg)).with_autostart(Some(entry(t.path(), "/new/nicevault")));
    s.startup_check();
    assert_eq!(s.startup(), (false, true));
    assert!(!file.exists(), "not recreated");
    assert_eq!(Store::new(installed(&cfg)).startup(), (false, true));
}

#[test]
fn a_failed_write_changes_nothing() {
    let t = tempfile::tempdir().unwrap();
    let cfg = t.path().join("cfg");
    std::fs::write(t.path().join("autostart"), b"").unwrap();
    let st = app(Store::new(installed(&cfg)).with_autostart(Some(entry(t.path(), "/opt/nv"))));
    let e = st.app_settings_set(patch(r#"{"start_at_login": true}"#)).unwrap_err();
    assert_eq!(e.code, "autostart_failed");
    assert!(!st.app_settings_get().start_at_login);
    assert!(!cfg.join("settings.json").exists());
}

#[test]
fn keys_of_a_newer_build_survive() {
    let t = tempfile::tempdir().unwrap();
    let cfg = t.path().join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("settings.json"), r#"{"start_minimized":true,"start_hidden_mode":"tray"}"#).unwrap();
    let st = app(Store::new(installed(&cfg)).with_autostart(Some(entry(t.path(), "/opt/nv"))));
    assert!(st.app_settings_get().start_minimized);
    st.app_settings_set(patch(r#"{"start_at_login": true}"#)).unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(cfg.join("settings.json")).unwrap()).unwrap();
    assert_eq!((raw["start_at_login"].as_bool(), raw["start_hidden_mode"].as_str()), (Some(true), Some("tray")));
}

#[test]
fn portable_ignores_them_and_writes_nothing() {
    let t = tempfile::tempdir().unwrap();
    let stick = t.path().join("stick");
    std::fs::create_dir_all(&stick).unwrap();
    let loc = Locations { exe_dir: Some(stick.clone()), removable: Some(vec![]), config_dir: None };
    let st = app(Store::new(loc).with_autostart(Some(entry(&t.path().join("host"), "/media/stick/nicevault"))));
    let a = st.app_settings_set(patch(r#"{"start_at_login": true, "start_minimized": true}"#)).unwrap();
    assert_eq!((a.start_at_login, a.start_minimized, a.persisted), (false, false, false));
    assert!(!t.path().join("host").exists());
    assert_eq!(std::fs::read_dir(&stick).unwrap().count(), 0);
}

#[test]
fn the_override_folder_gets_this_platforms_entry() {
    let t = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("NICEVAULT_AUTOSTART_DIR", t.path()) };
    let a = Autostart::detect().expect("an entry");
    assert_eq!(a.exe, std::env::current_exe().unwrap());
    let expected = if cfg!(windows) {
        Target::RunFile(t.path().join("nicevault.run"))
    } else if cfg!(target_os = "macos") {
        Target::LaunchAgent(t.path().join("app.nicevault.desktop.plist"))
    } else {
        Target::Desktop(t.path().join("nicevault.desktop"))
    };
    assert_eq!(a.target, expected);
    a.apply(true, true).unwrap();
    assert!(a.exists());
    a.apply(false, true).unwrap();
    assert!(!a.exists());
}

#[test]
fn close_to_tray_defaults_on_installed_and_persists_additively() {
    let t = tempfile::tempdir().unwrap();
    let cfg = t.path().join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("settings.json"), br#"{ "language": "it", "from_the_future": 7 }"#).unwrap();
    let st = app(Store::new(installed(&cfg)));
    assert!(st.app_settings_get().close_to_tray, "default on when installed");

    let a = st.app_settings_set(patch(r#"{"close_to_tray": false}"#)).unwrap();
    assert!(!a.close_to_tray);
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(cfg.join("settings.json")).unwrap()).unwrap();
    assert_eq!(raw["close_to_tray"].as_bool(), Some(false));
    assert_eq!(raw["from_the_future"].as_i64(), Some(7), "unknown keys survive");
    assert_eq!(raw["language"].as_str(), Some("it"));
    assert_eq!((a.start_at_login, a.start_minimized), (false, false));

    let again = app(Store::new(installed(&cfg)));
    assert!(!again.app_settings_get().close_to_tray);
    again.app_settings_set(patch(r#"{"close_to_tray": true}"#)).unwrap();
    assert!(app(Store::new(installed(&cfg))).app_settings_get().close_to_tray);
}

#[test]
fn close_to_tray_is_off_when_portable() {
    let st = app(Store::new(Locations { exe_dir: None, removable: Some(vec![]), config_dir: None }));
    assert!(!st.app_settings_get().close_to_tray);
    let a = st.app_settings_set(patch(r#"{"close_to_tray": true}"#)).unwrap();
    assert!(!a.close_to_tray, "portable: nothing kept, closing quits");
}
