use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, NoEvents};

#[test]
fn gallery_save_asked_once_then_kept() {
    let cfg = tempfile::tempdir().unwrap();
    let loc = || Locations { exe_dir: None, removable: Some(vec![]), config_dir: Some(cfg.path().to_path_buf()) };
    let st = AppState::new(Store::new(loc()), Box::new(NoEvents));
    assert_eq!(st.app_settings_get().gallery_save, None, "never asked");
    st.app_settings_set(serde_json::from_str(r#"{ "site_icons": false }"#).unwrap()).unwrap();
    let raw = std::fs::read_to_string(cfg.path().join("settings.json")).unwrap();
    assert!(!raw.contains("gallery_save"), "not written while unanswered");
    st.app_settings_set(serde_json::from_str(r#"{ "gallery_save": false }"#).unwrap()).unwrap();
    assert_eq!(st.app_settings_get().gallery_save, Some(false));
    drop(st);
    let again = AppState::new(Store::new(loc()), Box::new(NoEvents));
    assert_eq!(again.app_settings_get().gallery_save, Some(false), "kept across starts");
}
