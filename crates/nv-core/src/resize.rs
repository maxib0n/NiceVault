use crate::header::{Fixed, JournalRegion, STORE_AT};
use crate::{CIPHER_PRAGMAS, Error, HEADER_SEQ, Inner, Layout, Result, Vault, dberr, init_store, ioerr, pq, side_path, single, sync_dir, vfs, write_headers};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use zeroize::Zeroizing;

pub const RESIZE_MARGIN_MIN: u64 = 8 << 20;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ResizeInfo {
    pub capacity: Option<u64>,
    pub file_bytes: u64,
    pub used_bytes: u64,
    pub min_capacity: u64,
    pub min_in_place: u64,
    pub max_capacity: u64,
    pub drive_free: u64,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResizePhase {
    Compact,
    Extend,
    Commit,
    Truncate,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct ResizeProgress {
    pub phase: ResizePhase,
    pub done: u64,
    pub total: u64,
}

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

pub(crate) fn side(path: &Path) -> PathBuf {
    side_path(path, "-resize")
}

#[cfg(test)]
pub(crate) static CRASH_AT: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
#[cfg(test)]
pub(crate) fn failpoint(name: &str) {
    if *CRASH_AT.lock().unwrap_or_else(|e| e.into_inner()) == name {
        std::process::exit(134);
    }
}
#[cfg(not(test))]
#[inline(always)]
pub(crate) fn failpoint(_: &str) {}

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

fn extent(i: &Inner) -> Result<(u64, u64)> {
    let q = |p: &str| i.conn.query_row(p, [], |r| r.get::<_, i64>(0)).map_err(dberr);
    let page = i.header.layout.unwrap_or(Layout::LEGACY).page_size as u64;
    let (pages, free) = (q("PRAGMA page_count")?.max(0) as u64, q("PRAGMA freelist_count")?.max(0) as u64);
    Ok((pages * page, pages.saturating_sub(free) * page + i.buffered as u64))
}

fn layout_of(i: &Inner, extent: u64) -> (u64, u64) {
    match i.header.journal {
        Some(j) => (single::Map::new(j, i.header.fixed).db_end(extent).max(j.moved_to), STORE_AT + j.region),
        None => (vfs::FIXED_DB_OFFSET as u64 + extent, vfs::FIXED_DB_OFFSET as u64),
    }
}

fn limits(cap: u64, off: u64, end: u64, used: u64, free: u64) -> (u64, u64, u64) {
    let min = up4k(off + used + margin(used)).max(Fixed::MIN).min(cap);
    let in_place = up4k(end).max(min).min(cap);
    let max = (cap.saturating_add(free) / 4096 * 4096).min(Fixed::MAX).max(cap);
    (min, in_place, max)
}

fn set_capacity(conn: &Connection, to: u64) -> Result<()> {
    let mut arg = to;
    let rc = unsafe {
        rusqlite::ffi::sqlite3_file_control(conn.handle(), c"main".as_ptr(), vfs::FCNTL_SET_CAPACITY, &mut arg as *mut u64 as *mut _)
    };
    match rc {
        rusqlite::ffi::SQLITE_OK => Ok(()),
        rc => Err(Error::Io(format!("fixed-size volume: could not record the new size (sqlite {rc})"))),
    }
}

fn commit_header(i: &mut Inner, to: u64) -> Result<()> {
    let a = i.header.appearance.as_ref().map(|a| a.appearance.clone());
    let body = i.header.body.clone();
    i.header = write_headers(&i.hfile, &i.master, &body, a.as_ref(), i.header.layout, Some(Fixed { capacity: to }), i.header.journal, &mut i.copy_seq)?;
    Ok(())
}

impl Vault {
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

    pub fn resize(&self, target: u64, progress: &(dyn Fn(ResizeProgress) + Sync)) -> std::result::Result<u64, ResizeError> {
        let _s = self.slot_lock.lock().unwrap_or_else(|e| e.into_inner());
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
        let _ = i.hfile.set_len(to).and_then(|_| i.hfile.sync_all());
        failpoint("resize-truncated");
        Ok(())
    }

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
            let single = i.header.journal.map(|j| JournalRegion::fresh_fixed(j.region));
            let h = write_headers(&f, &i.master, &i.header.body, a.as_ref(), i.header.layout, fixed, single, &mut seqs)?;
            init_store(&f, fixed, single).map_err(ioerr)?;
            drop(f);
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
                    .and_then(|_| crate::fresh::record_appearance(conn, "resize", &h))
                    .map_err(dberr);
                done.store(true, Ordering::Relaxed);
                r
            });
            let d = i.conn.execute_batch("DETACH DATABASE resize").map_err(dberr);
            r.and(d)?;
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
        let b = pat(2, (20 * MIB) as usize);
        assert!(v.create(ROOT, "b", Kind::File).and_then(|f| v.write(f.id, 0, &b).and_then(|_| v.flush())).is_err());
        let _ = v.lookup(ROOT, "b").and_then(|n| v.remove(n.id));
        v.flush().unwrap();
        assert_eq!(v.resize(48 * MIB + 77, &no_progress).unwrap(), 48 * MIB);
        assert_eq!((len(&path), v.capacity(), v.stats().unwrap().total_bytes), (48 * MIB, Some(48 * MIB), 48 * MIB));
        let fb = put(&v, "b2", &b);
        v.close().unwrap();
        let v = Vault::open(&path, &pw()).unwrap();
        assert_eq!((read_file(&v, fa), read_file(&v, fb)), (a.clone(), b));
        v.remove(fb).unwrap();
        v.flush().unwrap();
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
        let (min, inp, max) = limits(64 * MIB, off, off, 0, 0);
        assert_eq!((min, inp, max), (up4k(off + RESIZE_MARGIN_MIN), up4k(off + RESIZE_MARGIN_MIN), 64 * MIB));
        let (min, inp, max) = limits(64 * MIB, off, 64 * MIB, 64 * MIB - off, 10 * MIB + 5);
        assert_eq!((min, inp, max), (64 * MIB, 64 * MIB, 74 * MIB));
        let used = 3200 * MIB;
        assert_eq!(limits(8000 * MIB, off, off + used, used, 0).0, up4k(off + used + 100 * MIB));
    }

    #[test]
    fn resize_crash_child() {
        let Ok(path) = std::env::var("NV_RESIZE_CHILD") else { return };
        let v = Vault::open(Path::new(&path), &pw()).unwrap();
        *CRASH_AT.lock().unwrap() = std::env::var("NV_RESIZE_CRASH").unwrap();
        let to: u64 = std::env::var("NV_RESIZE_TO").unwrap().parse().unwrap();
        let _ = v.resize(to, &no_progress);
        std::process::exit(3);
    }

    #[test]
    fn resize_survives_a_crash_at_every_step() {
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
            let other = if single {
                1 - crate::single::read_record(&f).unwrap().seq % 2
            } else {
                1 - vfs::read_footer_for(&f, 24 * MIB).unwrap().0 as u64
            };
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
