use nicevault_app::appearance::Visibility;
use nicevault_app::emblem::Emblem;
use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, NoEvents};
use nv_core::{Appearance, KdfPreset, LockedEmblem, Vault};
use secrecy::SecretString;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn app(dir: &Path) -> Arc<AppState> {
    AppState::new(
        Store::new(Locations { exe_dir: Some(dir.to_path_buf()), removable: Some(vec![]), config_dir: None }),
        Box::new(NoEvents),
    )
}

fn patch(j: Value) -> nicevault_app::appearance::SettingsPatch {
    serde_json::from_value(j).unwrap()
}

fn newer_volume(dir: &Path, meta: &[(&str, &[u8])], header: Option<Appearance>) -> PathBuf {
    let p = dir.join("nuovo.nvault");
    let v = Vault::create(&p, &pw("pw"), KdfPreset::Fast).unwrap();
    for (k, b) in meta {
        v.meta_set(k, Some(b)).unwrap();
    }
    v.set_appearance(header.as_ref()).unwrap();
    v.close().unwrap();
    p
}

fn meta(p: &Path, k: &str) -> Option<Vec<u8>> {
    let v = Vault::open(p, &pw("pw")).unwrap();
    let r = v.meta_get(k).unwrap();
    v.close().unwrap();
    r
}

fn json_of(b: Option<Vec<u8>>) -> Value {
    serde_json::from_slice(&b.unwrap()).unwrap()
}

#[test]
fn unknown_emblem_kind_is_neither_shown_nor_rewritten() {
    let t = tempfile::tempdir().unwrap();
    let rec = br#"{"emblem":{"kind":"hologram","seed":7},"show_when_locked":true,"frame":"gold"}"#;
    let header = Appearance { accent: Some("prussia".into()), emblem: Some(LockedEmblem::Icon("hologram-7".into())) };
    let p = newer_volume(t.path(), &[("nicevault.emblem", rec)], Some(header.clone()));
    let st = app(t.path());

    let info = st.vault_open(p.to_str().unwrap(), pw("pw")).unwrap();
    assert_eq!((info.emblem.clone(), info.show_when_locked, info.visibility), (Emblem::Initial, false, Visibility::Encrypted), "neutral, never visible");
    let a = st.vault_appearance_get(&info.id).unwrap();
    assert_eq!(a.visibility, Visibility::Encrypted);
    st.settings_set(patch(json!({"theme": "light", "accent": "seppia"})), Some(&info.id)).unwrap();
    let e = st.vault_appearance_set(&info.id, None, None, Some(Visibility::Encrypted), None, None).unwrap_err();
    assert_eq!(e.code, "unsupported_version");
    st.vault_lock(&info.id, "user").unwrap();
    assert_eq!(nv_core::peek_appearance(&p).unwrap(), Some(header), "header section rewritten");
    assert_eq!(meta(&p, "nicevault.emblem").as_deref(), Some(&rec[..]), "emblem record rewritten");

    let info = st.vault_open(p.to_str().unwrap(), pw("pw")).unwrap();
    st.vault_emblem_set(&info.id, Emblem::Icon { id: "house".into() }, true, None, None).unwrap();
    st.vault_lock(&info.id, "user").unwrap();
    assert_eq!(
        json_of(meta(&p, "nicevault.emblem")),
        json!({"emblem": {"kind": "icon", "id": "house"}, "show_when_locked": true, "frame": "gold"})
    );
    let h = nv_core::peek_appearance(&p).unwrap().unwrap();
    assert_eq!((h.accent.as_deref(), h.emblem), (Some("seppia"), Some(LockedEmblem::Icon("house".into()))));
}

