//! Slow-drive emulation for tests and measurements (never on unless asked for): models a USB
//! stick behind the kernel's page cache, so lock-hold and latency problems that only show up on
//! slow media can be reproduced on a fast disk without root (no loop devices, no dm-delay).
//!
//! Model: writes land in a "dirty" pool that drains in the background at `bytes_per_sec`; a
//! write that would push the pool past `dirty_cap` waits until it fits (balance_dirty_pages); a
//! sync waits until the pool is empty, plus `sync_ms` (the device's cache flush). Every file the
//! store writes through the VFS shims (side-file and single-file) shares the one pool, as they
//! share one device.
//!
//! Turned on with [`set`] (tests, benches) or, once per process, from the environment:
//! `NICEVAULT_TEST_SLOW_IO=<MB/s>,<dirty cap MB>,<sync ms>` (e.g. `15,64,20`; the real app under
//! the import-lag measurement).

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
    /// `<MB/s>,<dirty cap MB>,<sync ms>`.
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
/// Bytes the VFS shims handed to the OS so far (every store of the process; always counted, the
/// emulation on or off): what the store's writeback has to write back, deletes and journal pages
/// included (cancel-hang: a secure delete wrote GBs that the store never counted, so the COMMIT
/// synced them under the store lock).
static WRITTEN: AtomicU64 = AtomicU64::new(0);

/// See [`WRITTEN`].
pub(crate) fn written() -> u64 {
    WRITTEN.load(Relaxed)
}
static POOL: Mutex<Option<Pool>> = Mutex::new(None);

/// Turns the emulation on (Some) or off (None) for the whole process.
pub fn set(cfg: Option<SlowIo>) {
    *POOL.lock().unwrap_or_else(|e| e.into_inner()) = Some(Pool { cfg, dirty: 0.0, at: Instant::now() });
    ON.store(cfg.is_some(), Relaxed);
}

/// Reads `NICEVAULT_TEST_SLOW_IO` (called when the VFS shims register).
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

/// A write of `n` bytes: waits while the dirty pool is over its cap.
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

/// A sync: waits until everything written so far is on the device, plus the flush.
pub(crate) fn sync() {
    flush(true);
}

/// A writeback without the device cache flush (sync_file_range).
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
