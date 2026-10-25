//! "Estendi / Riduci": a new capacity for a fixed-size (preallocated) volume.
//!
//! The header's `fixed` capacity is the commit point. The footer keeps two records and the shim
//! only takes the one of the header's capacity, so a record for the new capacity can be written
//! next to the current one before the header names it. Every step is synced before the next:
//!
//!  - **grow** C0 -> C1: allocate the file to C1 · footer record for C1 · header C1. Interrupted
//!    before the header: the volume opens at C0 and the open cuts the file back to C0.
//!  - **shrink in place** (the database's extent fits C1): footer record for C1 · header C1 · cut
//!    the file to C1. Interrupted after the header: the open finishes the cut.
//!  - **shrink with compaction** (free pages in the tail): the store is exported, packed, into a
//!    new fixed volume of C1 next to this one (`<vol>-resize`, same master key and slots), which
//!    is renamed over it once complete and synced (like a key rotation). Interrupted before the
//!    rename: the old volume is intact and the leftover is removed at the next open.
//!
//! Nothing ever moves data the database still uses outside the capacity a valid header names.
//!
//! A **single-file** fixed-size volume (header `journal` too, single.rs) works the same way: its
//! journal records carry the capacity like the footer records, the `nvsingle` shim answers the
//! same file control, and a compaction builds a single-file copy (same region size).

use crate::header::{Fixed, JournalRegion, STORE_AT};
use crate::{CIPHER_PRAGMAS, Error, HEADER_SEQ, Inner, Layout, Result, Vault, dberr, init_store, ioerr, pq, side_path, single, sync_dir, vfs, write_headers};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use zeroize::Zeroizing;

/// Room a shrink leaves above the data in use: the larger of this and 1/32 of it.
pub const RESIZE_MARGIN_MIN: u64 = 8 << 20;

