//! "Estendi / Riduci" through the functions the Tauri commands call, with the JSON shapes api.ts
//! relies on: vault_resize_info for fixed and growable volumes, vault_resize grow / shrink with
//! its vault-resize events, the refusal codes, and the size surviving a lock + reopen.

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
    assert!(e.code == "no_space", "{e}"); // 1 TiB more than any test drive has free

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

    // growable: size facts, nothing to resize
    let g = t.path().join("g.nvault");
    let id = st.vault_create(g.to_str().unwrap(), pw(), Some("fast")).unwrap().id;
    let info = serde_json::to_value(st.vault_resize_info(&id).unwrap()).unwrap();
    assert!(info["capacity"].is_null() && info["file_bytes"].as_u64().unwrap() > 0, "{info}");
    assert_eq!(st.vault_resize(&id, 64 * MIB).unwrap_err().code, "not_fixed");
    st.vault_lock(&id, "user").unwrap();
}
