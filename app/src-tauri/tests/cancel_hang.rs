use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events};
use nv_core::slowio::{self, SlowIo};
use nv_fs::ROOT;
use secrecy::SecretString;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Ev {
    done: Mutex<Vec<(String, serde_json::Value)>>,
    bytes: AtomicU64,
    cv: Condvar,
}

struct Sink(Arc<Ev>);
impl Events for Sink {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        match event {
            "job-done" => {
                self.0.done.lock().unwrap().push((payload["job"].as_str().unwrap_or_default().to_owned(), payload));
                self.0.cv.notify_all();
            }
            "job-progress" => self.0.bytes.store(payload["done_bytes"].as_u64().unwrap_or(0), Ordering::SeqCst),
            _ => {}
        }
    }
}

impl Ev {
    fn wait_done(&self, job: &str, max: Duration) -> Option<serde_json::Value> {
        let t0 = Instant::now();
        let mut g = self.done.lock().unwrap();
        loop {
            if let Some((_, v)) = g.iter().find(|(j, _)| j == job) {
                return Some(v.clone());
            }
            let left = max.checked_sub(t0.elapsed())?;
            g = self.cv.wait_timeout(g, left).unwrap().0;
        }
    }
}

#[test]
fn cancelled_import_ends_and_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let host = dir.path().join("host");
    std::fs::create_dir(&host).unwrap();
    let src = host.join("film.mkv");
    {
        let mut f = std::fs::File::create(&src).unwrap();
        let mut buf = vec![0u8; 1 << 20];
        for k in 0..240u64 {
            buf[..8].copy_from_slice(&k.to_le_bytes());
            f.write_all(&buf).unwrap();
        }
    }
    let stick = dir.path().join("stick");
    std::fs::create_dir(&stick).unwrap();
    let ev = Arc::new(Ev::default());
    let st = AppState::new(
        Store::new(Locations { exe_dir: Some(stick.clone()), removable: Some(vec![]), config_dir: None }),
        Box::new(Sink(ev.clone())),
    );
    let info = st
        .vault_create_sized(stick.join("VaultData.nvault").to_str().unwrap(), SecretString::from("pw".to_owned()), Some("fast"), Some(64 << 20))
        .unwrap();
    let id = info.id.clone();
    st.vault_resize(&id, 512 << 20).unwrap();

    slowio::set(Some(SlowIo { bytes_per_sec: 40e6, dirty_cap: 4_000_000_000, sync_ms: 20 }));
    let stop = Arc::new(AtomicBool::new(false));
    let worst = Arc::new(AtomicU64::new(0));
    let lister = {
        let (st, id, stop, worst) = (st.clone(), id.clone(), stop.clone(), worst.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let t = Instant::now();
                st.fs_list(&id, ROOT).unwrap();
                worst.fetch_max(t.elapsed().as_millis() as u64, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(50));
            }
        })
    };
    for cancel_at_mb in [0u64, 2, 30, 150] {
        ev.bytes.store(0, Ordering::SeqCst);
        let job = nicevault_app::jobs::fs_import(&st, &id, ROOT, vec![src.to_string_lossy().into_owned()]).unwrap();
        let t0 = Instant::now();
        while ev.bytes.load(Ordering::SeqCst) < cancel_at_mb << 20 && t0.elapsed() < Duration::from_secs(60) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let tc = Instant::now();
        st.job_cancel(&job);
        let done = ev.wait_done(&job, Duration::from_secs(20)).unwrap_or_else(|| panic!("cancel at {cancel_at_mb} MiB: no job-done"));
        eprintln!("cancel at {cancel_at_mb} MiB: job-done after {:?}: {done}", tc.elapsed());
        assert!(done["error"].is_null() || done["error"]["code"] == "cancelled", "{done}");
        let names: Vec<String> = st.fs_list(&id, ROOT).unwrap().into_iter().map(|n| n.name).collect();
        if done["error"].is_null() {
            assert_eq!(names, vec!["film.mkv".to_owned()]);
            let v = st.vault(&id).unwrap();
            let n = nv_fs::Fs::lookup(&*v, ROOT, "film.mkv").unwrap();
            v.discard(n.id).unwrap();
        } else {
            assert!(names.is_empty(), "cancel at {cancel_at_mb} MiB left {names:?}");
        }
    }
    stop.store(true, Ordering::SeqCst);
    lister.join().unwrap();
    slowio::set(None);
    let w = worst.load(Ordering::SeqCst);
    eprintln!("worst fs_list {w} ms");
    assert!(w < 1000, "an fs_list took {w} ms");
    st.vault(&id).unwrap().check_integrity().unwrap();
    st.vault_lock(&id, "user").unwrap();
}
