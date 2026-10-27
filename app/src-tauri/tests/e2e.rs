use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events};
use nv_fs::{Kind, Node, ROOT};
use secrecy::SecretString;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::http::{Request, StatusCode, header};

#[derive(Clone, Default)]
struct Rec(Arc<Mutex<Vec<(String, Value)>>>);

impl Events for Rec {
    fn emit(&self, event: &str, payload: Value) {
        self.0.lock().unwrap().push((event.to_owned(), payload));
    }
}

impl Rec {
    fn wait(&self, timeout: Duration, f: impl Fn(&str, &Value) -> bool) -> Value {
        let t0 = Instant::now();
        loop {
            if let Some((_, v)) = self.0.lock().unwrap().iter().find(|(e, v)| f(e, v)) {
                return v.clone();
            }
            assert!(t0.elapsed() < timeout, "timed out waiting for event; got {:?}", self.0.lock().unwrap());
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn job_done(&self, job: &str) -> Value {
        self.wait(Duration::from_secs(60), |e, v| e == "job-done" && v["job"] == job)
    }
    fn count(&self, event: &str, job: &str) -> usize {
        self.0.lock().unwrap().iter().filter(|(e, v)| e == event && v["job"] == job).count()
    }
}

fn stick_store(exe: &Path) -> Store {
    Store::new(Locations { exe_dir: Some(exe.to_path_buf()), removable: Some(vec![]), config_dir: None })
}

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn pattern(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2654435761) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn child(st: &AppState, vault: &str, dir: u64, name: &str) -> Node {
    st.fs_list(vault, dir).unwrap().into_iter().find(|n| n.name == name).unwrap_or_else(|| panic!("{name} missing"))
}

fn assert_same_tree(a: &Path, b: &Path) {
    let mut ea: Vec<_> = std::fs::read_dir(a).unwrap().map(|e| e.unwrap().file_name()).collect();
    let mut eb: Vec<_> = std::fs::read_dir(b).unwrap().map(|e| e.unwrap().file_name()).collect();
    ea.sort();
    eb.sort();
    assert_eq!(ea, eb, "entries of {} vs {}", a.display(), b.display());
    for n in ea {
        let (pa, pb) = (a.join(&n), b.join(&n));
        if pa.is_dir() {
            assert!(pb.is_dir());
            assert_same_tree(&pa, &pb);
        } else {
            assert_eq!(std::fs::read(&pa).unwrap(), std::fs::read(&pb).unwrap(), "{}", pa.display());
        }
    }
}

fn get(st: &AppState, uri: &str, range: Option<&str>) -> tauri::http::Response<Vec<u8>> {
    let mut b = Request::builder().uri(uri);
    if let Some(r) = range {
        b = b.header(header::RANGE, r);
    }
    nicevault_app::preview::handle(st, &b.body(Vec::new()).unwrap())
}

#[test]
fn end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let rec = Rec::default();
    let st = AppState::new(stick_store(root), Box::new(rec.clone()));
    let vol = root.join("Prova.nvault");

    let info = st.vault_create(vol.to_str().unwrap(), pw("segreto"), Some("fast")).unwrap();
    assert_eq!(info.name, "Prova");
    let k = info.kdf.expect("VaultInfo.kdf of the unlocking password slot");
    assert_eq!((k.m_kib, k.t, k.p), (8 * 1024, 1, 1), "fast preset");
    assert_eq!(serde_json::to_value(&info).unwrap()["kdf"], serde_json::json!({ "m_kib": 8192, "t": 1, "p": 1 }));
    let id = info.id.clone();
    assert!(st.vault_create(vol.to_str().unwrap(), pw("x"), Some("fast")).is_err_and(|e| e.code == "exists"));

    let src = root.join("Lavoro");
    std::fs::create_dir_all(src.join("sub/deeper")).unwrap();
    std::fs::create_dir_all(src.join("vuota")).unwrap();
    let big = pattern(3 * 1024 * 1024 + 12345, 7);
    std::fs::write(src.join("note.txt"), "ciao NiceVault\n").unwrap();
    std::fs::write(src.join("sub/big.bin"), &big).unwrap();
    std::fs::write(src.join("sub/deeper/zero.dat"), b"").unwrap();
    std::fs::write(src.join("sub/deeper/foto.jpg"), pattern(70_000, 3)).unwrap();

    let job = nicevault_app::jobs::fs_import(&st, &id, ROOT, vec![src.to_string_lossy().into()]).unwrap();
    let done = rec.job_done(&job);
    assert!(done["error"].is_null(), "import failed: {done}");
    assert!(rec.count("job-progress", &job) >= 2);
    let last = rec.0.lock().unwrap().iter().rev().find(|(e, v)| e == "job-progress" && v["job"] == job).unwrap().1.clone();
    assert_eq!(last["done_bytes"], last["total_bytes"]);
    assert_eq!(last["total_bytes"].as_u64().unwrap(), (big.len() + 15 + 70_000) as u64);

    let job2 = nicevault_app::jobs::fs_import(&st, &id, ROOT, vec![src.join("note.txt").to_string_lossy().into()]).unwrap();
    assert!(rec.job_done(&job2)["error"].is_null());
    let names: Vec<String> = st.fs_list(&id, ROOT).unwrap().into_iter().map(|n| n.name).collect();
    assert!(names.contains(&"Lavoro".into()) && names.contains(&"note.txt".into()), "{names:?}");
    let job3 = nicevault_app::jobs::fs_import(&st, &id, ROOT, vec![src.join("note.txt").to_string_lossy().into()]).unwrap();
    assert!(rec.job_done(&job3)["error"].is_null());
    assert!(st.fs_list(&id, ROOT).unwrap().iter().any(|n| n.name == "note (2).txt"));

    let lavoro = child(&st, &id, ROOT, "Lavoro");
    assert_eq!(lavoro.kind, Kind::Dir);
    let sub = child(&st, &id, lavoro.id, "sub");
    let bigf = child(&st, &id, sub.id, "big.bin");
    assert_eq!(bigf.size, big.len() as u64);
    assert!(st.fs_list(&id, lavoro.id).unwrap().iter().any(|n| n.name == "vuota" && n.kind == Kind::Dir));

    let r = get(&st, &format!("nvault://{id}/{}", bigf.id), Some("bytes=1000-1999"));
    assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(r.body(), &big[1000..2000]);
    assert_eq!(r.headers()[header::CONTENT_RANGE], format!("bytes 1000-1999/{}", big.len()));
    assert_eq!(r.headers()[header::CONTENT_TYPE], "application/octet-stream");
    let r = get(&st, &format!("http://nvault.localhost/{id}/{}", bigf.id), Some("bytes=-100"));
    assert_eq!(r.body(), &big[big.len() - 100..]);
    let r = get(&st, &format!("nvault://{id}/{}", bigf.id), None);
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.body(), &big);
    let r = get(&st, &format!("nvault://{id}/{}", bigf.id), Some(&format!("bytes={}-", big.len())));
    assert_eq!(r.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    let deeper = child(&st, &id, sub.id, "deeper");
    let jpg = child(&st, &id, deeper.id, "foto.jpg");
    assert_eq!(get(&st, &format!("nvault://{id}/{}", jpg.id), None).headers()[header::CONTENT_TYPE], "image/jpeg");
    assert_eq!(get(&st, &format!("nvault://nope/{}", jpg.id), None).status(), StatusCode::NOT_FOUND);

    let out = root.join("out");
    std::fs::create_dir(&out).unwrap();
    let job = nicevault_app::jobs::fs_export(&st, &id, vec![lavoro.id], out.to_str().unwrap()).unwrap();
    let done = rec.job_done(&job);
    assert!(done["error"].is_null(), "export failed: {done}");
    assert_same_tree(&src, &out.join("Lavoro"));

    let job = nicevault_app::jobs::fs_export(&st, &id, vec![lavoro.id], out.to_str().unwrap()).unwrap();
    st.job_cancel(&job);
    let done = rec.job_done(&job);
    assert!(done["error"].is_null() || done["error"]["code"] == "cancelled", "{done}");

    let d = st.fs_mkdir(&id, ROOT, " Nuova ").unwrap();
    assert_eq!(d.name, "Nuova");
    assert_eq!(st.fs_mkdir(&id, ROOT, "Nuova").unwrap_err().code, "exists");
    assert_eq!(st.fs_rename(&id, lavoro.id, sub.id, "x").unwrap_err().code, "invalid_name");
    assert_eq!(st.fs_rename(&id, d.id, ROOT, "Lavoro").unwrap_err().code, "exists");
    st.fs_rename(&id, d.id, lavoro.id, "Spostata").unwrap();
    st.fs_delete(&id, &[child(&st, &id, lavoro.id, "Spostata").id]).unwrap();

    let s = st.vault_stats(&id).unwrap();
    assert_eq!(s.file_count, 6);
    assert!(s.file_size_bytes > 0);

    use nicevault_app::preview::{MAX_BODY, MAX_RANGE};
    let v = st.vault(&id).unwrap();
    let video = nv_fs::Fs::create(&*v, ROOT, "film.mp4", Kind::File).unwrap();
    let huge = nv_fs::Fs::create(&*v, ROOT, "huge.bin", Kind::File).unwrap();
    let block = pattern(4 << 20, 11);
    let (vsize, hsize) = (MAX_RANGE + 12345, MAX_BODY + 777);
    for (node, size) in [(video.id, vsize), (huge.id, hsize)] {
        let mut off = 0;
        while off < size {
            let k = (size - off).min(block.len() as u64) as usize;
            nv_fs::Fs::write(&*v, node, off, &block[..k]).unwrap();
            off += k as u64;
        }
    }
    nv_fs::Fs::flush(&*v).unwrap();
    drop(v);
    let r = get(&st, &format!("nvault://{id}/{}", video.id), None);
    assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(r.body().len() as u64, MAX_RANGE);
    assert_eq!(r.headers()[header::CONTENT_RANGE], format!("bytes 0-{}/{vsize}", MAX_RANGE - 1));
    assert_eq!(r.headers()[header::ACCEPT_RANGES], "bytes");
    assert_eq!(&r.body()[..1000], &block[..1000]);
    let r = get(&st, &format!("nvault://{id}/{}", video.id), Some(&format!("bytes={MAX_RANGE}-")));
    assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(r.body().len(), 12345, "the follow-up gets the rest");
    let r = get(&st, &format!("nvault://{id}/{}", huge.id), None);
    assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(r.body().len() as u64, MAX_BODY);
    let r = get(&st, &format!("nvault://{id}/{}", huge.id), Some(&format!("bytes=0-{}", hsize - 1)));
    assert_eq!(r.body().len() as u64, MAX_BODY, "explicit ranges are capped too");
    assert_eq!(r.headers()[header::CONTENT_RANGE], format!("bytes 0-{}/{hsize}", MAX_BODY - 1));
    drop(r);
    st.fs_delete(&id, &[video.id, huge.id]).unwrap();

    let slots = st.slots_list(&id).unwrap();
    assert_eq!(slots.len(), 1);
    assert!(slots[0].in_use && slots[0].label.is_none());
    let slots = st.slot_add_password(&id, pw("seconda"), Some("Busta".into())).unwrap();
    assert_eq!(slots.len(), 2);
    assert_eq!(slots[1].label.as_deref(), Some("Busta"));
    assert!(slots[1].kdf.is_some_and(|k| k.m_kib >= 64 * 1024), "app adds strong slots: {:?}", slots[1].kdf);
    assert_eq!(slots.iter().map(|s| s.in_use).collect::<Vec<_>>(), [true, false]);
    let j = serde_json::to_value(&slots[1]).unwrap();
    for key in ["index", "kind", "label", "kdf", "fingerprint", "in_use"] {
        assert!(j.get(key).is_some(), "SlotInfo.{key} missing: {j}");
    }
    let e = st.slot_change_password(&id, 1, pw("sbagliata"), pw("terza"), None).unwrap_err();
    assert_eq!(e.code, "wrong_password");
    let e = st.slot_change_password(&id, 1, pw("segreto"), pw("terza"), None).unwrap_err();
    assert_eq!(e.code, "wrong_password", "password of another slot");
    assert_eq!(st.slot_change_password(&id, 5, pw("x"), pw("y"), None).unwrap_err().code, "not_found");
    assert_eq!(st.slot_change_password(&id, 1, pw("seconda"), pw(""), None).unwrap_err().code, "invalid_password");
    let slots = st.slot_change_password(&id, 1, pw("seconda"), pw("terza"), Some("Busta nuova".into())).unwrap();
    assert_eq!(slots[1].label.as_deref(), Some("Busta nuova"));
    assert!(st.vault(&id).unwrap().verify_password(1, &pw("terza")));
    let slots = st.slot_remove(&id, 1).unwrap();
    assert_eq!(slots.len(), 1);
    assert_eq!(st.slot_remove(&id, 0).unwrap_err().code, "last_slot");
    assert_eq!(st.slot_remove(&id, 3).unwrap_err().code, "not_found");
    use nicevault_app::error::ApiError;
    assert_eq!(ApiError::from(nv_core::SlotError::Full).code, "slots_full");
    assert_eq!(ApiError::from(nv_core::SlotError::NotPassword).code, "invalid_slot");
    assert_eq!(ApiError::from(nv_core::SlotError::Fs(nv_fs::Error::Locked)).code, "locked");

    let backends = st.mount_available();
    assert_eq!(backends.len(), 3);
    if backends.iter().any(|b| b.id == "fuse" && b.available) {
        let m = st.mount(&id).unwrap();
        let mp = PathBuf::from(&m.mountpoint);
        assert_eq!(std::fs::read(mp.join("Lavoro/sub/big.bin")).unwrap(), big);
        assert_eq!(std::fs::read_to_string(mp.join("Lavoro/note.txt")).unwrap(), "ciao NiceVault\n");
        assert_eq!(st.vault_list_open()[0].mountpoint.as_deref(), Some(m.mountpoint.as_str()));
        st.unmount(&id).unwrap();
        assert!(!mp.join("Lavoro").exists());
        eprintln!("FUSE mount verified at {}", m.mountpoint);
    } else {
        eprintln!("FUSE unavailable here: mount step skipped");
    }

    let note = child(&st, &id, lavoro.id, "note.txt");
    let ext = st.extract_external(&id, note.id).unwrap();
    assert_eq!(std::fs::read_to_string(&ext).unwrap(), "ciao NiceVault\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(ext.parent().unwrap().parent().unwrap()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&ext, "modificato dall'editor").unwrap();
    st.poll_external();
    st.poll_external();
    let r = get(&st, &format!("nvault://{id}/{}", note.id), None);
    assert_eq!(r.body(), b"modificato dall'editor");

    st.vault_lock(&id, "idle").unwrap();
    let ev = rec.wait(Duration::from_secs(5), |e, v| e == "vault-locked" && v["vault"] == id.as_str());
    assert_eq!(ev["reason"], "idle");
    assert!(!ext.exists(), "external copy must be wiped on lock");
    assert_eq!(st.fs_list(&id, ROOT).unwrap_err().code, "locked");
    assert!(st.vault_list_open().is_empty());
    st.vault_lock(&id, "user").unwrap();
    for side in ["Prova.nvault-wal", "Prova.nvault-shm"] {
        assert!(!root.join(side).exists(), "{side} left behind");
    }

    assert_eq!(st.vault_open(vol.to_str().unwrap(), pw("sbagliata")).unwrap_err().code, "wrong_password");
    let info = st.vault_open(vol.to_str().unwrap(), pw("segreto")).unwrap();
    let id = info.id;
    let lavoro = child(&st, &id, ROOT, "Lavoro");
    let note = child(&st, &id, lavoro.id, "note.txt");
    assert_eq!(get(&st, &format!("nvault://{id}/{}", note.id), None).body(), b"modificato dall'editor");
    let sub = child(&st, &id, lavoro.id, "sub");
    assert_eq!(get(&st, &format!("nvault://{id}/{}", child(&st, &id, sub.id, "big.bin").id), None).body(), &big);
    let again = st.vault_open(vol.to_str().unwrap(), pw("segreto")).unwrap();
    assert_eq!(again.id, id, "opening an open volume returns the same session");

    let recent = st.recent_list();
    assert_eq!(recent.len(), 1);
    assert!(recent[0].exists && recent[0].unlock_ms.is_some() && recent[0].size_bytes > 0);
    assert_eq!(recent[0].source, "here");
    assert!(!root.join("NiceVault.json").exists());
    let s = st.settings_set(serde_json::from_str(r#"{"auto_lock_minutes": 5}"#).unwrap(), None).unwrap();
    assert_eq!(s.auto_lock_minutes, 5);
    assert!(st.recent_forget(vol.to_str().unwrap()).unwrap().is_empty());

    let stick = root.join("stick");
    std::fs::create_dir(&stick).unwrap();
    let v2 = st.vault_create(stick.join("Due.nvault").to_str().unwrap(), pw("p"), Some("fast")).unwrap();
    std::fs::rename(&stick, root.join("stick-gone")).unwrap();
    let ev = rec.wait(Duration::from_secs(5), |e, v| e == "vault-locked" && v["vault"] == v2.id.as_str());
    assert_eq!(ev["reason"], "removed");
    assert_eq!(st.fs_list(&v2.id, ROOT).unwrap_err().code, "locked");

    st.lock_all("user");
    assert!(st.vault_list_open().is_empty());
}

#[test]
fn stale_external_temp_folders_are_wiped() {
    use nicevault_app::external::cleanup_stale;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let mk = |name: &str, lock: bool| {
        let d = root.join(name);
        std::fs::create_dir_all(d.join("abcd")).unwrap();
        std::fs::write(d.join("abcd/secret.txt"), b"decrypted").unwrap();
        if lock {
            std::fs::write(d.join(".lock"), b"").unwrap();
        }
        d
    };
    let crashed = mk("nicevault-0123456789abcdef", true);
    let live = mk("nicevault-fedcba9876543210", true);
    let held = std::fs::File::options().write(true).open(live.join(".lock")).unwrap();
    held.try_lock().unwrap();
    let fresh = mk("nicevault-00000000000000aa", false);
    let other = mk("nicevault-notours", true);
    let foreign = mk("someone-else-0123456789abcdef", true);
    #[cfg(unix)]
    let link = {
        let l = root.join("nicevault-1111111111111111");
        std::os::unix::fs::symlink(&foreign, &l).unwrap();
        l
    };

    assert_eq!(cleanup_stale(root), 1);
    assert!(!crashed.exists(), "stale folder wiped");
    assert!(live.join("abcd/secret.txt").exists(), "a live instance's folder is kept");
    assert!(fresh.join("abcd/secret.txt").exists(), "an unlocked folder younger than the grace period is kept");
    assert!(other.exists() && foreign.join("abcd/secret.txt").exists(), "names that don't match are kept");
    #[cfg(unix)]
    assert!(std::fs::symlink_metadata(&link).is_ok(), "symlinks are never followed or removed");
    drop(held);
    assert_eq!(cleanup_stale(root), 1, "once released, it is stale too");
    assert!(!live.exists());

    let st = AppState::new(stick_store(root), Box::new(Rec::default()));
    let info = st.vault_create(root.join("C.nvault").to_str().unwrap(), pw("p"), Some("fast")).unwrap();
    let n = nv_fs::Fs::create(&*st.vault(&info.id).unwrap(), ROOT, "a.txt", Kind::File).unwrap();
    let ext = st.extract_external(&info.id, n.id).unwrap();
    cleanup_stale(&nicevault_app::external::temp_root());
    assert!(ext.exists(), "our own live external copy must survive");
    st.lock_all("user");
    assert!(!ext.exists());
}

