use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlowIo {
    pub bytes_per_sec: f64,
    pub dirty_cap: u64,
    pub sync_ms: u64,
}

impl SlowIo {
    pub fn parse(s: &str) -> Option<SlowIo> {
        let mut it = s.split(',').map(|p| p.trim().parse::<f64>().ok());
        let (Some(Some(mbs)), Some(Some(cap)), Some(Some(ms))) = (it.next(), it.next(), it.next()) else { return None };
        (mbs > 0.0 && cap >= 0.0 && ms >= 0.0).then_some(SlowIo { bytes_per_sec: mbs * 1e6, dirty_cap: (cap * 1e6) as u64, sync_ms: ms as u64 })
    }
}

struct Pool {
    cfg: Option<SlowIo>,
    dirty: f64,
    at: Instant,
}

static ON: AtomicBool = AtomicBool::new(false);
static WRITTEN: AtomicU64 = AtomicU64::new(0);

pub(crate) fn written() -> u64 {
    WRITTEN.load(Relaxed)
}
static POOL: Mutex<Option<Pool>> = Mutex::new(None);

pub fn set(cfg: Option<SlowIo>) {
    *POOL.lock().unwrap_or_else(|e| e.into_inner()) = Some(Pool { cfg, dirty: 0.0, at: Instant::now() });
    ON.store(cfg.is_some(), Relaxed);
}

pub(crate) fn from_env() {
    if let Some(c) = std::env::var("NICEVAULT_TEST_SLOW_IO").ok().as_deref().and_then(SlowIo::parse) {
        set(Some(c));
    }
}

fn drain(p: &mut Pool, c: &SlowIo) {
    let now = Instant::now();
    p.dirty = (p.dirty - now.duration_since(p.at).as_secs_f64() * c.bytes_per_sec).max(0.0);
    p.at = now;
}

pub(crate) fn write(n: usize) {
    WRITTEN.fetch_add(n as u64, Relaxed);
    if !ON.load(Relaxed) {
        return;
    }
    let wait = {
        let mut g = POOL.lock().unwrap_or_else(|e| e.into_inner());
        let Some(p) = g.as_mut() else { return };
        let Some(c) = p.cfg else { return };
        drain(p, &c);
        p.dirty += n as f64;
        let over = p.dirty - c.dirty_cap as f64;
        if over > 0.0 { over / c.bytes_per_sec } else { 0.0 }
    };
    if wait > 0.0 {
        std::thread::sleep(Duration::from_secs_f64(wait));
    }
}

pub(crate) fn sync() {
    flush(true);
}

pub(crate) fn writeback() {
    flush(false);
}

fn flush(device: bool) {
    if !ON.load(Relaxed) {
        return;
    }
    let wait = {
        let mut g = POOL.lock().unwrap_or_else(|e| e.into_inner());
        let Some(p) = g.as_mut() else { return };
        let Some(c) = p.cfg else { return };
        drain(p, &c);
        p.dirty / c.bytes_per_sec + if device { c.sync_ms as f64 / 1000.0 } else { 0.0 }
    };
    std::thread::sleep(Duration::from_secs_f64(wait));
}
