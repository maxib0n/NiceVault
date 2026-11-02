use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events};
use nv_fs::{Fs, Kind, ROOT};
use secrecy::SecretString;
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Rec(Arc<Mutex<Vec<(String, Value)>>>);

impl Events for Rec {
    fn emit(&self, event: &str, payload: Value) {
        self.0.lock().unwrap().push((event.to_owned(), payload));
    }
}

impl Rec {
    fn take(&self, event: &str) -> Vec<Value> {
        let mut g = self.0.lock().unwrap();
        let (hit, rest): (Vec<_>, Vec<_>) = g.drain(..).partition(|(e, _)| e == event);
        *g = rest;
        hit.into_iter().map(|(_, v)| v).collect()
    }
}

fn pw() -> SecretString {
    SecretString::from("pw".to_owned())
}

const MIB: u64 = 1 << 20;

#[test]
fn resize_through_the_app() {
    let t = tempfile::tempdir().unwrap();
    let rec = Rec::default();
    let st = AppState::new(Store::new(Locations { exe_dir: None, removable: Some(vec![]), config_dir: None }), Box::new(rec.clone()));
    let vol = t.path().join("r.nvault");
    let id = st.vault_create_sized(vol.to_str().unwrap(), pw(), Some("fast"), Some(24 * MIB)).unwrap().id;
    let v = st.vault(&id).unwrap();
    let f = v.create(ROOT, "a.bin", Kind::File).unwrap();
    v.write(f.id, 0, &vec![7u8; 3 << 20]).unwrap();
    v.flush().unwrap();
    drop(v);

    let info = serde_json::to_value(st.vault_resize_info(&id).unwrap()).unwrap();
    for k in ["capacity", "file_bytes", "used_bytes", "min_capacity", "min_in_place", "max_capacity", "drive_free"] {
        assert!(info[k].is_u64(), "{k}: {info}");
    }
    assert_eq!(info["capacity"], 24 * MIB);

    let after = st.vault_resize(&id, 40 * MIB).unwrap();
    assert_eq!((after.capacity, after.file_bytes), (Some(40 * MIB), 40 * MIB));
    let ev = rec.take("vault-resize");
    assert!(ev.iter().any(|e| e["phase"] == "extend" && e["vault"] == id.as_str()), "{ev:?}");
    assert_eq!(st.vault_stats(&id).unwrap().file_size_bytes, 40 * MIB);

    let e = st.vault_resize(&id, 4 * MIB).unwrap_err();
    assert_eq!(e.code, "too_small", "{e}");
    let e = st.vault_resize(&id, 1 << 50).unwrap_err();
    assert_eq!(e.code, "out_of_range", "{e}");
    let e = st.vault_resize(&id, (1 << 40) + 4096).unwrap_err();
    assert!(e.code == "no_space", "{e}");

    let after = st.vault_resize(&id, 16 * MIB).unwrap();
    assert_eq!(after.capacity, Some(16 * MIB));
    st.vault_lock(&id, "user").unwrap();
    let id = st.vault_open(vol.to_str().unwrap(), pw()).unwrap().id;
    let info = st.vault_resize_info(&id).unwrap();
    assert_eq!((info.capacity, info.file_bytes), (Some(16 * MIB), 16 * MIB));
    let v = st.vault(&id).unwrap();
    let n = v.lookup(ROOT, "a.bin").unwrap();
    let mut buf = vec![0u8; 3 << 20];
    let mut off = 0;
    while off < buf.len() {
        off += v.read(n.id, off as u64, &mut buf[off..]).unwrap();
    }
    assert!(buf.iter().all(|&b| b == 7));
    drop(v);
    st.vault_lock(&id, "user").unwrap();

    let g = t.path().join("g.nvault");
    let id = st.vault_create(g.to_str().unwrap(), pw(), Some("fast")).unwrap().id;
    let info = serde_json::to_value(st.vault_resize_info(&id).unwrap()).unwrap();
    assert!(info["capacity"].is_null() && info["file_bytes"].as_u64().unwrap() > 0, "{info}");
    assert_eq!(st.vault_resize(&id, 64 * MIB).unwrap_err().code, "not_fixed");
    st.vault_lock(&id, "user").unwrap();
}

#[test]
fn fixed_size_from_the_create_wizard() {
    let t = tempfile::tempdir().unwrap();
    let st = AppState::new(Store::new(Locations { exe_dir: None, removable: Some(vec![]), config_dir: None }), Box::new(Rec::default()));
    let vol = t.path().join("Fisso.nvault");
    let path = vol.to_str().unwrap();

    let e = st.vault_create_sized(path, pw(), Some("fast"), Some(MIB)).unwrap_err();
    assert_eq!(e.code, "out_of_range", "{e}");
    let e = st.vault_create_sized(path, pw(), Some("fast"), Some(1 << 50)).unwrap_err();
    assert_eq!(e.code, "out_of_range", "{e}");
    let e = st.vault_create_sized(path, pw(), Some("fast"), Some(8 << 40)).unwrap_err();
    assert_eq!(e.code, "no_space", "{e}");
    assert!(!vol.exists());

    let asked = 48 * MIB + 1234;
    let id = st.vault_create_sized(path, pw(), Some("fast"), Some(asked)).unwrap().id;
    let want = asked - asked % 4096;
    assert_eq!(std::fs::metadata(&vol).unwrap().len(), want);
    let info = st.vault_resize_info(&id).unwrap();
    assert_eq!((info.capacity, info.file_bytes), (Some(want), want));
    let v = st.vault(&id).unwrap();
    let f = v.create(ROOT, "x.bin", Kind::File).unwrap();
    v.write(f.id, 0, &[9u8; 1 << 16]).unwrap();
    v.flush().unwrap();
    drop(v);
    assert_eq!(std::fs::metadata(&vol).unwrap().len(), want, "a fixed volume does not grow");
    st.vault_lock(&id, "user").unwrap();

    let id = st.vault_open(path, pw()).unwrap().id;
    assert_eq!(st.vault_resize_info(&id).unwrap().capacity, Some(want));
    assert_eq!(st.vault_resize(&id, 64 * MIB).unwrap().capacity, Some(64 * MIB));
    assert_eq!(std::fs::metadata(&vol).unwrap().len(), 64 * MIB);
    assert_eq!(st.vault_resize(&id, 32 * MIB).unwrap().capacity, Some(32 * MIB));
    assert_eq!(std::fs::metadata(&vol).unwrap().len(), 32 * MIB);
    let v = st.vault(&id).unwrap();
    let n = v.lookup(ROOT, "x.bin").unwrap();
    let mut buf = vec![0u8; 1 << 16];
    let mut off = 0;
    while off < buf.len() {
        off += v.read(n.id, off as u64, &mut buf[off..]).unwrap();
    }
    assert!(buf.iter().all(|&b| b == 9));
    drop(v);
    st.vault_lock(&id, "user").unwrap();
}
