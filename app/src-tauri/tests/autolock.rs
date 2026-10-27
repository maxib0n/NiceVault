use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events};
use secrecy::SecretString;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default, Clone)]
struct Rec(Arc<Mutex<Vec<(String, Value)>>>);
impl Events for Rec {
    fn emit(&self, event: &str, payload: Value) {
        self.0.lock().unwrap().push((event.into(), payload));
    }
}
impl Rec {
    fn locked(&self) -> Vec<(String, String)> {
        let mut g = self.0.lock().unwrap();
        let mut v: Vec<(String, String)> = g
            .iter()
            .filter(|(e, _)| e == "vault-locked")
            .map(|(_, p)| (p["vault"].as_str().unwrap().to_owned(), p["reason"].as_str().unwrap().to_owned()))
            .collect();
        g.clear();
        v.sort();
        v
    }
}

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn app(dir: &std::path::Path) -> (Arc<AppState>, Rec) {
    let rec = Rec::default();
    let loc = Locations { exe_dir: Some(dir.to_path_buf()), removable: Some(vec![]), config_dir: None };
    (AppState::new(Store::new(loc), Box::new(rec.clone())), rec)
}

fn patch(json: &str) -> nicevault_app::appearance::SettingsPatch {
    serde_json::from_str(json).unwrap()
}

fn open_ids(st: &AppState) -> Vec<String> {
    let mut v: Vec<String> = st.vault_list_open().into_iter().map(|i| i.id).collect();
    v.sort();
    v
}

const MIN: Duration = Duration::from_secs(60);

