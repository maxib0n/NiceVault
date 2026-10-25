//! Deleting as a job (delete.rs): "616 elementi, 1,1 GB — sembra non fare nulla". The delete
//! runs in committed batches with progress, can be cancelled (what went stays gone) and reports
//! the items that could not go.

use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events};
use nv_fs::{Fs, Kind, NodeId, ROOT};
use secrecy::SecretString;
use serde_json::Value;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Default)]
struct Rec {
    events: Mutex<Vec<(String, Value)>>,
    cv: Condvar,
    /// block the job thread on the first `fs-changed` (a committed batch) until released
    gate: Mutex<Gate>,
    gate_cv: Condvar,
}

#[derive(Default)]
struct Gate {
    armed: bool,
    reached: bool,
    released: bool,
}

struct Sink(Arc<Rec>);
impl Events for Sink {
    fn emit(&self, event: &str, payload: Value) {
        self.0.events.lock().unwrap().push((event.to_owned(), payload));
        self.0.cv.notify_all();
        if event == "fs-changed" {
            let mut g = self.0.gate.lock().unwrap();
            if g.armed && !g.reached {
                g.reached = true;
                self.0.gate_cv.notify_all();
                while !g.released {
                    g = self.0.gate_cv.wait(g).unwrap();
                }
            }
        }
    }
}

impl Rec {
    fn wait_done(&self, job: &str) -> Value {
        let mut ev = self.events.lock().unwrap();
        loop {
            if let Some((_, p)) = ev.iter().find(|(e, p)| e == "job-done" && p["job"] == job) {
                return p.clone();
            }
            let (g, t) = self.cv.wait_timeout(ev, Duration::from_secs(120)).unwrap();
            assert!(!t.timed_out(), "job-done never came");
            ev = g;
        }
    }
    fn wait_gate(&self) {
        let mut g = self.gate.lock().unwrap();
        while !g.reached {
            let (n, t) = self.gate_cv.wait_timeout(g, Duration::from_secs(120)).unwrap();
            assert!(!t.timed_out(), "no batch was committed");
            g = n;
        }
    }
    fn release(&self) {
        self.gate.lock().unwrap().released = true;
        self.gate_cv.notify_all();
    }
    fn of(&self, event: &str) -> Vec<Value> {
        self.events.lock().unwrap().iter().filter(|(e, _)| e == event).map(|(_, p)| p.clone()).collect()
    }
}

fn state(dir: &Path) -> (Arc<AppState>, Arc<Rec>) {
    let rec = Arc::new(Rec::default());
    let st = AppState::new(
        Store::new(Locations { exe_dir: Some(dir.to_path_buf()), removable: Some(vec![]), config_dir: None }),
        Box::new(Sink(rec.clone())),
    );
    (st, rec)
}

fn pw() -> SecretString {
    SecretString::from("pw".to_owned())
}

fn files(v: &dyn Fs, dir: NodeId, prefix: &str, n: usize, size: usize) {
    let data = vec![7u8; size];
    for i in 0..n {
        let f = v.create(dir, &format!("{prefix}{i:04}.jpg"), Kind::File).unwrap();
        v.write(f.id, 0, &data).unwrap();
    }
}

fn count(v: &dyn Fs, dir: NodeId) -> usize {
    v.readdir(dir).unwrap().len()
}

#[test]
fn deletes_in_committed_batches_with_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, rec) = state(tmp.path());
    let path = tmp.path().join("v.nvault");
    let id = st.vault_create(path.to_str().unwrap(), pw(), Some("fast")).unwrap().id;
    let v = st.vault(&id).unwrap();
    let album = v.create(ROOT, "Album", Kind::Dir).unwrap();
    files(&*v, album.id, "IMG_", 600, 2048);
    let big = v.create(ROOT, "video.mov", Kind::File).unwrap();
    let mb = vec![3u8; 1 << 20];
    for k in 0..40u64 {
        v.write(big.id, k << 20, &mb).unwrap();
    }
    v.flush().unwrap();
    let seq0 = v.commit_seq().unwrap();

    let job = nicevault_app::delete::fs_delete(&st, &id, vec![album.id, big.id]).unwrap();
    let done = rec.wait_done(&job);
    assert!(done["error"].is_null(), "{done}");
    let out = &done["outcome"];
    assert_eq!(out["deleted"], 602);
    assert_eq!(out["total"], 602);
    assert_eq!(out["freed_bytes"], 600 * 2048 + (40u64 << 20));
    assert_eq!(out["errors"].as_array().unwrap().len(), 0);
    assert_eq!(count(&*v, ROOT), 0);

    // batches: 600 items at 128 per commit, the 40 MiB file cut 4 MiB per commit
    let changed = rec.of("fs-changed");
    assert!(changed.len() >= 5, "{} batches", changed.len());
    assert!(changed.iter().any(|c| c["dirs"].as_array().unwrap().contains(&album.id.into())));
    assert!(v.commit_seq().unwrap() >= seq0 + 7, "{} commits", v.commit_seq().unwrap() - seq0);
    let prog: Vec<Value> = rec.of("job-progress").into_iter().filter(|p| p["job"] == job).collect();
    let items: Vec<u64> = prog.iter().filter_map(|p| p["done_items"].as_u64()).collect();
    assert!(items.windows(2).all(|w| w[0] <= w[1]), "items go forward");
    assert!(prog.iter().any(|p| p["total_items"] == 602 && p["total_bytes"] == 600 * 2048 + (40u64 << 20)));
    assert!(
        prog.iter().any(|p| p["current"] == "video.mov" && p["done_bytes"].as_u64().is_some_and(|b| b > 0 && b < 600 * 2048 + (40u64 << 20))),
        "the big file is freed step by step"
    );

    // durable: gone after lock / reopen
    drop(v);
    st.vault_lock(&id, "user").unwrap();
    let id = st.vault_open(path.to_str().unwrap(), pw()).unwrap().id;
    assert_eq!(count(&*st.vault(&id).unwrap(), ROOT), 0);
}

