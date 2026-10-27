use nicevault_app::global::AppSettingsPatch;
use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, NoEvents};

#[test]
fn sidebar_collapsed_persists_additively() {
    let t = tempfile::tempdir().unwrap();
    let cfg = t.path().join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("settings.json"), br#"{ "language": "it", "from_the_future": 7 }"#).unwrap();
    let loc = || Locations { exe_dir: None, removable: Some(vec![]), config_dir: Some(cfg.clone()) };
    let st = AppState::new(Store::new(loc()), Box::new(NoEvents));
    assert!(!st.app_settings_get().sidebar_collapsed, "default off");

    let patch: AppSettingsPatch = serde_json::from_str(r#"{"sidebar_collapsed": true}"#).unwrap();
    assert!(st.app_settings_set(patch).unwrap().sidebar_collapsed);
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(cfg.join("settings.json")).unwrap()).unwrap();
    assert_eq!(raw["sidebar_collapsed"].as_bool(), Some(true));
    assert_eq!(raw["from_the_future"].as_i64(), Some(7), "unknown keys survive");

    let again = AppState::new(Store::new(loc()), Box::new(NoEvents));
    assert!(again.app_settings_get().sidebar_collapsed);
}
