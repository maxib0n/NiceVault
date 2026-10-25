//! Live refresh through a real FUSE mount: files changed through the OS (std::fs on the
//! mountpoint, like a file manager would) produce `fs-changed` for the right folder within
//! 1 s; changes made by the app are visible through the drive within the 1 s attr TTL.

use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events};
use nv_fs::ROOT;
use secrecy::SecretString;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
struct Rec(Arc<Mutex<Vec<(Instant, String, Value)>>>);

impl Events for Rec {
    fn emit(&self, event: &str, payload: Value) {
        self.0.lock().unwrap().push((Instant::now(), event.to_owned(), payload));
    }
}

impl Rec {
    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
    /// First `fs-changed` naming `dir`, waited for at most `timeout` after `since`.
    fn changed(&self, vault: &str, dir: u64, since: Instant, timeout: Duration) -> Duration {
        loop {
            let hit = self.0.lock().unwrap().iter().find_map(|(t, e, v)| {
                (e == "fs-changed" && v["vault"] == vault && v["dirs"].as_array().unwrap().iter().any(|d| d == dir))
                    .then_some(*t)
            });
            if let Some(t) = hit {
                return t - since;
            }
            assert!(since.elapsed() < timeout, "no fs-changed for dir {dir} within {timeout:?}; got {:?}", self.0.lock().unwrap());
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn count(&self, event: &str) -> usize {
        self.0.lock().unwrap().iter().filter(|(_, e, _)| e == event).count()
    }
}

#[test]
fn os_changes_on_the_mounted_drive_emit_fs_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = Rec::default();
    let st = AppState::new(
        Store::new(Locations { exe_dir: Some(tmp.path().to_path_buf()), removable: Some(vec![]), config_dir: None }),
        Box::new(rec.clone()),
    );
    let vol = tmp.path().join("Live.nvault");
    let id = st.vault_create(vol.to_str().unwrap(), SecretString::from("x".to_owned()), Some("fast")).unwrap().id;
    let docs = st.fs_mkdir(&id, ROOT, "Documenti").unwrap();

    if !st.mount_available().iter().any(|b| b.id == "fuse" && b.available) {
        eprintln!("FUSE unavailable here: live refresh test skipped");
        return;
    }
    let mp = PathBuf::from(st.mount(&id).unwrap().mountpoint);
    let one_s = Duration::from_secs(1);

    // copy a file into a subfolder (create + writes + close, as cp/Dolphin do)
    rec.clear();
    let t0 = Instant::now();
    std::fs::write(mp.join("Documenti/da-dolphin.bin"), vec![9u8; 3 * 1024 * 1024]).unwrap();
    let took = rec.changed(&id, docs.id, t0, one_s);
    eprintln!("fs-changed for the copy after {took:?}");
    assert!(st.fs_list(&id, docs.id).unwrap().iter().any(|n| n.name == "da-dolphin.bin" && n.size == 3 * 1024 * 1024));
    std::thread::sleep(Duration::from_millis(400));
    // coalesced: a 3 MiB copy is a handful of events, not one per FUSE write
    assert!(rec.count("fs-changed") <= 4, "{} events for one copy", rec.count("fs-changed"));

    // mkdir in the root
    rec.clear();
    let t0 = Instant::now();
    std::fs::create_dir(mp.join("Nuova")).unwrap();
    rec.changed(&id, ROOT, t0, one_s);

    // rename across folders: both parents
    rec.clear();
    let t0 = Instant::now();
    std::fs::rename(mp.join("Documenti/da-dolphin.bin"), mp.join("Nuova/spostato.bin")).unwrap();
    let nuova = st.fs_list(&id, ROOT).unwrap().into_iter().find(|n| n.name == "Nuova").unwrap();
    rec.changed(&id, docs.id, t0, one_s);
    rec.changed(&id, nuova.id, t0, one_s);

    // delete
    rec.clear();
    let t0 = Instant::now();
    std::fs::remove_file(mp.join("Nuova/spostato.bin")).unwrap();
    rec.changed(&id, nuova.id, t0, one_s);

    // the reverse: what the app does shows up on the drive within the attr/entry TTL (1 s)
    st.fs_mkdir(&id, nuova.id, "dall-app").unwrap();
    let t0 = Instant::now();
    while !mp.join("Nuova/dall-app").is_dir() {
        assert!(t0.elapsed() < Duration::from_millis(1500), "app mkdir not visible through the drive");
        std::thread::sleep(Duration::from_millis(50));
    }
    st.fs_rename(&id, docs.id, ROOT, "Rinominata").unwrap();
    let t0 = Instant::now();
    while !mp.join("Rinominata").is_dir() || mp.join("Documenti").exists() {
        assert!(t0.elapsed() < Duration::from_millis(1500), "app rename not visible through the drive");
        std::thread::sleep(Duration::from_millis(50));
    }

    st.unmount(&id).unwrap();
    st.vault_lock(&id, "user").unwrap();
}