#[test]
fn cancel_keeps_what_was_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, rec) = state(tmp.path());
    let path = tmp.path().join("v.nvault");
    let id = st.vault_create(path.to_str().unwrap(), pw(), Some("fast")).unwrap().id;
    let v = st.vault(&id).unwrap();
    let album = v.create(ROOT, "Album", Kind::Dir).unwrap();
    files(&*v, album.id, "IMG_", 600, 1024);
    v.flush().unwrap();

    rec.gate.lock().unwrap().armed = true;
    let job = nicevault_app::delete::fs_delete(&st, &id, vec![album.id]).unwrap();
    rec.wait_gate();
    st.job_cancel(&job);
    rec.release();
    let done = rec.wait_done(&job);
    assert_eq!(done["error"]["code"], "cancelled", "{done}");
    let deleted = done["outcome"]["deleted"].as_u64().unwrap();
    assert!((128..600).contains(&deleted), "{deleted}");
    assert_eq!(count(&*v, album.id) as u64, 600 - deleted);

    drop(v);
    st.vault_lock(&id, "user").unwrap();
    let id = st.vault_open(path.to_str().unwrap(), pw()).unwrap().id;
    let v = st.vault(&id).unwrap();
    let album = v.lookup(ROOT, "Album").unwrap();
    assert_eq!(count(&*v, album.id) as u64, 600 - deleted, "the deleted ones stay deleted");
}

#[test]
fn an_item_that_cannot_go_is_reported_and_its_folders_stay() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, rec) = state(tmp.path());
    let id = st.vault_create(tmp.path().join("v.nvault").to_str().unwrap(), pw(), Some("fast")).unwrap().id;
    let v = st.vault(&id).unwrap();
    let album = v.create(ROOT, "Album", Kind::Dir).unwrap();
    let sub = v.create(album.id, "Sub", Kind::Dir).unwrap();
    files(&*v, album.id, "A_", 50, 512);
    files(&*v, sub.id, "B_", 300, 512);
    let other = v.create(ROOT, "nota.txt", Kind::File).unwrap();
    v.flush().unwrap();

    // while the job runs, something lands in Sub (a copy through the mounted drive)
    rec.gate.lock().unwrap().armed = true;
    let job = nicevault_app::delete::fs_delete(&st, &id, vec![album.id, other.id]).unwrap();
    rec.wait_gate();
    v.create(sub.id, "nuovo.txt", Kind::File).unwrap();
    rec.release();
    let done = rec.wait_done(&job);
    assert!(done["error"].is_null(), "{done}");
    let errs = done["outcome"]["errors"].as_array().unwrap();
    assert_eq!(errs.len(), 1, "{done}");
    assert_eq!(errs[0]["node"], sub.id);
    assert_eq!(errs[0]["code"], "not_empty");
    assert_eq!(done["outcome"]["deleted"], 351);
    // Sub (with the newcomer) and Album stay; everything else went
    assert_eq!(v.readdir(ROOT).unwrap().iter().map(|n| n.name.clone()).collect::<Vec<_>>(), ["Album"]);
    assert_eq!(v.readdir(album.id).unwrap().iter().map(|n| n.name.clone()).collect::<Vec<_>>(), ["Sub"]);
    assert_eq!(v.readdir(sub.id).unwrap().iter().map(|n| n.name.clone()).collect::<Vec<_>>(), ["nuovo.txt"]);
}

#[test]
fn root_and_overlapping_selections() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, rec) = state(tmp.path());
    let id = st.vault_create(tmp.path().join("v.nvault").to_str().unwrap(), pw(), Some("fast")).unwrap().id;
    assert_eq!(nicevault_app::delete::fs_delete(&st, &id, vec![ROOT]).unwrap_err().code, "invalid_name");
    let v = st.vault(&id).unwrap();
    let a = v.create(ROOT, "A", Kind::Dir).unwrap();
    let b = v.create(a.id, "b.txt", Kind::File).unwrap();
    // a child selected with its folder, and a node already gone
    let job = nicevault_app::delete::fs_delete(&st, &id, vec![b.id, a.id, 9999]).unwrap();
    let done = rec.wait_done(&job);
    assert!(done["error"].is_null(), "{done}");
    assert_eq!(done["outcome"]["deleted"], 2);
    assert_eq!(count(&*v, ROOT), 0);
    // the synchronous form (mobile Files provider, tests) takes the same path
    let c = v.create(ROOT, "c", Kind::Dir).unwrap();
    files(&*v, c.id, "x", 3, 10);
    st.fs_delete(&id, &[c.id]).unwrap();
    assert_eq!(count(&*v, ROOT), 0);
}
