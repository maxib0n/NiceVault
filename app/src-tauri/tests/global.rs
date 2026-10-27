use nicevault_app::global::AppSettingsPatch;
use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, NoEvents};
use secrecy::SecretString;
use std::path::Path;
use std::sync::Arc;

fn app(loc: Locations) -> Arc<AppState> {
    AppState::new(Store::new(loc), Box::new(NoEvents))
}

fn patch(json: &str) -> AppSettingsPatch {
    serde_json::from_str(json).unwrap()
}

fn all_files(dir: &Path) -> Vec<String> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() { out.extend(all_files(&p)) } else { out.push(p.display().to_string()) }
    }
    out.sort();
    out
}

#[test]
fn installed_persists_language_and_defaults() {
    let t = tempfile::tempdir().unwrap();
    let cfg = t.path().join("cfg");
    let loc = || Locations { exe_dir: Some(t.path().join("exe")), removable: Some(vec![]), config_dir: Some(cfg.clone()) };
    let st = app(loc());
    let a = st.app_settings_get();
    assert_eq!((a.language.as_str(), a.persisted, a.defaults.accent.as_str()), ("system", true, "velluto"));
    let a = st
        .app_settings_set(patch(r#"{"language": "en", "new_like_app": false, "defaults": {"accent": "prussia", "theme": "light", "auto_lock_minutes": 0, "lock_on_sleep": false}}"#))
        .unwrap();
    assert_eq!((a.language.as_str(), a.defaults.accent.as_str(), a.defaults.auto_lock_minutes), ("en", "prussia", 0));
    assert_eq!(st.app_settings_set(patch(r#"{"language": "de"}"#)).unwrap().language, "en", "unknown language ignored");

    let st2 = app(loc());
    let b = st2.app_settings_get();
    assert_eq!((b.language.as_str(), b.defaults.theme.as_str(), b.defaults.lock_on_sleep), ("en", "light", false));

    std::fs::create_dir_all(t.path().join("exe")).unwrap();
    let v = st2.vault_create(t.path().join("exe/N.nvault").to_str().unwrap(), SecretString::from("p".to_owned()), Some("fast")).unwrap();
    let s = st2.settings_get(Some(&v.id)).unwrap();
    assert_eq!((s.accent.as_str(), s.theme.as_str(), s.auto_lock_minutes), ("prussia", "light", 0));
    let info = st2.app_info();
    assert!(!info.portable && info.config_dir.is_some() && !info.version.is_empty());
}

#[test]
fn portable_keeps_them_in_memory_only() {
    let t = tempfile::tempdir().unwrap();
    let stick = t.path().join("stick");
    std::fs::create_dir_all(&stick).unwrap();
    let loc = || Locations { exe_dir: Some(stick.clone()), removable: Some(vec![]), config_dir: None };
    let st = app(loc());
    let a = st.app_settings_set(patch(r#"{"language": "it", "defaults": {"accent": "seppia"}}"#)).unwrap();
    assert_eq!((a.language.as_str(), a.persisted), ("it", false));
    assert!(st.app_info().portable);
    assert!(all_files(t.path()).is_empty(), "nothing written: {:?}", all_files(t.path()));
    let st2 = app(loc());
    assert_eq!(st2.app_settings_get().language, "system", "a new session starts clean");
}

#[test]
fn settings_json_keeps_unknown_keys_and_never_overwrites_what_it_cannot_read() {
    let t = tempfile::tempdir().unwrap();
    let cfg = t.path().join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    let loc = || Locations { exe_dir: None, removable: Some(vec![]), config_dir: Some(cfg.clone()) };
    std::fs::write(
        cfg.join("settings.json"),
        r#"{"language":"it","future":{"x":1},"defaults":{"accent":"prugna","future_default":true}}"#,
    )
    .unwrap();
    let st = app(loc());
    assert_eq!(st.app_settings_get().defaults.accent, "prugna");
    st.app_settings_set(patch(r#"{"defaults": {"theme": "light"}}"#)).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(cfg.join("settings.json")).unwrap()).unwrap();
    assert_eq!(v["future"]["x"], 1);
    assert_eq!(v["defaults"]["future_default"], true);
    assert_eq!((v["defaults"]["theme"].as_str(), v["language"].as_str()), (Some("light"), Some("it")));

    std::fs::write(cfg.join("settings.json"), b"[\"not an object\"]").unwrap();
    let st = app(loc());
    let e = st.app_settings_set(patch(r#"{"language": "en"}"#)).unwrap_err();
    assert_eq!(e.code, "unsupported_version");
    assert_eq!(std::fs::read(cfg.join("settings.json")).unwrap(), b"[\"not an object\"]");
}

#[test]
fn app_look_persists_and_new_volumes_wear_it_when_like_app() {
    let t = tempfile::tempdir().unwrap();
    let cfg = t.path().join("cfg");
    let exe = t.path().join("exe");
    std::fs::create_dir_all(&exe).unwrap();
    let loc = || Locations { exe_dir: Some(exe.clone()), removable: Some(vec![]), config_dir: Some(cfg.clone()) };
    let st = app(loc());
    let a = st.app_settings_get();
    assert_eq!((a.app_theme.as_str(), a.app_accent.as_str(), a.ui_scale, a.new_like_app), ("dark", "velluto", 100, true));
    let a = st
        .app_settings_set(patch(r#"{"app_theme": "light", "app_accent": "ottanio", "ui_scale": 125, "defaults": {"accent": "prussia", "theme": "dark"}}"#))
        .unwrap();
    assert_eq!((a.app_theme.as_str(), a.app_accent.as_str(), a.ui_scale), ("light", "ottanio", 125));
    let a = st.app_settings_set(patch(r#"{"app_theme": "neon", "app_accent": "oro", "ui_scale": 300}"#)).unwrap();
    assert_eq!((a.app_theme.as_str(), a.app_accent.as_str(), a.ui_scale), ("light", "ottanio", 125));

    let v = st.vault_create(exe.join("A.nvault").to_str().unwrap(), SecretString::from("p".to_owned()), Some("fast")).unwrap();
    let s = st.settings_get(Some(&v.id)).unwrap();
    assert_eq!((s.accent.as_str(), s.theme.as_str()), ("ottanio", "light"));
    st.app_settings_set(patch(r#"{"new_like_app": false}"#)).unwrap();
    let v = st.vault_create(exe.join("B.nvault").to_str().unwrap(), SecretString::from("p".to_owned()), Some("fast")).unwrap();
    let s = st.settings_get(Some(&v.id)).unwrap();
    assert_eq!((s.accent.as_str(), s.theme.as_str()), ("prussia", "dark"));

    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(cfg.join("settings.json")).unwrap()).unwrap();
    assert_eq!((raw["app_theme"].as_str(), raw["ui_scale"].as_u64(), raw["new_like_app"].as_bool()), (Some("light"), Some(125), Some(false)));
    let mut obj = raw.as_object().unwrap().clone();
    obj.insert("from_the_future".into(), serde_json::json!(1));
    std::fs::write(cfg.join("settings.json"), serde_json::to_vec(&obj).unwrap()).unwrap();
    let st2 = app(loc());
    let b = st2.app_settings_get();
    assert_eq!((b.app_theme.as_str(), b.app_accent.as_str(), b.ui_scale, b.new_like_app), ("light", "ottanio", 125, false));
    st2.app_settings_set(patch(r#"{"ui_scale": 90}"#)).unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(cfg.join("settings.json")).unwrap()).unwrap();
    assert_eq!((raw["ui_scale"].as_u64(), raw["from_the_future"].as_u64()), (Some(90), Some(1)));
}

#[test]
fn sounds_default_on_and_persist_additively() {
    let t = tempfile::tempdir().unwrap();
    let cfg = t.path().join("cfg");
    let exe = t.path().join("exe");
    std::fs::create_dir_all(&exe).unwrap();
    let loc = || Locations { exe_dir: Some(exe.clone()), removable: Some(vec![]), config_dir: Some(cfg.clone()) };
    let st = app(loc());
    assert!(st.app_settings_get().sounds);
    st.app_settings_set(patch(r#"{"sounds": false}"#)).unwrap();
    assert!(!app(loc()).app_settings_get().sounds);
}
