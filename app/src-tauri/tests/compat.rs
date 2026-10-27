use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use nicevault_app::appearance::{SettingsPatch, Visibility};
use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, NoEvents};
use nv_core::{Appearance, LockedEmblem};
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn sha(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn sets() -> Vec<(PathBuf, Value)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/nv-core/tests/fixtures/compat");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    assert!(!dirs.is_empty());
    dirs.into_iter().map(|d| {
        let m: Value = serde_json::from_slice(&std::fs::read(d.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(m["manifest_version"], 1);
        (d, m)
    }).collect()
}

fn contained(want: &Value, got: &Value, at: &str) {
    match (want, got) {
        (Value::Object(w), Value::Object(g)) => {
            for (k, wv) in w {
                let gv = g.get(k).unwrap_or_else(|| panic!("{at}.{k}: missing (have {got})"));
                contained(wv, gv, &format!("{at}.{k}"));
            }
        }
        (Value::Array(w), Value::Array(g)) => {
            assert_eq!(w.len(), g.len(), "{at}: length differs\nwant {want}\ngot  {got}");
            for (i, (wv, gv)) in w.iter().zip(g).enumerate() {
                contained(wv, gv, &format!("{at}[{i}]"));
            }
        }
        _ => assert_eq!(want, got, "{at}"),
    }
}

fn appearance_json(a: Option<&Appearance>) -> Value {
    let Some(a) = a else { return Value::Null };
    let emblem = match &a.emblem {
        None => Value::Null,
        Some(LockedEmblem::Initial(s)) => json!({"kind": "initial", "text": s}),
        Some(LockedEmblem::Icon(s)) => json!({"kind": "icon", "id": s}),
        Some(LockedEmblem::Image(b)) => json!({"kind": "image", "base64": B64.encode(b), "sha256": sha(b)}),
    };
    json!({"accent": a.accent, "emblem": emblem})
}

fn portable(stick: &Path) -> Arc<AppState> {
    AppState::new(Store::new(Locations { exe_dir: Some(stick.to_path_buf()), removable: Some(vec![]), config_dir: None }), Box::new(NoEvents))
}

fn check_volume(dir: &Path, vol: &Value) {
    let name = vol["file"].as_str().unwrap();
    let fixture = dir.join(name);
    assert_eq!(sha(&std::fs::read(&fixture).unwrap()), vol["sha256"].as_str().unwrap(), "{name}: fixture was modified");
    let app = &vol["app"];
    let slot = app["open_password_slot"].as_u64().unwrap();
    let main_pw = vol["passwords"].as_array().unwrap().iter().find(|p| p["slot"] == slot).unwrap()["password"].as_str().unwrap();

    let stick = tempfile::tempdir().unwrap();
    let p = stick.path().join(name);
    std::fs::copy(&fixture, &p).unwrap();
    let path = p.to_str().unwrap();
    let st = portable(stick.path());

    let recent = st.recent_list();
    assert_eq!(recent.len(), 1, "{name}");
    let r = &recent[0];
    let got = json!({"name": r.name, "accent": r.accent, "emblem": r.emblem, "show_when_locked": r.show_when_locked});
    contained(&app["recent_before_open"], &got, &format!("{name}:recent"));

    let info = st.vault_open(path, pw(main_pw)).unwrap();
    contained(&app["vault_info"], &serde_json::to_value(&info).unwrap(), &format!("{name}:vault_info"));
    contained(&app["appearance"], &serde_json::to_value(st.vault_appearance_get(&info.id).unwrap()).unwrap(), &format!("{name}:appearance"));
    contained(&app["settings"], &serde_json::to_value(st.settings_get(Some(&info.id)).unwrap()).unwrap(), &format!("{name}:settings"));
    contained(&app["slots"], &serde_json::to_value(st.slots_list(&info.id).unwrap()).unwrap(), &format!("{name}:slots"));
    contained(&app["pm_list"], &serde_json::to_value(st.pm_list(&info.id).unwrap()).unwrap(), &format!("{name}:pm_list"));
    st.vault_lock(&info.id, "user").unwrap();
    contained(&vol["header_after_app_open"], &appearance_json(nv_core::peek_appearance(&p).unwrap().as_ref()), &format!("{name}:header"));

    let settings: SettingsPatch = serde_json::from_value(json!({"theme": "light", "auto_lock_minutes": 7})).unwrap();
    let info = st.vault_open(path, pw(main_pw)).unwrap();
    let s = st.settings_set(settings, Some(&info.id)).unwrap();
    assert_eq!((s.theme.as_str(), s.auto_lock_minutes), ("light", 7));
    let vis = app["appearance"]["visibility"].as_str().unwrap();
    let (flip, back) = if vis == "visible" { (Visibility::Encrypted, Visibility::Visible) } else { (Visibility::Visible, Visibility::Encrypted) };
    st.vault_appearance_set(&info.id, None, None, Some(flip), None, None).unwrap();
    st.vault_appearance_set(&info.id, None, None, Some(back), None, None).unwrap();
    st.vault_lock(&info.id, "user").unwrap();
    contained(&vol["header_after_app_open"], &appearance_json(nv_core::peek_appearance(&p).unwrap().as_ref()), &format!("{name}:header after write"));
    let info = st.vault_open(path, pw(main_pw)).unwrap();
    let mut want = app["settings"].clone();
    want["theme"] = json!("light");
    want["auto_lock_minutes"] = json!(7);
    contained(&want, &serde_json::to_value(&info.settings).unwrap(), &format!("{name}:settings after write"));
    let mut want = app["appearance"].clone();
    want.as_object_mut().unwrap().remove("tampered");
    contained(&want, &serde_json::to_value(st.vault_appearance_get(&info.id).unwrap()).unwrap(), &format!("{name}:appearance after write"));
    contained(&app["pm_list"], &serde_json::to_value(st.pm_list(&info.id).unwrap()).unwrap(), &format!("{name}:pm_list after write"));
    st.vault_lock(&info.id, "user").unwrap();

    assert_eq!(sha(&std::fs::read(&fixture).unwrap()), vol["sha256"].as_str().unwrap(), "{name}: fixture modified by the test");
}

#[test]
fn every_fixture_volume_looks_the_same_in_the_app() {
    for (dir, m) in sets() {
        for vol in m["volumes"].as_array().unwrap() {
            check_volume(&dir, vol);
        }
    }
}

#[test]
fn bordeaux_reads_as_velluto() {
    let (_, m) = sets().into_iter().find(|(d, _)| d.ends_with("2026-10-24")).unwrap();
    let vol = m["volumes"].as_array().unwrap().iter().find(|v| v["file"] == "bordeaux.nvault").unwrap();
    assert_eq!(vol["peek"]["appearance"]["accent"], "bordeaux");
    assert_eq!(vol["app"]["settings"]["accent"], "velluto");
    assert_eq!(vol["app"]["recent_before_open"]["accent"], "velluto");
    assert_eq!(vol["header_after_app_open"]["accent"], "velluto");
}
