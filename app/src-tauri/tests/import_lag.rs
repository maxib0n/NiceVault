use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events};
use nv_core::slowio::{self, SlowIo};
use nv_fs::{Fs, ROOT};
use secrecy::SecretString;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Ev {
    done: Mutex<Option<serde_json::Value>>,
    cv: Condvar,
}

struct Sink(Arc<Ev>);
impl Events for Sink {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        if event == "job-done" {
            *self.0.done.lock().unwrap() = Some(payload);
            self.0.cv.notify_all();
        }
    }
}

fn fill(seed: u64, buf: &mut [u8]) {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for w in buf.chunks_mut(8) {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        w.copy_from_slice(&x.to_le_bytes()[..w.len()]);
    }
}

fn make_sources(dir: &Path, files: usize, mb_each: usize) -> Vec<String> {
    let mut buf = vec![0u8; 1 << 20];
    (0..files)
        .map(|i| {
            let p = dir.join(format!("video{i}.bin"));
            let mut f = std::fs::File::create(&p).unwrap();
            for b in 0..mb_each {
                fill(((i as u64) << 32) | b as u64, &mut buf);
                f.write_all(&buf).unwrap();
            }
            p.to_string_lossy().into_owned()
        })
        .collect()
}

#[derive(Debug)]
struct Lat {
    name: &'static str,
    ms: Vec<f64>,
}

impl Lat {
    fn pct(&self, p: f64) -> f64 {
        let mut v = self.ms.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if v.is_empty() {
            return 0.0;
        }
        v[((v.len() - 1) as f64 * p).round() as usize]
    }
    fn max(&self) -> f64 {
        self.ms.iter().copied().fold(0.0, f64::max)
    }
    fn line(&self) -> String {
        format!("{:<16} n={:<5} p50={:>7.1} p95={:>7.1} p99={:>7.1} max={:>8.1} ms", self.name, self.ms.len(), self.pct(0.5), self.pct(0.95), self.pct(0.99), self.max())
    }
}

struct Report {
    secs: f64,
    mb: f64,
    lats: Vec<Lat>,
}

fn run(slow: Option<SlowIo>, files: usize, mb_each: usize, every: Duration) -> Report {
    let dir = tempfile::tempdir().unwrap();
    let host = dir.path().join("host");
    std::fs::create_dir(&host).unwrap();
    let srcs = make_sources(&host, files, mb_each);
    let stick = dir.path().join("stick");
    std::fs::create_dir(&stick).unwrap();
    let ev = Arc::new(Ev::default());
    let st = AppState::new(
        Store::new(Locations { exe_dir: Some(stick.clone()), removable: Some(vec![]), config_dir: None }),
        Box::new(Sink(ev.clone())),
    );
    let cap = ((files * mb_each) as u64 + 256) << 20;
    let info = st
        .vault_create_sized(stick.join("VaultData.nvault").to_str().unwrap(), SecretString::from("pw".to_owned()), Some("fast"), Some(cap))
        .unwrap();
    let v = st.vault(&info.id).unwrap();
    let pic = v.create(ROOT, "foto.jpg", nv_fs::Kind::File).unwrap();
    let mut b = vec![0u8; 300 << 10];
    fill(7, &mut b);
    v.write(pic.id, 0, &b).unwrap();
    v.flush().unwrap();
    drop(v);

    slowio::set(slow);
    let stop = Arc::new(AtomicBool::new(false));
    type Call = Box<dyn Fn(&AppState, &str) + Send>;
    let calls: Vec<(&'static str, Call)> = vec![
        ("fs_list", Box::new(|st, id| drop(st.fs_list(id, ROOT).unwrap()))),
        ("vault_list_open", Box::new(|st, _| drop(st.vault_list_open()))),
        (
            "read 300 KB",
            Box::new(move |st, id| {
                let mut buf = vec![0u8; 300 << 10];
                st.vault(id).unwrap().read(pic.id, 0, &mut buf).unwrap();
            }),
        ),
        ("vault_stats", Box::new(|st, id| {
            st.vault_stats(id).unwrap();
        })),
    ];
    let readers: Vec<_> = calls
        .into_iter()
        .map(|(name, call)| {
            let (st, id, stop) = (st.clone(), info.id.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut l = Lat { name, ms: vec![] };
                while !stop.load(Ordering::Relaxed) {
                    let t = Instant::now();
                    call(&st, &id);
                    l.ms.push(t.elapsed().as_secs_f64() * 1e3);
                    std::thread::sleep(every);
                }
                l
            })
        })
        .collect();
    let t0 = Instant::now();
    nicevault_app::jobs::fs_import(&st, &info.id, ROOT, srcs).unwrap();
    let mut g = ev.done.lock().unwrap();
    while g.is_none() {
        g = ev.cv.wait(g).unwrap();
    }
    let secs = t0.elapsed().as_secs_f64();
    assert!(g.as_ref().unwrap()["error"].is_null(), "import failed: {g:?}");
    drop(g);
    stop.store(true, Ordering::Relaxed);
    let lats: Vec<Lat> = readers.into_iter().map(|r| r.join().unwrap()).collect();
    slowio::set(None);
    let names: Vec<String> = st.fs_list(&info.id, ROOT).unwrap().into_iter().map(|n| n.name).collect();
    assert_eq!(names.len(), files + 1, "{names:?}");
    st.vault_lock(&info.id, "user").unwrap();
    Report { secs, mb: (files * mb_each) as f64 * 1.048576, lats }
}

fn print(label: &str, r: &Report) {
    println!("--- {label}: {:.0} MB in {:.1} s = {:.1} MB/s", r.mb, r.secs, r.mb / r.secs);
    for l in &r.lats {
        println!("  {}", l.line());
    }
}

#[test]
fn listing_stays_fast_during_import() {
    let r = run(Some(SlowIo { bytes_per_sec: 20e6, dirty_cap: 4_000_000_000, sync_ms: 20 }), 3, 80, Duration::from_millis(20));
    print("regression (20 MB/s stick)", &r);
    for l in &r.lats {
        assert!(l.ms.len() >= 20, "{}: too few samples", l.name);
        assert!(l.pct(0.98) < 100.0, "{}: {}", l.name, l.line());
        assert!(l.max() < 1000.0, "{}: {}", l.name, l.line());
    }
}

#[test]
#[ignore]
fn measure_import_lag() {
    let mb: usize = std::env::var("NV_LAG_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(1200);
    let slow = match std::env::var("NV_LAG_SLOW").as_deref() {
        Ok("off") => None,
        Ok(s) => SlowIo::parse(s),
        Err(_) => SlowIo::parse("15,4000,20"),
    };
    let r = run(slow, 3, mb / 3, Duration::from_millis(50));
    print(&format!("{mb} MB, slow={slow:?}"), &r);
}