#[test]
fn settings_defaults_and_patch() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, _) = app(tmp.path());
    let a = st.vault_create(tmp.path().join("a.nvault").to_str().unwrap(), pw("x"), Some("fast")).unwrap();
    assert_eq!(
        serde_json::to_value(&a.settings).unwrap(),
        serde_json::json!({ "auto_lock_minutes": 15, "accent": "velluto", "theme": "dark",
                            "lock_on_sleep": true, "lock_on_screen_lock": true, "lock_scope": "volume",
                            "grid_size": "m", "list_density": "normal" })
    );
    let s = st
        .settings_set(patch(r#"{"auto_lock_minutes": 0, "lock_on_sleep": false, "lock_scope": "all", "lock_on_screen_lock": null}"#), Some(&a.id))
        .unwrap();
    assert_eq!((s.auto_lock_minutes, s.lock_on_sleep, s.lock_on_screen_lock, s.lock_scope.as_str()), (0, false, true, "all"));
    assert_eq!(st.settings_set(patch(r#"{"lock_scope": "nope"}"#), Some(&a.id)).unwrap().lock_scope, "all");
    st.vault_lock(&a.id, "user").unwrap();
    let a = st.vault_open(tmp.path().join("a.nvault").to_str().unwrap(), pw("x")).unwrap();
    assert_eq!((a.settings.auto_lock_minutes, a.settings.lock_on_sleep, a.settings.lock_scope.as_str()), (0, false, "all"));
    st.vault_lock(&a.id, "user").unwrap();
    st.settings_set(patch(r#"{"lock_on_screen_lock": false}"#), None).unwrap();
    let b = st.vault_create(tmp.path().join("b.nvault").to_str().unwrap(), pw("x"), Some("fast")).unwrap();
    assert!(!b.settings.lock_on_screen_lock);
}

#[test]
fn each_volume_has_its_own_idle_timer() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, rec) = app(tmp.path());
    let mk = |n: &str, min: u32| {
        let v = st.vault_create(tmp.path().join(n).to_str().unwrap(), pw("x"), Some("fast")).unwrap();
        st.settings_set(patch(&format!(r#"{{"auto_lock_minutes": {min}}}"#)), Some(&v.id)).unwrap();
        v.id
    };
    let (five, ten, never) = (mk("five.nvault", 5), mk("ten.nvault", 10), mk("never.nvault", 0));
    rec.locked();
    let t0 = Instant::now();
    assert!(st.idle_tick(t0 + 4 * MIN).is_empty());
    let status = st.autolock_status();
    assert_eq!(status.iter().find(|s| s.vault == never).unwrap().remaining_ms, None);
    assert!(status.iter().find(|s| s.vault == five).unwrap().remaining_ms.unwrap() > 4 * 60_000);
    assert_eq!(st.idle_tick(t0 + 5 * MIN + Duration::from_secs(1)), vec![five.clone()]);
    assert_eq!(rec.locked(), vec![(five.clone(), "idle".to_owned())]);
    assert!(open_ids(&st).contains(&ten));
    std::thread::sleep(Duration::from_millis(30));
    let touched = Instant::now();
    st.vault_touch(&ten).unwrap();
    assert!(st.idle_tick(touched + 10 * MIN - Duration::from_secs(1)).is_empty());
    assert_eq!(st.idle_tick(touched + 10 * MIN + Duration::from_secs(1)), vec![ten.clone()]);
    assert!(st.idle_tick(t0 + 24 * 60 * MIN).is_empty());
    assert_eq!(open_ids(&st), vec![never.clone()]);
    assert_eq!(st.vault_touch("gone").unwrap_err().code, "locked");
}

#[test]
fn scope_all_and_os_triggers_per_volume() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, rec) = app(tmp.path());
    let mk = |n: &str, json: &str| {
        let v = st.vault_create(tmp.path().join(n).to_str().unwrap(), pw("x"), Some("fast")).unwrap();
        st.settings_set(patch(json), Some(&v.id)).unwrap();
        v.id
    };
    let a = mk("a.nvault", r#"{"lock_on_sleep": false}"#);
    let b = mk("b.nvault", r#"{"lock_on_screen_lock": false}"#);
    rec.locked();
    assert_eq!(st.lock_event("screen"), vec![a.clone()]);
    assert_eq!(rec.locked(), vec![(a.clone(), "screen".to_owned())]);
    assert_eq!(st.lock_event("screen"), Vec::<String>::new());
    assert_eq!(st.lock_event("sleep"), vec![b.clone()]);
    assert!(open_ids(&st).is_empty());

    let c = mk("c.nvault", r#"{"lock_scope": "all", "auto_lock_minutes": 1}"#);
    let d = mk("d.nvault", r#"{"lock_on_sleep": false, "lock_on_screen_lock": false, "auto_lock_minutes": 0}"#);
    rec.locked();
    let mut both = vec![c.clone(), d.clone()];
    both.sort();
    assert_eq!(st.idle_tick(Instant::now() + 2 * MIN), both);
    let mut ev = vec![(c, "idle".to_owned()), (d, "idle".to_owned())];
    ev.sort();
    assert_eq!(rec.locked(), ev);
    let e = mk("e.nvault", r#"{"lock_on_sleep": true}"#);
    let f = mk("f.nvault", r#"{"lock_on_sleep": false}"#);
    assert_eq!(st.lock_event("sleep"), vec![e]);
    assert_eq!(open_ids(&st), vec![f]);
}

#[test]
fn running_job_keeps_volume_awake() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, _) = app(tmp.path());
    let v = st.vault_create(tmp.path().join("j.nvault").to_str().unwrap(), pw("x"), Some("fast")).unwrap();
    st.settings_set(patch(r#"{"auto_lock_minutes": 1}"#), Some(&v.id)).unwrap();
    let src = tmp.path().join("big.bin");
    std::fs::write(&src, vec![7u8; 64 << 20]).unwrap();
    let job = nicevault_app::jobs::fs_import(&st, &v.id, nv_fs::ROOT, vec![src.to_string_lossy().into()]).unwrap();
    let busy = st.autolock_status()[0].remaining_ms.is_none();
    let locked = st.idle_tick(Instant::now() + 2 * MIN);
    if busy {
        assert!(locked.is_empty(), "locked while a job was running");
    }
    st.job_cancel(&job);
}

#[test]
fn duress_password_destroys_and_looks_like_a_wrong_one() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, _) = app(tmp.path());
    let path = tmp.path().join("d.nvault");
    let p = path.to_str().unwrap();
    let v = st.vault_create(p, pw("vera"), Some("fast")).unwrap();
    assert_eq!(st.slot_add_duress(&v.id, pw(""), None).unwrap_err().code, "invalid_password");
    assert_eq!(st.slot_add_duress(&v.id, pw("vera"), None).unwrap_err().code, "duress_conflict");
    let slots = st.slot_add_duress(&v.id, pw("panico"), Some("Rapina".into())).unwrap();
    let d = slots.iter().find(|s| s.kind == "duress").unwrap();
    assert_eq!(d.label.as_deref(), Some("Rapina"));
    assert_eq!(st.slot_remove(&v.id, 0).unwrap_err().code, "last_slot");
    assert_eq!(st.slot_change_password(&v.id, d.index, pw("panico"), pw("y"), None).unwrap_err().code, "invalid_slot");
    assert_eq!(st.slot_add_password(&v.id, pw("panico"), None).unwrap_err().code, "duress_conflict");
    let json = serde_json::to_value(&slots).unwrap();
    assert_eq!(json[d.index]["kind"], "duress");
    st.vault_lock(&v.id, "user").unwrap();

    let wrong = st.vault_open(p, pw("sbagliata")).unwrap_err();
    let duress = st.vault_open(p, pw("panico")).unwrap_err();
    assert_eq!(wrong, duress, "same error, same message");
    let after = st.vault_open(p, pw("vera")).unwrap_err();
    assert_eq!(after, wrong, "the real password no longer opens it");
    assert!(st.recent_list().iter().any(|r| r.path == p));
    let p2 = tmp.path().join("e.nvault");
    let v = st.vault_create(p2.to_str().unwrap(), pw("vera"), Some("fast")).unwrap();
    let i = st.slot_add_duress(&v.id, pw("panico"), None).unwrap().iter().find(|s| s.kind == "duress").unwrap().index;
    assert_eq!(st.slot_remove(&v.id, i).unwrap().len(), 1);
    st.vault_lock(&v.id, "user").unwrap();
    assert_eq!(st.vault_open(p2.to_str().unwrap(), pw("panico")).unwrap_err().code, "wrong_password");
    assert!(st.vault_open(p2.to_str().unwrap(), pw("vera")).is_ok());
}