#[test]
fn unreadable_emblem_record_is_kept_until_replaced() {
    let t = tempfile::tempdir().unwrap();
    let rec = b"\xff\x00 not json";
    let p = newer_volume(t.path(), &[("nicevault.emblem", rec)], None);
    let st = app(t.path());
    let info = st.vault_open(p.to_str().unwrap(), pw("pw")).unwrap();
    assert!(!info.show_when_locked);
    assert_eq!(st.vault_appearance_set(&info.id, None, None, Some(Visibility::Visible), None, None).unwrap_err().code, "unsupported_version");
    st.vault_lock(&info.id, "user").unwrap();
    assert_eq!(meta(&p, "nicevault.emblem").as_deref(), Some(&rec[..]));
    assert_eq!(nv_core::peek_appearance(&p).unwrap(), None, "no header section invented");

    let info = st.vault_open(p.to_str().unwrap(), pw("pw")).unwrap();
    let s = st.vault_appearance_set(&info.id, None, Some(Emblem::Initial), Some(Visibility::Visible), None, None).unwrap();
    assert_eq!(s.visibility, Visibility::Visible);
    st.vault_lock(&info.id, "user").unwrap();
    assert_eq!(json_of(meta(&p, "nicevault.emblem")), json!({"emblem": {"kind": "initial"}, "show_when_locked": true}));
}

#[test]
fn settings_keep_unknown_keys_and_values() {
    let t = tempfile::tempdir().unwrap();
    let rec = br#"{"accent":"prugna","theme":"auto","auto_lock_minutes":"soon","lock_on_sleep":false,"future_opt":{"a":1}}"#;
    let p = newer_volume(t.path(), &[("nicevault.settings", rec)], None);
    let st = app(t.path());
    let info = st.vault_open(p.to_str().unwrap(), pw("pw")).unwrap();
    let s = &info.settings;
    assert_eq!((s.accent.as_str(), s.theme.as_str(), s.auto_lock_minutes, s.lock_on_sleep), ("prugna", "dark", 15, false));
    st.settings_set(patch(json!({"lock_on_screen_lock": false, "theme": "neon"})), Some(&info.id)).unwrap();
    st.vault_lock(&info.id, "user").unwrap();
    assert_eq!(
        json_of(meta(&p, "nicevault.settings")),
        json!({"accent":"prugna","theme":"auto","auto_lock_minutes":"soon","lock_on_sleep":false,"future_opt":{"a":1},"lock_on_screen_lock":false}),
        "only the accepted key is written"
    );
}

#[test]
fn settings_that_are_not_an_object_are_never_replaced() {
    let t = tempfile::tempdir().unwrap();
    let rec = br#"["settings", "v9"]"#;
    let p = newer_volume(t.path(), &[("nicevault.settings", rec)], Some(Appearance { accent: Some("ottanio".into()), emblem: None }));
    let st = app(t.path());
    let info = st.vault_open(p.to_str().unwrap(), pw("pw")).unwrap();
    assert_eq!(info.settings.accent, "velluto", "shown as defaults");
    assert_eq!(st.settings_set(patch(json!({"theme": "light"})), Some(&info.id)).unwrap_err().code, "unsupported_version");
    assert_eq!(st.vault_appearance_set(&info.id, Some("prugna".into()), None, None, None, None).unwrap_err().code, "unsupported_version");
    st.vault_lock(&info.id, "user").unwrap();
    assert_eq!(meta(&p, "nicevault.settings").as_deref(), Some(&rec[..]));
    assert_eq!(nv_core::peek_appearance(&p).unwrap().unwrap().accent.as_deref(), Some("ottanio"), "header not resynced from defaults");
}

#[test]
fn newer_header_reports_unsupported_version() {
    let t = tempfile::tempdir().unwrap();
    let p = newer_volume(t.path(), &[], None);
    let mut raw = std::fs::read(&p).unwrap();
    for c in 0..2 {
        let copy = &mut raw[c * 65536..(c + 1) * 65536];
        copy[8] = 3;
        let sum = blake3::hash(&copy[..65536 - 32]);
        copy[65536 - 32..].copy_from_slice(sum.as_bytes());
    }
    std::fs::write(&p, &raw).unwrap();
    let st = app(t.path());
    assert_eq!(st.recent_list().len(), 1);
    let e = st.vault_open(p.to_str().unwrap(), pw("pw")).unwrap_err();
    assert_eq!(e.code, "unsupported_version", "{e}");
    assert!(std::fs::read(&p).unwrap() == raw);
}
