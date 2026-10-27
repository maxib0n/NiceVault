use nv_core::{KdfPreset, Vault, slowio};
use nv_fs::{Fs, Kind, ROOT};
use secrecy::SecretString;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static SERIAL: Mutex<()> = Mutex::new(());

fn pw() -> SecretString {
    SecretString::from("pw".to_owned())
}

fn slow(mbs: f64) {
    slowio::set(Some(slowio::SlowIo { bytes_per_sec: mbs * 1e6, dirty_cap: 4_000_000_000, sync_ms: 20 }));
}

struct Lister {
    stop: Arc<AtomicBool>,
    worst: Arc<AtomicU64>,
    h: std::thread::JoinHandle<()>,
}

impl Lister {
    fn start(v: &Arc<Vault>) -> Lister {
        let (stop, worst) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU64::new(0)));
        let (v, s, w) = (v.clone(), stop.clone(), worst.clone());
        let h = std::thread::spawn(move || {
            while !s.load(SeqCst) {
                let t = Instant::now();
                v.readdir(ROOT).unwrap();
                w.fetch_max(t.elapsed().as_millis() as u64, SeqCst);
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        Lister { stop, worst, h }
    }
    fn finish(self) -> u64 {
        self.stop.store(true, SeqCst);
        self.h.join().unwrap();
        self.worst.load(SeqCst)
    }
}

fn grown_volume(path: &Path, cap: u64) -> Arc<Vault> {
    let v = Vault::create_fixed(path, &pw(), KdfPreset::Fast, 64 << 20).unwrap();
    let keep = v.create(ROOT, "keep.txt", Kind::File).unwrap();
    v.write(keep.id, 0, b"still here").unwrap();
    v.flush().unwrap();
    v.resize(cap, &|_| {}).unwrap();
    Arc::new(v)
}

fn import_then_cancel(v: &Vault, cancel_at: u64) -> Duration {
    let f = v.create(ROOT, "film.mkv", Kind::File).unwrap();
    let mut block = vec![0u8; 1 << 20];
    for k in 0..cancel_at {
        block[..8].copy_from_slice(&k.to_le_bytes());
        v.write(f.id, k << 20, &block).unwrap();
    }
    let t = Instant::now();
    v.discard(f.id).unwrap();
    v.flush().unwrap();
    t.elapsed()
}

fn only_keep(v: &Vault) {
    let names: Vec<String> = v.readdir(ROOT).unwrap().into_iter().map(|n| n.name).collect();
    assert_eq!(names, vec!["keep.txt".to_owned()], "no half-imported file");
    v.check_integrity().unwrap();
}

#[test]
fn cancelled_import_keeps_the_volume_responsive() {
    let _s = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stick.nvault");
    let v = grown_volume(&path, 400 << 20);
    slow(40.0);
    for cancel_at in [0, 1, 3, 5, 37, 160] {
        let l = Lister::start(&v);
        let cleanup = import_then_cancel(&v, cancel_at);
        let worst = l.finish();
        eprintln!("cancel at {cancel_at} MiB: cleanup {cleanup:?}, worst listing {worst} ms");
        assert!(worst < 1000, "cancel at {cancel_at} MiB: a listing took {worst} ms");
        assert!(cleanup < Duration::from_secs(20), "cancel at {cancel_at} MiB: cleanup took {cleanup:?}");
        only_keep(&v);
    }
    slowio::set(None);
    let stats = v.stats().unwrap();
    assert!(stats.free_bytes > 300 << 20, "the room came back: {stats:?}");
    drop(v);
    let v = Vault::open(&path, &pw()).unwrap();
    only_keep(&v);
}

#[test]
fn stepwise_secure_delete_keeps_listings_fast() {
    let _s = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let v = grown_volume(&dir.path().join("stick.nvault"), 256 << 20);
    let f = v.create(ROOT, "big.bin", Kind::File).unwrap();
    let block = vec![0x5au8; 1 << 20];
    for k in 0..64u64 {
        v.write(f.id, k << 20, &block).unwrap();
    }
    v.flush().unwrap();
    slow(15.0);
    let l = Lister::start(&v);
    let mut size = 64u64 << 20;
    let step = 4u64 << 20;
    while size > step {
        size -= step;
        v.set_size(f.id, size).unwrap();
        v.flush().unwrap();
    }
    v.remove(f.id).unwrap();
    v.flush().unwrap();
    let worst = l.finish();
    slowio::set(None);
    eprintln!("worst listing {worst} ms");
    assert!(worst < 1000, "a listing took {worst} ms");
    only_keep(&v);
}

#[test]
fn discard_basics() {
    let _s = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let v = Vault::create(&dir.path().join("d.nvault"), &pw(), KdfPreset::Fast).unwrap();
    let big = v.create(ROOT, "big", Kind::File).unwrap();
    let block = vec![7u8; 1 << 20];
    for k in 0..(nv_core::DISCARD_STEP >> 20) * 2 + 3 {
        v.write(big.id, k << 20, &block).unwrap();
    }
    let seq = v.commit_seq().unwrap();
    v.discard(big.id).unwrap();
    assert!(v.commit_seq().unwrap() >= seq + 3, "one durability point per step");
    assert!(matches!(v.getattr(big.id), Err(nv_fs::Error::NotFound)));
    v.discard(big.id).unwrap();
    let d = v.create(ROOT, "dir", Kind::Dir).unwrap();
    assert!(matches!(v.discard(d.id), Err(nv_fs::Error::IsDir)));
    v.check_integrity().unwrap();
}