/// What "Estendi / Riduci" can do with a volume now.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ResizeInfo {
    /// The fixed capacity; `None` = a growable volume (its file follows its content: nothing to resize).
    pub capacity: Option<u64>,
    /// Length of the volume file.
    pub file_bytes: u64,
    /// Store bytes holding data (pages in use; free pages left out).
    pub used_bytes: u64,
    /// Smallest capacity a shrink accepts (data in use + margin; = capacity when it cannot shrink).
    pub min_capacity: u64,
    /// Smallest capacity reachable without compaction (no free space needed on the drive).
    /// Between `min_capacity` and this, the shrink compacts into a copy of the new size.
    pub min_in_place: u64,
    /// Largest capacity the drive has room for (and the format allows).
    pub max_capacity: u64,
    /// Free space on the volume's drive.
    pub drive_free: u64,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResizePhase {
    /// Packing the store into the new, smaller copy (`done` / `total` bytes, estimated).
    Compact,
    /// Allocating the larger file.
    Extend,
    /// Writing the new size into the footer and the header.
    Commit,
    /// Cutting the file to its new size.
    Truncate,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct ResizeProgress {
    pub phase: ResizePhase,
    pub done: u64,
    pub total: u64,
}

/// Why a resize was refused. Nothing was changed in any of these cases.
#[derive(Debug, thiserror::Error)]
pub enum ResizeError {
    #[error("not a fixed-size volume: it grows by itself")]
    NotFixed,
    #[error("size out of range ({min}..={max} bytes)")]
    OutOfRange { min: u64, max: u64 },
    #[error("too small for the data in the volume (at least {min} bytes)")]
    TooSmall { min: u64 },
    #[error("not enough free space on the drive ({need} bytes needed, {free} free)")]
    NoSpace { need: u64, free: u64 },
    #[error(transparent)]
    Fs(#[from] Error),
}

/// Where a compacting shrink builds the new volume.
pub(crate) fn side(path: &Path) -> PathBuf {
    side_path(path, "-resize")
}

/// Crash injection (tests): the process aborts at the step [`CRASH_AT`] names.
#[cfg(test)]
pub(crate) static CRASH_AT: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
#[cfg(test)]
pub(crate) fn failpoint(name: &str) {
    if *CRASH_AT.lock().unwrap_or_else(|e| e.into_inner()) == name {
        std::process::exit(134); // simulated crash: no core dump, so no DrKonqi/abrt popup on the desktop
    }
}
#[cfg(not(test))]
#[inline(always)]
pub(crate) fn failpoint(_: &str) {}

/// At open, under the exclusive lock, once the header (capacity `cap`) is authentic: a file
/// longer than `cap` is a resize that stopped between its steps (grow before its header, shrink
/// after it). The bytes past `cap` are outside the volume either way: cut them.
pub(crate) fn settle(f: &File, cap: u64) -> std::io::Result<()> {
    if f.metadata()?.len() > cap {
        f.set_len(cap)?;
        f.sync_all()?;
    }
    Ok(())
}

fn up4k(n: u64) -> u64 {
    n.div_ceil(4096) * 4096
}

fn margin(used: u64) -> u64 {
    (used / 32).max(RESIZE_MARGIN_MIN)
}

fn drive_free(path: &Path) -> u64 {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs4::available_space(dir).unwrap_or(0)
}

/// (database extent, bytes in use) of the store, buffered file data counted as in use.
fn extent(i: &Inner) -> Result<(u64, u64)> {
    let q = |p: &str| i.conn.query_row(p, [], |r| r.get::<_, i64>(0)).map_err(dberr);
    let page = i.header.layout.unwrap_or(Layout::LEGACY).page_size as u64;
    let (pages, free) = (q("PRAGMA page_count")?.max(0) as u64, q("PRAGMA freelist_count")?.max(0) as u64);
    Ok((pages * page, pages.saturating_sub(free) * page + i.buffered as u64))
}

/// (file bytes a store of `extent` bytes reaches, bytes before the store of a compacted copy).
fn layout_of(i: &Inner, extent: u64) -> (u64, u64) {
    match i.header.journal {
        Some(j) => (single::Map::new(j, i.header.fixed).db_end(extent).max(j.moved_to), STORE_AT + j.region),
        None => (vfs::FIXED_DB_OFFSET as u64 + extent, vfs::FIXED_DB_OFFSET as u64),
    }
}

/// (min, min in place, max) capacities for a volume of `cap` whose store reaches `end` in the
/// file; `off`: what comes before the store in a compacted copy.
fn limits(cap: u64, off: u64, end: u64, used: u64, free: u64) -> (u64, u64, u64) {
    let min = up4k(off + used + margin(used)).max(Fixed::MIN).min(cap);
    let in_place = up4k(end).max(min).min(cap);
    let max = (cap.saturating_add(free) / 4096 * 4096).min(Fixed::MAX).max(cap);
    (min, in_place, max)
}

/// Gives the open database (fixed mode) capacity `to`: the shim writes its footer record (vfs.rs),
/// or its journal record (single.rs).
fn set_capacity(conn: &Connection, to: u64) -> Result<()> {
    let mut arg = to;
    // SAFETY: the handle is valid for the connection's life; the shim reads one u64 from `arg`.
    let rc = unsafe {
        rusqlite::ffi::sqlite3_file_control(conn.handle(), c"main".as_ptr(), vfs::FCNTL_SET_CAPACITY, &mut arg as *mut u64 as *mut _)
    };
    match rc {
        rusqlite::ffi::SQLITE_OK => Ok(()),
        rc => Err(Error::Io(format!("fixed-size volume: could not record the new size (sqlite {rc})"))),
    }
}

/// Writes the header with capacity `to` (both copies, each synced): the commit point.
fn commit_header(i: &mut Inner, to: u64) -> Result<()> {
    let a = i.header.appearance.as_ref().map(|a| a.appearance.clone());
    let body = i.header.body.clone();
    i.header = write_headers(&i.hfile, &i.master, &body, a.as_ref(), i.header.layout, Some(Fixed { capacity: to }), i.header.journal, &mut i.copy_seq)?;
    Ok(())
}

impl Vault {
    /// Size facts for "Estendi / Riduci" (also for growable volumes: `capacity: None`).
    pub fn resize_info(&self) -> Result<ResizeInfo> {
        let free = drive_free(&self.path);
        self.with(|i| {
            let file_bytes = i.hfile.metadata().map_err(ioerr)?.len();
            let (ext, used) = extent(i)?;
            let Some(Fixed { capacity }) = i.header.fixed else {
                return Ok(ResizeInfo {
                    capacity: None,
                    file_bytes,
                    used_bytes: used,
                    min_capacity: file_bytes,
                    min_in_place: file_bytes,
                    max_capacity: file_bytes,
                    drive_free: free,
                });
            };
            let (end, off) = layout_of(i, ext);
            let (min_capacity, min_in_place, max_capacity) = limits(capacity, off, end, used, free);
            Ok(ResizeInfo { capacity: Some(capacity), file_bytes, used_bytes: used, min_capacity, min_in_place, max_capacity, drive_free: free })
        })
    }

    /// Gives a fixed-size volume the capacity `target` (rounded down to 4 KiB) and returns it.
    /// Larger: the file is extended (needs the difference free on the drive). Smaller: down to
    /// [`ResizeInfo::min_capacity`]; below [`ResizeInfo::min_in_place`] the store is compacted
    /// into a copy of the new size (needs that much free on the drive). Crash-safe at every
    /// step: an interrupted resize opens at the old or the new size, never loses data. File
    /// operations wait while it runs (the store lock is held).
    pub fn resize(&self, target: u64, progress: &(dyn Fn(ResizeProgress) + Sync)) -> std::result::Result<u64, ResizeError> {
        let _s = self.slot_lock.lock().unwrap_or_else(|e| e.into_inner()); // header writers take turns
        let to = target - target % 4096;
        let free = drive_free(&self.path);
        let mut g = self.lock()?;
        let i = g.as_mut().unwrap();
        let Some(Fixed { capacity: cap }) = i.header.fixed else { return Err(ResizeError::NotFixed) };
        if !(Fixed { capacity: to }).is_valid() {
            return Err(ResizeError::OutOfRange { min: Fixed::MIN, max: Fixed::MAX });
        }
        if to == cap {
            return Ok(cap);
        }
        i.durable()?;
        let (ext, used) = extent(i)?;
        let (end, off) = layout_of(i, ext);
        let (min, in_place, _) = limits(cap, off, end, used, free);
        if to > cap {
            if to - cap > free {
                return Err(ResizeError::NoSpace { need: to - cap, free });
            }
            progress(ResizeProgress { phase: ResizePhase::Extend, done: 0, total: to - cap });
            self.grow(&mut g, cap, to)?;
        } else if to < min {
            return Err(ResizeError::TooSmall { min });
        } else if to >= in_place {
            self.shrink_in_place(&mut g, to, progress)?;
        } else {
            if to > free {
                return Err(ResizeError::NoSpace { need: to, free });
            }
            self.compact(&mut g, to, used, progress)?;
        }
        Ok(to)
    }

    fn grow(&self, g: &mut Option<Inner>, cap: u64, to: u64) -> Result<()> {
        let i = g.as_mut().unwrap();
        if let Err(e) = vfs::allocate(&i.hfile, to).and_then(|_| i.hfile.sync_all()) {
            let _ = i.hfile.set_len(cap);
            return Err(ioerr(e));
        }
        failpoint("resize-allocated");
        if let Err(e) = set_capacity(&i.conn, to) {
            let _ = i.hfile.set_len(cap);
            return Err(e);
        }
        failpoint("resize-footer");
        self.commit(g, to)
    }

    fn shrink_in_place(&self, g: &mut Option<Inner>, to: u64, progress: &(dyn Fn(ResizeProgress) + Sync)) -> Result<()> {
        let i = g.as_mut().unwrap();
        progress(ResizeProgress { phase: ResizePhase::Commit, done: 0, total: 1 });
        set_capacity(&i.conn, to)?;
        failpoint("resize-footer");
        self.commit(g, to)?;
        progress(ResizeProgress { phase: ResizePhase::Truncate, done: 0, total: 1 });
        let i = g.as_mut().unwrap();
        // committed: if this fails the next open cuts the file (resize::settle)
        let _ = i.hfile.set_len(to).and_then(|_| i.hfile.sync_all());
        failpoint("resize-truncated");
        Ok(())
    }

    /// Header with the new capacity, then the database learns its seq. After the footer step,
    /// a failure leaves both footer records valid; the volume is reopened so the header on disk
    /// (old or new capacity) decides, as after a crash.
    fn commit(&self, g: &mut Option<Inner>, to: u64) -> Result<()> {
        let i = g.as_mut().unwrap();
        let r = commit_header(i, to);
        failpoint("resize-header");
        let r = r.and_then(|_| i.record_header_seq());
        if r.is_err() {
            self.reopen(g);
        }
        r
    }

    /// Closes the store and opens it again (same keys and session) after a resize step failed.
    /// `g` is left empty (the volume reads as locked) if that fails.
    fn reopen(&self, g: &mut Option<Inner>) {
        let Some(old) = g.take() else { return };
        let m = old.master.clone();
        let (unlocked, tampered) = (old.unlocked, old.appearance_tampered);
        let Inner { conn, hfile, .. } = old;
        let _ = conn.close();
        drop(hfile);
        if let Ok(v) = Self::open_with(&self.path, |_, _, _| Ok((m.clone(), unlocked.unwrap_or(0)))) {
            let mut inner = v.shared.inner.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(n) = inner.as_mut() {
                n.unlocked = unlocked;
                n.appearance_tampered = tampered;
            }
            *g = inner;
            drop(v);
        }
    }

    fn compact(&self, g: &mut Option<Inner>, to: u64, used: u64, progress: &(dyn Fn(ResizeProgress) + Sync)) -> Result<()> {
        let tmp = side(&self.path);
        let tmp_journal = side_path(&tmp, "-journal");
        let i = g.as_mut().unwrap();
        let built = (|| -> Result<()> {
            let _ = std::fs::remove_file(&tmp);
            let f = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&tmp).map_err(ioerr)?;
            let a = i.header.appearance.as_ref().map(|a| a.appearance.clone());
            let mut seqs = i.copy_seq;
            let fixed = Some(Fixed { capacity: to });
            // a single-file volume's copy is single-file too, made in one piece (no moved bytes)
            let single = i.header.journal.map(|j| JournalRegion::fresh_fixed(j.region));
            let h = write_headers(&f, &i.master, &i.header.body, a.as_ref(), i.header.layout, fixed, single, &mut seqs)?;
            init_store(&f, fixed, single).map_err(ioerr)?;
            drop(f);
            // Same VFS (an attached database uses the connection's), same key and cipher settings.
            let name = tmp.to_str().ok_or(Error::InvalidName)?.replace('\'', "''");
            let key = i.master.derive(b"nv sqlcipher");
            let page = i.header.layout.unwrap_or(Layout::LEGACY).page_size;
            let attach = Zeroizing::new(format!("ATTACH DATABASE '{name}' AS resize KEY \"x'{}'\";", pq::hex(key.bytes())));
            match single {
                Some(j) => single::with_map(single::Map::new(j, fixed), || i.conn.execute_batch(&attach)),
                None => i.conn.execute_batch(&attach),
            }
            .map_err(dberr)?;
            let conn = &i.conn;
            let done = AtomicBool::new(false);
            let r = std::thread::scope(|sc| {
                // progress: database bytes written since the start (process-wide counter, an estimate)
                let start = vfs::io_counters().write_bytes[vfs::FileKind::Db as usize];
                let done = &done;
                sc.spawn(move || {
                    while !done.load(Ordering::Relaxed) {
                        let w = vfs::io_counters().write_bytes[vfs::FileKind::Db as usize] - start;
                        progress(ResizeProgress { phase: ResizePhase::Compact, done: w.min(used), total: used });
                        std::thread::sleep(Duration::from_millis(100));
                    }
                });
                let r = conn
                    .execute_batch(&format!("{} PRAGMA resize.cipher_page_size = {page};", CIPHER_PRAGMAS.replace("PRAGMA ", "PRAGMA resize.")))
                    .and_then(|_| conn.query_row("SELECT sqlcipher_export('resize')", [], |_| Ok(())))
                    .and_then(|_| {
                        conn.execute(
                            "INSERT OR REPLACE INTO resize.meta(k, v) VALUES (?, ?)",
                            params![HEADER_SEQ, h.body.seq.saturating_sub(1).to_string().as_bytes()],
                        )
                    })
                    // the copy's appearance anchor names its own header (as a key rotation's does)
                    .and_then(|_| crate::fresh::record_appearance(conn, "resize", &h))
                    .map_err(dberr);
                done.store(true, Ordering::Relaxed);
                r
            });
            let d = i.conn.execute_batch("DETACH DATABASE resize").map_err(dberr);
            r.and(d)?;
            // the copy is synced by its commits; make its directory entry durable too
            sync_dir(&tmp).map_err(ioerr)
        })();
        if let Err(e) = built {
            for t in [&tmp, &tmp_journal] {
                let _ = std::fs::remove_file(t);
            }
            return Err(e);
        }
        failpoint("resize-exported");
        progress(ResizeProgress { phase: ResizePhase::Commit, done: 0, total: 1 });
        // Swap: close this one (no journal left behind), rename the copy over it, reopen.
        let _ = i.conn.query_row("PRAGMA journal_mode = DELETE", [], |_| Ok(()));
        let old = g.take().unwrap();
        let master = old.master.clone();
        let Inner { conn, hfile, unlocked, appearance_tampered, .. } = old;
        let _ = conn.close();
        drop(hfile);
        let renamed = std::fs::rename(&tmp, &self.path).and_then(|_| sync_dir(&self.path));
        failpoint("resize-renamed");
        let _ = std::fs::remove_file(&tmp_journal);
        let v = Self::open_with(&self.path, |_, _, _| Ok((master.clone(), unlocked.unwrap_or(0))))
            .map_err(|e| Error::Io(format!("reopen after resize: {e}")))?;
        *g = v.shared.inner.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(v);
        if let Some(n) = g.as_mut() {
            n.unlocked = unlocked;
            n.appearance_tampered = appearance_tampered;
        }
        if renamed.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        renamed.map_err(ioerr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KdfPreset, OpenError};
    use nv_fs::{Fs, Kind, NodeId, ROOT};
    use secrecy::SecretString;

    const MIB: u64 = 1 << 20;

    fn pw() -> SecretString {
        SecretString::from("pw".to_owned())
    }
    fn pat(seed: u8, len: usize) -> Vec<u8> {
        (0..len).map(|i| (i as u32).wrapping_mul(2_654_435_761).rotate_left(seed as u32) as u8 ^ seed).collect()
    }
    fn read_file(v: &Vault, id: NodeId) -> Vec<u8> {
        let mut out = vec![0u8; v.getattr(id).unwrap().size as usize];
        let mut off = 0;
        while off < out.len() {
            off += v.read(id, off as u64, &mut out[off..]).unwrap();
        }
        out
    }
    fn put(v: &Vault, name: &str, data: &[u8]) -> NodeId {
        let f = v.create(ROOT, name, Kind::File).unwrap();
        v.write(f.id, 0, data).unwrap();
        v.flush().unwrap();
        f.id
    }
    fn len(p: &Path) -> u64 {
        std::fs::metadata(p).unwrap().len()
    }
    fn no_progress(_: ResizeProgress) {}

    /// Grow, use the room, shrink in place, reopen: same data, the file always its capacity.
    #[test]
    fn grow_then_shrink_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.nvault");
        let v = Vault::create_fixed(&path, &pw(), KdfPreset::Fast, 24 * MIB).unwrap();
        let a = pat(1, (6 * MIB) as usize);
        let fa = put(&v, "a", &a);
        let info = v.resize_info().unwrap();
        assert_eq!(info.capacity, Some(24 * MIB));
        assert!(info.max_capacity > 24 * MIB && info.min_capacity <= info.min_in_place && info.min_in_place <= 24 * MIB, "{info:?}");
        // 20 more MiB do not fit 24 MiB...
        let b = pat(2, (20 * MIB) as usize);
        assert!(v.create(ROOT, "b", Kind::File).and_then(|f| v.write(f.id, 0, &b).and_then(|_| v.flush())).is_err());
        let _ = v.lookup(ROOT, "b").and_then(|n| v.remove(n.id));
        v.flush().unwrap();
        // ...but they fit 48 MiB
        assert_eq!(v.resize(48 * MIB + 77, &no_progress).unwrap(), 48 * MIB);
        assert_eq!((len(&path), v.capacity(), v.stats().unwrap().total_bytes), (48 * MIB, Some(48 * MIB), 48 * MIB));
        let fb = put(&v, "b2", &b);
        v.close().unwrap();
        let v = Vault::open(&path, &pw()).unwrap();
        assert_eq!((read_file(&v, fa), read_file(&v, fb)), (a.clone(), b));
        v.remove(fb).unwrap();
        v.flush().unwrap();
        // the freed pages are in the tail: the extent shrinks only by compaction
        let info = v.resize_info().unwrap();
        assert!(info.min_in_place >= 26 * MIB, "{info:?}");
        assert_eq!(v.resize(info.min_in_place, &no_progress).unwrap(), info.min_in_place);
        assert_eq!(len(&path), info.min_in_place);
        assert_eq!(v.resize(info.min_in_place, &no_progress).unwrap(), info.min_in_place, "same size: nothing to do");
        let c = pat(3, MIB as usize);
        let fc = put(&v, "c", &c);
        v.close().unwrap();
        let v = Vault::open(&path, &pw()).unwrap();
        assert_eq!((read_file(&v, fa), read_file(&v, fc)), (a, c));
        assert_eq!((len(&path), v.capacity()), (info.min_in_place, Some(info.min_in_place)));
        v.check_integrity().unwrap();
        v.close().unwrap();
    }

    /// Below the store's extent: the data is packed into a copy of the new size; never below
    /// the data in use + margin.
    #[test]
    fn shrink_compacts_or_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.nvault");
        let v = Vault::create_fixed(&path, &pw(), KdfPreset::Fast, 64 * MIB).unwrap();
        let big = put(&v, "big", &pat(4, (30 * MIB) as usize));
        let keep = pat(5, (2 * MIB) as usize);
        let fk = put(&v, "keep", &keep);
        v.remove(big).unwrap();
        v.flush().unwrap();
        let info = v.resize_info().unwrap();
        assert!(info.min_capacity < 16 * MIB && info.min_in_place > 32 * MIB, "{info:?}");
        match v.resize(info.min_capacity - 4096, &no_progress) {
            Err(ResizeError::TooSmall { min }) => assert_eq!(min, info.min_capacity),
            r => panic!("{r:?}"),
        }
        assert!(matches!(v.resize(1 << 20, &no_progress), Err(ResizeError::OutOfRange { .. })));
        assert_eq!(len(&path), 64 * MIB, "refused: unchanged");
        let phases = std::sync::Mutex::new(Vec::new());
        assert_eq!(v.resize(16 * MIB, &|p| phases.lock().unwrap().push(p.phase)).unwrap(), 16 * MIB);
        assert!(phases.lock().unwrap().contains(&ResizePhase::Commit));
        assert_eq!((len(&path), v.capacity()), (16 * MIB, Some(16 * MIB)));
        assert!(!side(&path).exists());
        assert_eq!(read_file(&v, fk), keep);
        assert!(v.slots().iter().any(|s| s.in_use), "same session");
        let more = pat(6, (3 * MIB) as usize);
        let fm = put(&v, "more", &more);
        v.close().unwrap();
        let v = Vault::open(&path, &pw()).unwrap();
        assert_eq!((read_file(&v, fk), read_file(&v, fm)), (keep, more));
        v.check_integrity().unwrap();
        v.close().unwrap();
    }

    #[test]
    fn growable_volume_has_nothing_to_resize() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("g.nvault");
        let v = Vault::create(&path, &pw(), KdfPreset::Fast).unwrap();
        let info = v.resize_info().unwrap();
        assert_eq!(info.capacity, None);
        assert_eq!(info.file_bytes, len(&path));
        assert!(matches!(v.resize(64 * MIB, &no_progress), Err(ResizeError::NotFixed)));
        v.close().unwrap();
    }

    #[test]
    fn limits_rules() {
        let off = vfs::FIXED_DB_OFFSET as u64;
        // empty: the format minimum (or the margin)
        let (min, inp, max) = limits(64 * MIB, off, off, 0, 0);
        assert_eq!((min, inp, max), (up4k(off + RESIZE_MARGIN_MIN), up4k(off + RESIZE_MARGIN_MIN), 64 * MIB));
        // full: cannot shrink; the drive bounds growth
        let (min, inp, max) = limits(64 * MIB, off, 64 * MIB, 64 * MIB - off, 10 * MIB + 5);
        assert_eq!((min, inp, max), (64 * MIB, 64 * MIB, 74 * MIB));
        // big: margin 1/32
        let used = 3200 * MIB;
        assert_eq!(limits(8000 * MIB, off, off + used, used, 0).0, up4k(off + used + 100 * MIB));
    }

    /// Child of `resize_survives_a_crash_at_every_step`: opens the volume, resizes it with
    /// `NV_RESIZE_TO` and aborts at step `NV_RESIZE_CRASH`.
    #[test]
    fn resize_crash_child() {
        let Ok(path) = std::env::var("NV_RESIZE_CHILD") else { return };
        let v = Vault::open(Path::new(&path), &pw()).unwrap();
        *CRASH_AT.lock().unwrap() = std::env::var("NV_RESIZE_CRASH").unwrap();
        let to: u64 = std::env::var("NV_RESIZE_TO").unwrap().parse().unwrap();
        let _ = v.resize(to, &no_progress);
        std::process::exit(3); // the step was not reached: the parent fails
    }

    /// Crash injection: a resize killed after any of its steps (grow, shrink in place, shrink
    /// with compaction) opens at the old or the new size with every byte of data, and can be
    /// resized and written again.
    #[test]
    fn resize_survives_a_crash_at_every_step() {
        // (operation, new size, [(step to crash after, opens at the new size)])
        type Steps = &'static [(&'static str, bool)];
        let cases: &[(&str, u64, Steps)] = &[
            ("grow", 40 * MIB, &[("resize-allocated", false), ("resize-footer", false), ("header-first-copy", true), ("resize-header", true)]),
            (
                "shrink",
                16 * MIB,
                &[("resize-footer", false), ("header-first-copy", true), ("resize-header", true), ("resize-truncated", true)],
            ),
            ("compact", 14 * MIB, &[("header-first-copy", false), ("resize-exported", false), ("resize-renamed", true)]),
        ];
        let dir = tempfile::tempdir().unwrap();
        // single-file fixed volumes (what this build makes) and side-file ones (made before)
        for single in [true, false] {
        for (op, to, steps) in cases {
            for (step, new) in *steps {
                let path = dir.path().join(format!("{op}-{step}-{single}.nvault"));
                let from = 24 * MIB;
                let keep = pat(7, (3 * MIB) as usize);
                {
                    let v = Vault::create_fixed_as(&path, &pw(), KdfPreset::Fast, from, single).unwrap();
                    if *op == "compact" {
                        let gap = put(&v, "gap", &pat(8, (12 * MIB) as usize));
                        put(&v, "keep", &keep);
                        v.remove(gap).unwrap();
                    } else {
                        put(&v, "keep", &keep);
                    }
                    v.close().unwrap();
                }
                let st = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["resize::tests::resize_crash_child", "--exact", "--nocapture", "--test-threads=1"])
                    .env("NV_RESIZE_CHILD", &path)
                    .env("NV_RESIZE_CRASH", step)
                    .env("NV_RESIZE_TO", to.to_string())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .unwrap();
                assert!(!st.success() && st.code() != Some(3), "{op}/{step}/{single}: the crash point was not reached ({st:?})");
                let want = if *new { *to } else { from };
                // what the crash left: a grown file before its header / a shrink before its cut
                let raw = match *op {
                    "grow" => *to,
                    "shrink" if *step == "resize-truncated" => *to,
                    "shrink" => from,
                    _ => want,
                };
                assert_eq!(len(&path), raw, "{op}/{step}: file length after the crash");
                let v = Vault::open(&path, &pw()).unwrap_or_else(|e| panic!("{op}/{step}: {e}"));
                assert_eq!((v.capacity(), len(&path)), (Some(want), want), "{op}/{step}");
                assert!(!side(&path).exists(), "{op}/{step}: leftover");
                let k = v.lookup(ROOT, "keep").unwrap().id;
                assert_eq!(read_file(&v, k), keep, "{op}/{step}");
                v.check_integrity().unwrap();
                // keeps working: written, resized again, reopened
                let more = pat(9, MIB as usize);
                let m = put(&v, "more", &more);
                assert_eq!(v.resize(32 * MIB, &no_progress).unwrap(), 32 * MIB, "{op}/{step}");
                v.close().unwrap();
                let v = Vault::open(&path, &pw()).unwrap();
                assert_eq!((read_file(&v, k), read_file(&v, m), len(&path)), (keep, more, 32 * MIB), "{op}/{step}");
                assert_eq!(v.is_single_file(), single, "{op}/{step}");
                v.close().unwrap();
                let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                assert!(!names.iter().any(|n| n.starts_with(&*path.file_name().unwrap().to_string_lossy()) && n.len() > path.file_name().unwrap().len()), "{op}/{step}/{single}: {names:?}");
            }
        }
        }
    }

    /// The record for the new capacity is written next to the current one: torn (garbage), the
    /// volume opens at the old size as if the step had not happened. A file cut below its
    /// capacity is still refused.
    #[test]
    fn torn_or_cut_files() {
        for single in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("t.nvault");
            let v = Vault::create_fixed_as(&path, &pw(), KdfPreset::Fast, 24 * MIB, single).unwrap();
            let keep = pat(10, MIB as usize);
            put(&v, "keep", &keep);
            v.close().unwrap();
            let f = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
            // the slot not holding the current record (footer / journal records alike: 4 KiB apart)
            let other = if single {
                1 - crate::single::read_record(&f).unwrap().seq % 2
            } else {
                1 - vfs::read_footer_for(&f, 24 * MIB).unwrap().0 as u64
            };
            // a grow that allocated, then tore its record
            vfs::allocate(&f, 40 * MIB).unwrap();
            crate::write_at(&f, vfs::FOOTER_AT as u64 + other * 4096, &[0x5A; 96]).unwrap();
            let v = Vault::open(&path, &pw()).unwrap();
            assert_eq!((v.capacity(), len(&path)), (Some(24 * MIB), 24 * MIB), "{single}");
            assert_eq!(read_file(&v, v.lookup(ROOT, "keep").unwrap().id), keep);
            v.close().unwrap();
            f.set_len(20 * MIB).unwrap();
            assert!(matches!(Vault::open(&path, &pw()), Err(OpenError::Corrupt(_))), "{single}");
        }
    }
}
