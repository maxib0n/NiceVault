//! Single-file journal (format v2 "un solo file"): SQLite's rollback journal lives **inside** the
//! `.nvault` file, so a volume is always exactly one file — no `-journal` beside it, not even
//! after a crash.
//!
//! A separate VFS shim (`nvsingle`, same pass-throughs as vfs.rs) presents two files to SQLite:
//! - the **database**, mapped through the header's [`JournalRegion`] (its first `region` bytes
//!   live at `moved_to`, see [`Map`]); its size in use is tracked like a fixed volume's;
//! - the **journal**, a virtual file: its first `region - 64 KiB` bytes are the journal area
//!   right after the records, the rest (a big transaction: deleting or rewriting gigabytes) goes
//!   to a *tail* past the end of the database, which is moved further out if the database grows
//!   into it and given back (file truncated) once the journal is reset after the commit.
//!
//! The 64 KiB records area holds two 96-byte records (newest valid `seq` wins, own checksum):
//! database bytes in use, journal size, tail position. A record is written before every sync of
//! either file (they are one file: one fsync covers both), so whatever SQLite relies on after a
//! sync — a hot journal's pages, the database size — is described by a record of the same sync.
//! Journal reads past its size read as zeros (a short file), so a reset journal is never "hot".
//!
//! **Fixed-size volumes** (header `fixed` and `journal`): the same, inside the preallocated file.
//! The region takes the place of the 64 KiB footer region plus the first `region - 64 KiB` bytes
//! of the database ([`JournalRegion::base`]); records carry the capacity (the footer records'
//! job: the header's capacity is a resize's commit point, the shim takes only records of it);
//! nothing is ever written past the capacity (`SQLITE_FULL`: database, journal tail and its moves
//! alike) and the file is never cut (it keeps its length; resize.rs owns it).

use crate::header::{Fixed, JournalRegion, STORE_AT};
use crate::vfs::{self, FileKind, SYNCS, W_BYTES, W_CALLS};
use libsqlite3_sys as ffi;
use std::cell::Cell;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Mutex, OnceLock};

/// Name the shim is registered under.
pub const VFS_SINGLE_NAME: &str = "nvsingle";
/// Records area at the start of the region (two records, in different 4 KiB blocks).
const RECORDS_LEN: u64 = 64 << 10;
const REC_SLOT: u64 = 4096;
const REC_MAGIC: [u8; 8] = *b"NVJRNL01";
const REC_LEN: usize = 96;
/// Where the in-file journal area starts.
const JOURNAL_AT: u64 = STORE_AT + RECORDS_LEN;
/// Region of the volumes this build makes single-file: 64 KiB of records + ~4 MiB of journal,
/// which holds every ordinary transaction (copies append new pages, which are never journaled).
pub const DEFAULT_REGION: u64 = 4 << 20;
/// Room left between the database and a new (or moved) journal tail.
#[cfg(not(test))]
const GAP: u64 = 16 << 20;
/// (small in tests, so small workloads move the tail)
#[cfg(test)]
const GAP: u64 = 192 << 10;

/// The database <-> file mapping of a single-file volume (header [`JournalRegion`]), and the
/// capacity of a fixed-size one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Map {
    pub j: JournalRegion,
    /// Capacity (file length) of a fixed-size volume; 0 = a growable one.
    pub cap: u64,
}

impl Map {
    pub fn new(j: JournalRegion, fixed: Option<Fixed>) -> Self {
        Map { j, cap: fixed.map_or(0, |f| f.capacity) }
    }
    fn fixed(&self) -> bool {
        self.cap != 0
    }
    /// Bytes of journal kept in the region (the rest goes to the tail).
    pub fn jbase(&self) -> u64 {
        self.j.region - RECORDS_LEN
    }
    /// File offset of database byte `l`, and how many bytes from there stay contiguous.
    fn db_run(&self, l: u64) -> (u64, u64) {
        let JournalRegion { moved_to, split, .. } = self.j;
        let d = self.j.displaced(self.fixed());
        if l < d {
            (moved_to + l, d - l)
        } else if l < split {
            (JournalRegion::base(self.fixed()) + l, split - l)
        } else {
            (moved_to + d + (l - split), u64::MAX)
        }
    }
    /// End (file offset) of a database of `logical` bytes.
    pub fn db_end(&self, logical: u64) -> u64 {
        let JournalRegion { moved_to, split, .. } = self.j;
        let d = self.j.displaced(self.fixed());
        if logical <= d {
            moved_to + logical
        } else if logical <= split {
            moved_to + d
        } else {
            moved_to + d + (logical - split)
        }
    }
    /// Largest database a fixed-size volume of `cap` bytes holds with this mapping.
    pub fn db_room(&self, cap: u64) -> u64 {
        let JournalRegion { moved_to, split, .. } = self.j;
        let d = self.j.displaced(self.fixed());
        if cap >= moved_to + d { split + (cap - moved_to - d) } else { cap.saturating_sub(moved_to) }
    }
    fn j_run(&self, j: u64, tail: u64) -> Option<(u64, u64)> {
        let b = self.jbase();
        if j < b {
            Some((JOURNAL_AT + j, b - j))
        } else if tail != 0 {
            Some((tail + (j - b), u64::MAX))
        } else {
            None
        }
    }
}

/// One record of the records area.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct Record {
    pub seq: u64,
    /// Database bytes in use.
    pub logical: u64,
    /// Journal size (bytes).
    pub jsize: u64,
    /// File offset of the journal's tail (0 = none).
    pub tail: u64,
    /// Capacity of a fixed-size volume (0 = growable; the bytes were zero before fixed-size
    /// volumes could be single-file, and are still in growable ones).
    pub cap: u64,
}

fn encode(r: &Record) -> [u8; REC_LEN] {
    let mut b = [0u8; REC_LEN];
    b[..8].copy_from_slice(&REC_MAGIC);
    for (i, v) in [r.seq, r.logical, r.jsize, r.tail, r.cap].into_iter().enumerate() {
        b[8 + i * 8..16 + i * 8].copy_from_slice(&v.to_le_bytes());
    }
    let sum = blake3::hash(&b[..64]);
    b[64..].copy_from_slice(sum.as_bytes());
    b
}

fn decode(b: &[u8]) -> Option<Record> {
    if b.len() < REC_LEN || b[..8] != REC_MAGIC || blake3::hash(&b[..64]) != blake3::Hash::from_bytes(b[64..96].try_into().ok()?) {
        return None;
    }
    let n = |i: usize| u64::from_le_bytes(b[8 + i * 8..16 + i * 8].try_into().unwrap());
    Some(Record { seq: n(0), logical: n(1), jsize: n(2), tail: n(3), cap: n(4) })
}

fn newest(recs: [Option<Record>; 2]) -> Option<Record> {
    match recs {
        [Some(a), Some(b)] => Some(if b.seq > a.seq { b } else { a }),
        [a, b] => a.or(b),
    }
}

/// Prepares the region of `file` (header copies written or about to be): clears the records and
/// the journal area (`whole`: all of it — after a migration it still holds the moved database
/// bytes; else its first 64 KiB, a new file is empty there), writes the first record (`logical`
/// bytes in use; `cap`: a fixed-size volume's capacity, 0 = growable), synced.
pub(crate) fn init_region(file: &std::fs::File, region: u64, logical: u64, whole: bool, cap: u64) -> std::io::Result<()> {
    let clear = if whole { region } else { RECORDS_LEN + (64 << 10) };
    let zeros = vec![0u8; 1 << 20];
    let mut done = 0;
    while done < clear {
        let k = (clear - done).min(zeros.len() as u64) as usize;
        crate::write_at(file, STORE_AT + done, &zeros[..k])?;
        done += k as u64;
    }
    #[cfg(test)]
    crash::point();
    crate::write_at(file, STORE_AT, &encode(&Record { seq: 1, logical, jsize: 0, tail: 0, cap }))?;
    #[cfg(test)]
    crash::point();
    file.sync_all()
}

/// Newest valid record of `file` (tests and diagnostics).
#[cfg(test)]
pub(crate) fn read_record(file: &std::fs::File) -> Option<Record> {
    let mut recs = [None, None];
    for (i, r) in recs.iter_mut().enumerate() {
        let mut b = [0u8; REC_LEN];
        if crate::read_at(file, STORE_AT + i as u64 * REC_SLOT, &mut b).ok()? == REC_LEN {
            *r = decode(&b);
        }
    }
    newest(recs)
}

/// The mapping [`move_out`] gives a closed side-file volume whose database holds `db` bytes
/// (`fixed`: a fixed-size volume, database at [`JournalRegion::base`]).
pub(crate) fn moved_map(page_size: u32, region: u64, db: u64, fixed: bool) -> (JournalRegion, u64) {
    let p = page_size as u64;
    let db = db.div_ceil(p) * p;
    let probe = JournalRegion { region, moved_to: 0, split: 0 };
    let split = db.max(probe.displaced(fixed));
    (JournalRegion { region, moved_to: JournalRegion::base(fixed) + split, split }, db)
}

/// Room an upgraded fixed-size volume keeps free past the region and the moved bytes.
const UPGRADE_ROOM: u64 = 1 << 20;

/// The journal region [`move_out`] gives a fixed-size volume of `cap` bytes whose database holds
/// `db` bytes: `want`, halved (down to [`JournalRegion::MIN`]) until the region's displaced bytes,
/// moved past the database, and [`UPGRADE_ROOM`] fit the capacity. None: not even the smallest.
pub(crate) fn region_to_upgrade(cap: u64, db: u64, page_size: u32, want: u64) -> Option<u64> {
    let mut region = want;
    loop {
        let (map, _) = moved_map(page_size, region, db, true);
        if map.moved_to + map.displaced(true) + UPGRADE_ROOM <= cap {
            return Some(region);
        }
        if region <= JournalRegion::MIN {
            return None;
        }
        region = (region / 2).max(JournalRegion::MIN);
    }
}

/// The journal region of a new fixed-size volume of `cap` bytes: [`DEFAULT_REGION`], halved
/// (down to [`JournalRegion::MIN`]) while it is more than an eighth of the volume.
pub(crate) fn region_for(cap: u64) -> u64 {
    let mut r = DEFAULT_REGION;
    while r > JournalRegion::MIN && r * 8 > cap {
        r /= 2;
    }
    r
}

/// Writes `r` into record slot `slot` of `file` (tests: damaged / foreign records).
#[cfg(test)]
pub(crate) fn write_record_at(file: &std::fs::File, slot: u64, r: &Record) -> std::io::Result<()> {
    crate::write_at(file, STORE_AT + slot * REC_SLOT, &encode(r))
}

/// Step 1 of making a closed side-file volume single-file **in place** (see `Vault::upgrade_format`):
/// copies the database's first displaced bytes past its end and syncs. Nothing the old format
/// reads is changed yet (the copy is past the end of its database, which every build ignores and
/// truncates away — a fixed-size one reads it as never written), so a crash here leaves the volume
/// as it was. `fixed`: a fixed-size volume (capacity, database bytes in use from its footer).
/// Returns the new mapping and the database size. The caller then writes the v2 header (the
/// switch) and [`init_region`].
pub(crate) fn move_out(file: &std::fs::File, page_size: u32, region: u64, fixed: Option<(u64, u64)>) -> std::io::Result<(JournalRegion, u64)> {
    let in_use = match fixed {
        Some((_, logical)) => logical,
        None => file.metadata()?.len().saturating_sub(STORE_AT),
    };
    let (map, db) = moved_map(page_size, region, in_use, fixed.is_some());
    let base = JournalRegion::base(fixed.is_some());
    let n = db.min(map.displaced(fixed.is_some()));
    if let Some((cap, _)) = fixed
        && map.moved_to + n > cap
    {
        return Err(std::io::Error::new(std::io::ErrorKind::StorageFull, "no room in the fixed-size volume for the journal region"));
    }
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0;
    while done < n {
        let k = (n - done).min(buf.len() as u64) as usize;
        let got = crate::read_at(file, base + done, &mut buf[..k])?;
        buf[got..k].fill(0);
        crate::write_at(file, map.moved_to + done, &buf[..k])?;
        #[cfg(test)]
        crash::point();
        done += k as u64;
    }
    file.sync_all()?;
    #[cfg(test)]
    crash::point();
    Ok((map, db))
}

thread_local! {
    /// Mapping for the next database this thread opens through the shim (see [`with_map`]).
    static PENDING: Cell<Option<Map>> = const { Cell::new(None) };
}

/// Runs `f` (which opens or attaches exactly one single-file database through [`VFS_SINGLE_NAME`]
/// on this thread) with `map` handed to the shim's xOpen.
pub(crate) fn with_map<T>(map: Map, f: impl FnOnce() -> T) -> T {
    PENDING.with(|p| p.set(Some(map)));
    let r = f();
    PENDING.with(|p| p.set(None));
    r
}

/// State of one open single-file database (shared by its journal).
struct State {
    map: Map,
    logical: u64,
    jsize: u64,
    tail: u64,
    seq: u64,
    /// The values above changed since the last record was written.
    dirty: bool,
}

/// Our `sqlite3_file`. Main database: `real` is the real VFS's file right after this struct.
/// Journal: `real` / `st` point into the main database's.
#[repr(C)]
struct SFile {
    base: ffi::sqlite3_file, // must be first
    real: *mut ffi::sqlite3_file,
    st: *mut State,
    journal: bool,
    /// Main database: registry key (its journal's name), owned.
    key: *mut c_char,
}

/// Open main databases by journal name (`<db path>-journal`, what SQLite asks for), as `*mut SFile`.
static MAINS: Mutex<Vec<(CString, usize)>> = Mutex::new(Vec::new());

fn main_for(name: *const c_char) -> Option<*mut SFile> {
    if name.is_null() {
        return None;
    }
    let n = unsafe { CStr::from_ptr(name) };
    MAINS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| k.as_c_str() == n).map(|&(_, p)| p as *mut SFile)
}

/// `<x>-wal` of an open single-file database (it never has one).
fn is_wal_of_main(name: *const c_char) -> bool {
    if name.is_null() {
        return false;
    }
    let n = unsafe { CStr::from_ptr(name) }.to_bytes();
    let Some(stem) = n.strip_suffix(b"-wal") else { return false };
    let mut j = stem.to_vec();
    j.extend_from_slice(b"-journal");
    MAINS.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|(k, _)| k.as_bytes() == j)
}

static VFS: OnceLock<Result<(), c_int>> = OnceLock::new();

/// Registers the shim (idempotent).
pub fn register() -> Result<(), c_int> {
    *VFS.get_or_init(|| unsafe {
        // SAFETY: as vfs::register — the default VFS is a static; we copy it and replace methods.
        let real = ffi::sqlite3_vfs_find(std::ptr::null());
        if real.is_null() {
            return Err(ffi::SQLITE_ERROR);
        }
        let mut v: ffi::sqlite3_vfs = *real;
        v.iVersion = (*real).iVersion.min(3);
        v.szOsFile = std::mem::size_of::<SFile>() as c_int + (*real).szOsFile;
        v.pNext = std::ptr::null_mut();
        v.zName = c"nvsingle".as_ptr();
        v.pAppData = real as *mut c_void;
        v.xOpen = Some(x_open);
        v.xDelete = Some(x_delete);
        v.xAccess = Some(x_access);
        v.xFullPathname = Some(vfs::x_full_pathname);
        v.xDlOpen = Some(vfs::x_dl_open);
        v.xDlError = Some(vfs::x_dl_error);
        v.xDlSym = Some(vfs::x_dl_sym);
        v.xDlClose = Some(vfs::x_dl_close);
        v.xRandomness = Some(vfs::x_randomness);
        v.xSleep = Some(vfs::x_sleep);
        v.xCurrentTime = Some(vfs::x_current_time);
        v.xGetLastError = Some(vfs::x_get_last_error);
        if v.iVersion >= 2 {
            v.xCurrentTimeInt64 = Some(vfs::x_current_time_i64);
        }
        if v.iVersion >= 3 {
            v.xSetSystemCall = Some(vfs::x_set_system_call);
            v.xGetSystemCall = Some(vfs::x_get_system_call);
            v.xNextSystemCall = Some(vfs::x_next_system_call);
        }
        match ffi::sqlite3_vfs_register(Box::into_raw(Box::new(v)), 0) {
            ffi::SQLITE_OK => Ok(()),
            rc => Err(rc),
        }
    })
}

static MAIN_IO: ffi::sqlite3_io_methods = ffi::sqlite3_io_methods {
    iVersion: 1, // no shm (never WAL), no mmap (would bypass the mapping)
    xClose: Some(db_close),
    xRead: Some(db_read),
    xWrite: Some(db_write),
    xTruncate: Some(db_truncate),
    xSync: Some(db_sync),
    xFileSize: Some(db_file_size),
    xLock: Some(db_lock),
    xUnlock: Some(db_unlock),
    xCheckReservedLock: Some(db_check_reserved_lock),
    xFileControl: Some(db_file_control),
    xSectorSize: Some(db_sector_size),
    xDeviceCharacteristics: Some(db_device_characteristics),
    xShmMap: None,
    xShmLock: None,
    xShmBarrier: None,
    xShmUnmap: None,
    xFetch: None,
    xUnfetch: None,
};

static JOURNAL_IO: ffi::sqlite3_io_methods = ffi::sqlite3_io_methods {
    iVersion: 1,
    xClose: Some(j_close),
    xRead: Some(j_read),
    xWrite: Some(j_write),
    xTruncate: Some(j_truncate),
    xSync: Some(j_sync),
    xFileSize: Some(j_file_size),
    xLock: Some(j_lock),
    xUnlock: Some(j_lock),
    xCheckReservedLock: Some(j_check_reserved_lock),
    xFileControl: Some(j_file_control),
    xSectorSize: Some(j_sector_size),
    xDeviceCharacteristics: Some(j_device_characteristics),
    xShmMap: None,
    xShmLock: None,
    xShmBarrier: None,
    xShmUnmap: None,
    xFetch: None,
    xUnfetch: None,
};

// ---------------- real-file helpers ----------------

unsafe fn rmethods(f: *mut ffi::sqlite3_file) -> &'static ffi::sqlite3_io_methods {
    unsafe { &*(*f).pMethods }
}
/// Reads `buf` at `off` of the real file; a read past its end is zero-filled (`SHORT_READ`).
/// (In pieces of at most 64 KiB: the real VFS takes at most 128 KiB - 1 per call, what SQLite
/// itself ever asks for.)
unsafe fn rread(f: *mut ffi::sqlite3_file, buf: &mut [u8], off: u64) -> c_int {
    let mut rc_all = ffi::SQLITE_OK;
    for (i, c) in buf.chunks_mut(IO_MAX).enumerate() {
        match unsafe { (rmethods(f).xRead.unwrap())(f, c.as_mut_ptr() as *mut c_void, c.len() as c_int, (off + (i * IO_MAX) as u64) as i64) } {
            ffi::SQLITE_OK => {}
            ffi::SQLITE_IOERR_SHORT_READ => rc_all = ffi::SQLITE_IOERR_SHORT_READ,
            rc => return rc,
        }
    }
    rc_all
}
unsafe fn rwrite(f: *mut ffi::sqlite3_file, buf: &[u8], off: u64) -> c_int {
    for (i, c) in buf.chunks(IO_MAX).enumerate() {
        let rc = unsafe { (rmethods(f).xWrite.unwrap())(f, c.as_ptr() as *const c_void, c.len() as c_int, (off + (i * IO_MAX) as u64) as i64) };
        if rc != ffi::SQLITE_OK {
            return rc;
        }
    }
    ffi::SQLITE_OK
}
const IO_MAX: usize = 64 << 10;
/// A write of ours (counted, crash hook).
unsafe fn write(f: *mut ffi::sqlite3_file, buf: &[u8], off: u64) -> c_int {
    #[cfg(test)]
    {
        crash::tick(f);
        crash::before_write(f, off, buf.len());
    }
    crate::slowio::write(buf.len());
    unsafe { rwrite(f, buf, off) }
}
unsafe fn sync(f: *mut ffi::sqlite3_file, flags: c_int) -> c_int {
    #[cfg(test)]
    crash::tick(f);
    crate::slowio::sync();
    let rc = unsafe { (rmethods(f).xSync.unwrap())(f, flags) };
    #[cfg(test)]
    if rc == ffi::SQLITE_OK {
        crash::synced();
    }
    rc
}
unsafe fn rsize(f: *mut ffi::sqlite3_file) -> Result<u64, c_int> {
    let mut n = 0i64;
    match unsafe { (rmethods(f).xFileSize.unwrap())(f, &mut n) } {
        ffi::SQLITE_OK => Ok(n.max(0) as u64),
        rc => Err(rc),
    }
}
unsafe fn rtruncate(f: *mut ffi::sqlite3_file, n: u64) -> c_int {
    unsafe { (rmethods(f).xTruncate.unwrap())(f, n as i64) }
}

/// Writes the next record (the older slot), not synced: the caller's sync covers it.
unsafe fn write_record(real: *mut ffi::sqlite3_file, st: &mut State) -> c_int {
    let r = Record { seq: st.seq + 1, logical: st.logical, jsize: st.jsize, tail: st.tail, cap: st.map.cap };
    W_BYTES[FileKind::Db as usize].fetch_add(REC_LEN as u64, Relaxed);
    let rc = unsafe { write(real, &encode(&r), STORE_AT + (r.seq % 2) * REC_SLOT) };
    if rc == ffi::SQLITE_OK {
        st.seq = r.seq;
        st.dirty = false;
    }
    rc
}

/// Before the database writes up to file offset `end`: if that reaches the journal's tail, the
/// tail moves further out first (copied, synced, then the record pointing to it, synced), so a hot
/// journal is never overwritten and the record never points to a copy that is not on disk.
unsafe fn make_room(real: *mut ffi::sqlite3_file, st: &mut State, end: u64) -> c_int {
    if st.tail == 0 || end <= st.tail {
        return ffi::SQLITE_OK;
    }
    let len = st.jsize.saturating_sub(st.map.jbase());
    let from = end.max(st.tail + len);
    let mut to = (from + GAP.max(len)).next_multiple_of(64 << 10);
    if st.map.fixed() && to + len > st.map.cap {
        // a fixed-size volume: closer, inside its free room, or else past its capacity
        to = (from + gap_in(st, from, len)).next_multiple_of(64 << 10);
        if to + len > st.map.cap {
            to = outside(st);
        }
    }
    unsafe { move_tail(real, st, to) }
}

/// Where a fixed-size volume's journal tail goes when its free room cannot hold it: right past
/// the capacity. The file is then longer than its capacity until the journal is reset (the file
/// is cut back, [`drop_tail`]; after a crash, once the open has rolled the journal back).
fn outside(st: &State) -> u64 {
    #[cfg(test)]
    crash::OUTSIDE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    st.map.cap.next_multiple_of(64 << 10)
}

/// Copies the journal's tail to `to` (past where it is now, or anywhere not overlapping it),
/// synced, then the record pointing to it, synced.
unsafe fn move_tail(real: *mut ffi::sqlite3_file, st: &mut State, to: u64) -> c_int {
    unsafe {
        let len = st.jsize.saturating_sub(st.map.jbase());
        let mut buf = vec![0u8; 1 << 20];
        let mut done = 0;
        while done < len {
            let k = (len - done).min(buf.len() as u64) as usize;
            let rc = rread(real, &mut buf[..k], st.tail + done);
            if rc != ffi::SQLITE_OK && rc != ffi::SQLITE_IOERR_SHORT_READ {
                return rc;
            }
            let rc = write(real, &buf[..k], to + done);
            if rc != ffi::SQLITE_OK {
                return rc;
            }
            W_BYTES[FileKind::Journal as usize].fetch_add(k as u64, Relaxed);
            done += k as u64;
        }
        let rc = sync(real, ffi::SQLITE_SYNC_NORMAL);
        if rc != ffi::SQLITE_OK {
            return rc;
        }
        st.tail = to;
        #[cfg(test)]
        crash::MOVES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let rc = write_record(real, st);
        if rc != ffi::SQLITE_OK {
            return rc;
        }
        sync(real, ffi::SQLITE_SYNC_NORMAL)
    }
}

/// Gives the journal's tail back once the journal is no longer needed past its area: the file
/// is cut to the end of the database (if the tail was the last thing in it).
unsafe fn drop_tail(real: *mut ffi::sqlite3_file, st: &mut State) -> c_int {
    if st.tail == 0 {
        return ffi::SQLITE_OK;
    }
    let was = st.tail;
    st.tail = 0;
    st.dirty = true;
    unsafe {
        // recorded now (not synced: the journal is reset, a stale tail in an older record is harmless)
        let rc = write_record(real, st);
        if rc != ffi::SQLITE_OK {
            return rc;
        }
        // a fixed-size volume keeps its length: cut back only a tail that went past its capacity
        let end = if st.map.fixed() { if was >= st.map.cap { st.map.cap } else { return ffi::SQLITE_OK } } else { st.map.db_end(st.logical) };
        match rsize(real) {
            Ok(n) if n > end => rtruncate(real, end),
            Ok(_) => ffi::SQLITE_OK,
            Err(rc) => rc,
        }
    }
}

/// Room to leave after `from` for a journal tail of (so far) `len` bytes in a fixed-size volume:
/// [`GAP`], or half of what is left under the capacity.
fn gap_in(st: &State, from: u64, len: u64) -> u64 {
    GAP.min(st.map.cap.saturating_sub(from + len) / 2) / (64 << 10) * (64 << 10)
}

#[inline]
unsafe fn sf(f: *mut ffi::sqlite3_file) -> &'static mut SFile {
    unsafe { &mut *(f as *mut SFile) }
}

// ---------------- VFS methods ----------------

unsafe extern "C" fn x_open(
    vfs: *mut ffi::sqlite3_vfs,
    name: ffi::sqlite3_filename,
    file: *mut ffi::sqlite3_file,
    flags: c_int,
    out_flags: *mut c_int,
) -> c_int {
    unsafe {
        let real_vfs = vfs::real_vfs(vfs);
        let s = file as *mut SFile;
        if flags & ffi::SQLITE_OPEN_MAIN_DB != 0 {
            (*s).base.pMethods = std::ptr::null();
            let Some(map) = PENDING.with(|p| p.take()) else { return ffi::SQLITE_CANTOPEN };
            if name.is_null() {
                return ffi::SQLITE_CANTOPEN;
            }
            let real = (file as *mut u8).add(std::mem::size_of::<SFile>()) as *mut ffi::sqlite3_file;
            (*s).real = real;
            (*s).journal = false;
            (*s).st = std::ptr::null_mut();
            (*s).key = std::ptr::null_mut();
            let rc = ((*real_vfs).xOpen.unwrap())(real_vfs, name, real, flags, out_flags);
            if rc != ffi::SQLITE_OK || (*real).pMethods.is_null() {
                return rc;
            }
            let mut recs = [None, None];
            for (i, r) in recs.iter_mut().enumerate() {
                let mut b = [0u8; REC_LEN];
                if rread(real, &mut b, STORE_AT + i as u64 * REC_SLOT) == ffi::SQLITE_OK {
                    *r = decode(&b);
                }
            }
            // No valid record: a migration that crashed right after switching the header (the
            // database is still exactly where the mapping says, `split` bytes or fewer in use; the
            // database header holds the true size; a fixed-size volume's old footer records are not
            // records here). Only records of the header's capacity count (a resize writes the new
            // one next to the current one before the header names it, see resize.rs); valid
            // records of none, or any other inconsistency, is a damaged volume.
            let any = recs.iter().any(Option::is_some);
            let rec = match newest(recs.map(|r| r.filter(|r| r.cap == map.cap))) {
                None if !any => Some(Record { seq: 0, logical: map.j.split, jsize: 0, tail: 0, cap: map.cap }),
                r => r,
            };
            // (a fixed-size volume's database lies inside its capacity; a journal tail may also be
            // past it, see `outside`)
            let ok = rec.is_some_and(|rec| {
                let consistent = (rec.tail == 0 && rec.jsize <= map.jbase()) || (rec.tail != 0 && rec.tail >= map.db_end(rec.logical));
                consistent && (!map.fixed() || map.db_end(rec.logical) <= map.cap)
            });
            let Some(rec) = rec.filter(|_| ok) else {
                ((*(*real).pMethods).xClose.unwrap())(real);
                return ffi::SQLITE_CORRUPT;
            };
            let st = Box::new(State { map, logical: rec.logical, jsize: rec.jsize, tail: rec.tail, seq: rec.seq, dirty: rec.seq == 0 });
            (*s).st = Box::into_raw(st);
            let mut key = CStr::from_ptr(name).to_bytes().to_vec();
            key.extend_from_slice(b"-journal");
            let key = CString::new(key).unwrap();
            MAINS.lock().unwrap_or_else(|e| e.into_inner()).push((key.clone(), s as usize));
            (*s).key = key.into_raw();
            (*s).base.pMethods = &MAIN_IO;
            return ffi::SQLITE_OK;
        }
        if flags & ffi::SQLITE_OPEN_MAIN_JOURNAL != 0 {
            (*s).base.pMethods = std::ptr::null();
            let Some(m) = main_for(name) else { return ffi::SQLITE_CANTOPEN };
            (*s).real = (*m).real;
            (*s).st = (*m).st;
            (*s).journal = true;
            (*s).key = std::ptr::null_mut();
            (*s).base.pMethods = &JOURNAL_IO;
            if !out_flags.is_null() {
                *out_flags = flags;
            }
            return ffi::SQLITE_OK;
        }
        // Temp files, statement journals (never next to the volume): the real VFS, untouched.
        ((*real_vfs).xOpen.unwrap())(real_vfs, name, file, flags, out_flags)
    }
}

unsafe extern "C" fn x_delete(v: *mut ffi::sqlite3_vfs, z: *const c_char, sync_dir: c_int) -> c_int {
    unsafe {
        let Some(m) = main_for(z) else { return vfs::x_delete(v, z, sync_dir) };
        // "Deleting" the journal (journal_mode=DELETE, or a stale one at open): it is reset in
        // place, durably — the header of a hot journal is cleared and the tail given back.
        let real = (*m).real;
        let st = &mut *(*m).st;
        let rc = write(real, &[0u8; 4096], JOURNAL_AT);
        if rc != ffi::SQLITE_OK {
            return rc;
        }
        st.jsize = 0;
        st.dirty = true;
        let rc = drop_tail(real, st);
        if rc != ffi::SQLITE_OK {
            return rc;
        }
        let rc = write_record(real, st);
        if rc != ffi::SQLITE_OK {
            return rc;
        }
        SYNCS[FileKind::Journal as usize].fetch_add(1, Relaxed);
        sync(real, ffi::SQLITE_SYNC_NORMAL)
    }
}

unsafe extern "C" fn x_access(v: *mut ffi::sqlite3_vfs, z: *const c_char, fl: c_int, out: *mut c_int) -> c_int {
    unsafe {
        if let Some(m) = main_for(z) {
            *out = ((*(*m).st).jsize > 0) as c_int;
            return ffi::SQLITE_OK;
        }
        if is_wal_of_main(z) {
            *out = 0;
            return ffi::SQLITE_OK;
        }
        vfs::x_access(v, z, fl, out)
    }
}

// ---------------- database file ----------------

unsafe extern "C" fn db_close(f: *mut ffi::sqlite3_file) -> c_int {
    unsafe {
        let s = sf(f);
        if !s.key.is_null() {
            let key = CString::from_raw(s.key);
            MAINS.lock().unwrap_or_else(|e| e.into_inner()).retain(|(k, p)| !(k == &key && *p == f as usize));
            s.key = std::ptr::null_mut();
        }
        if !s.st.is_null() {
            let mut st = Box::from_raw(s.st);
            s.st = std::ptr::null_mut();
            if st.dirty {
                let _ = write_record(s.real, &mut st);
            }
        }
        (rmethods(s.real).xClose.unwrap())(s.real)
    }
}

unsafe extern "C" fn db_read(f: *mut ffi::sqlite3_file, buf: *mut c_void, n: c_int, off: i64) -> c_int {
    unsafe {
        let s = sf(f);
        let st = &*s.st;
        let out = std::slice::from_raw_parts_mut(buf as *mut u8, n as usize);
        let off = off as u64;
        // past the database in use: what a short file gives (zeros, SHORT_READ)
        let have = st.logical.saturating_sub(off).min(n as u64) as usize;
        out[have..].fill(0);
        let mut rc_all = if have < out.len() { ffi::SQLITE_IOERR_SHORT_READ } else { ffi::SQLITE_OK };
        let mut done = 0usize;
        while done < have {
            let (p, run) = st.map.db_run(off + done as u64);
            let k = (run.min((have - done) as u64)) as usize;
            match rread(s.real, &mut out[done..done + k], p) {
                ffi::SQLITE_OK => {}
                ffi::SQLITE_IOERR_SHORT_READ => rc_all = ffi::SQLITE_IOERR_SHORT_READ,
                rc => return rc,
            }
            done += k;
        }
        rc_all
    }
}

unsafe extern "C" fn db_write(f: *mut ffi::sqlite3_file, buf: *const c_void, n: c_int, off: i64) -> c_int {
    unsafe {
        let s = sf(f);
        let st = &mut *s.st;
        let data = std::slice::from_raw_parts(buf as *const u8, n as usize);
        let off = off as u64;
        W_BYTES[FileKind::Db as usize].fetch_add(n as u64, Relaxed);
        W_CALLS[FileKind::Db as usize].fetch_add(1, Relaxed);
        if st.map.fixed() && st.map.db_end(off + data.len() as u64) > st.map.cap {
            return ffi::SQLITE_FULL; // the file never grows past its capacity
        }
        let mut done = 0usize;
        while done < data.len() {
            let (p, run) = st.map.db_run(off + done as u64);
            let k = (run.min((data.len() - done) as u64)) as usize;
            let rc = make_room(s.real, st, p + k as u64);
            if rc != ffi::SQLITE_OK {
                return rc;
            }
            let rc = write(s.real, &data[done..done + k], p);
            if rc != ffi::SQLITE_OK {
                return rc;
            }
            done += k;
        }
        let end = off + data.len() as u64;
        if end > st.logical {
            st.logical = end;
            st.dirty = true;
        }
        ffi::SQLITE_OK
    }
}

unsafe extern "C" fn db_truncate(f: *mut ffi::sqlite3_file, size: i64) -> c_int {
    unsafe {
        let s = sf(f);
        let st = &mut *s.st;
        let size = size.max(0) as u64;
        if size != st.logical {
            st.logical = size;
            st.dirty = true;
        }
        if st.tail != 0 || st.map.fixed() {
            return ffi::SQLITE_OK; // the tail is past the database / a fixed-size file keeps its length
        }
        let end = st.map.db_end(size);
        match rsize(s.real) {
            Ok(n) if n > end => rtruncate(s.real, end),
            Ok(_) => ffi::SQLITE_OK,
            Err(rc) => rc,
        }
    }
}

unsafe extern "C" fn db_sync(f: *mut ffi::sqlite3_file, flags: c_int) -> c_int {
    unsafe {
        let s = sf(f);
        SYNCS[FileKind::Db as usize].fetch_add(1, Relaxed);
        let st = &mut *s.st;
        if st.dirty {
            let rc = write_record(s.real, st);
            if rc != ffi::SQLITE_OK {
                return rc;
            }
        }
        sync(s.real, flags)
    }
}

unsafe extern "C" fn db_file_size(f: *mut ffi::sqlite3_file, out: *mut i64) -> c_int {
    unsafe {
        *out = (*sf(f).st).logical as i64;
        ffi::SQLITE_OK
    }
}
unsafe extern "C" fn db_lock(f: *mut ffi::sqlite3_file, l: c_int) -> c_int {
    unsafe { (rmethods(sf(f).real).xLock.unwrap())(sf(f).real, l) }
}
unsafe extern "C" fn db_unlock(f: *mut ffi::sqlite3_file, l: c_int) -> c_int {
    unsafe { (rmethods(sf(f).real).xUnlock.unwrap())(sf(f).real, l) }
}
unsafe extern "C" fn db_check_reserved_lock(f: *mut ffi::sqlite3_file, out: *mut c_int) -> c_int {
    unsafe { (rmethods(sf(f).real).xCheckReservedLock.unwrap())(sf(f).real, out) }
}
unsafe extern "C" fn db_file_control(f: *mut ffi::sqlite3_file, op: c_int, arg: *mut c_void) -> c_int {
    unsafe {
        if op == ffi::SQLITE_FCNTL_SIZE_HINT {
            return ffi::SQLITE_OK; // database sizes are not file sizes here
        }
        if op == vfs::FCNTL_SET_CAPACITY {
            return set_capacity(sf(f), arg as *const u64);
        }
        (rmethods(sf(f).real).xFileControl.unwrap())(sf(f).real, op, arg)
    }
}
/// [`vfs::FCNTL_SET_CAPACITY`] on a fixed-size single-file database (resize.rs): a record of the
/// new capacity, written into the slot not holding the current one and synced; until the header
/// names it the volume opens at the old one. `SQLITE_FULL` if what is in use does not fit.
unsafe fn set_capacity(s: &mut SFile, arg: *const u64) -> c_int {
    unsafe {
        let st = &mut *s.st;
        if !st.map.fixed() || arg.is_null() {
            return ffi::SQLITE_NOTFOUND;
        }
        let to = *arg;
        if !(Fixed { capacity: to }).is_valid() || st.map.j.moved_to >= to {
            return ffi::SQLITE_MISUSE;
        }
        let tail_end = if st.tail == 0 { 0 } else { st.tail + st.jsize.saturating_sub(st.map.jbase()) };
        if st.map.db_end(st.logical) > to || tail_end > to {
            return ffi::SQLITE_FULL;
        }
        let old = st.map.cap;
        st.map.cap = to;
        let mut rc = write_record(s.real, st);
        if rc == ffi::SQLITE_OK {
            rc = sync(s.real, ffi::SQLITE_SYNC_FULL);
        }
        if rc != ffi::SQLITE_OK {
            // the next record (if any) names the old capacity again
            st.map.cap = old;
            st.dirty = true;
        }
        rc
    }
}
unsafe extern "C" fn db_sector_size(f: *mut ffi::sqlite3_file) -> c_int {
    unsafe { (rmethods(sf(f).real).xSectorSize.unwrap())(sf(f).real) }
}
unsafe extern "C" fn db_device_characteristics(f: *mut ffi::sqlite3_file) -> c_int {
    unsafe { (rmethods(sf(f).real).xDeviceCharacteristics.unwrap())(sf(f).real) }
}

// ---------------- journal (virtual file inside the volume) ----------------

unsafe extern "C" fn j_close(_f: *mut ffi::sqlite3_file) -> c_int {
    ffi::SQLITE_OK
}

unsafe extern "C" fn j_read(f: *mut ffi::sqlite3_file, buf: *mut c_void, n: c_int, off: i64) -> c_int {
    unsafe {
        let s = sf(f);
        let st = &*s.st;
        let out = std::slice::from_raw_parts_mut(buf as *mut u8, n as usize);
        let off = off as u64;
        let have = st.jsize.saturating_sub(off).min(n as u64) as usize;
        out[have..].fill(0);
        let mut rc_all = if have < out.len() { ffi::SQLITE_IOERR_SHORT_READ } else { ffi::SQLITE_OK };
        let mut done = 0usize;
        while done < have {
            let Some((p, run)) = st.map.j_run(off + done as u64, st.tail) else {
                out[done..have].fill(0);
                return ffi::SQLITE_IOERR_SHORT_READ;
            };
            let k = (run.min((have - done) as u64)) as usize;
            match rread(s.real, &mut out[done..done + k], p) {
                ffi::SQLITE_OK => {}
                ffi::SQLITE_IOERR_SHORT_READ => rc_all = ffi::SQLITE_IOERR_SHORT_READ,
                rc => return rc,
            }
            done += k;
        }
        rc_all
    }
}

unsafe extern "C" fn j_write(f: *mut ffi::sqlite3_file, buf: *const c_void, n: c_int, off: i64) -> c_int {
    unsafe {
        let s = sf(f);
        let st = &mut *s.st;
        let data = std::slice::from_raw_parts(buf as *const u8, n as usize);
        let off = off as u64;
        W_BYTES[FileKind::Journal as usize].fetch_add(n as u64, Relaxed);
        W_CALLS[FileKind::Journal as usize].fetch_add(1, Relaxed);
        let mut done = 0usize;
        while done < data.len() {
            let j = off + done as u64;
            if j >= st.map.jbase() && st.tail == 0 {
                // a big transaction: the journal continues past the end of the database (in a
                // fixed-size volume: in its free room, the file's length is not the database's)
                let end = st.map.db_end(st.logical);
                st.tail = if !st.map.fixed() {
                    (end.max(rsize(s.real).unwrap_or(0)) + GAP).next_multiple_of(64 << 10)
                } else if end + 2 * IO_MAX as u64 <= st.map.cap {
                    (end + gap_in(st, end, data.len() as u64)).next_multiple_of(64 << 10)
                } else {
                    outside(st)
                };
                st.dirty = true;
            }
            let (mut p, mut run) = st.map.j_run(j, st.tail).unwrap();
            let mut k = (run.min((data.len() - done) as u64)) as usize;
            if st.map.fixed() && st.tail < st.map.cap && p + k as u64 > st.map.cap {
                // the tail outgrew the volume's free room: past the capacity with it
                let rc = move_tail(s.real, st, outside(st));
                if rc != ffi::SQLITE_OK {
                    return rc;
                }
                (p, run) = st.map.j_run(j, st.tail).unwrap();
                k = (run.min((data.len() - done) as u64)) as usize;
            }
            let rc = write(s.real, &data[done..done + k], p);
            if rc != ffi::SQLITE_OK {
                return rc;
            }
            done += k;
        }
        let end = off + data.len() as u64;
        if end > st.jsize {
            st.jsize = end;
            st.dirty = true;
        }
        ffi::SQLITE_OK
    }
}

unsafe extern "C" fn j_truncate(f: *mut ffi::sqlite3_file, size: i64) -> c_int {
    unsafe {
        let s = sf(f);
        let st = &mut *s.st;
        let size = size.max(0) as u64;
        if size != st.jsize {
            st.jsize = size;
            st.dirty = true;
        }
        // SQLite cuts the journal only once it is reset (journal_size_limit after a commit,
        // TRUNCATE mode, after a rollback): nothing past `size` is needed any more.
        if size <= st.map.jbase() { drop_tail(s.real, st) } else { ffi::SQLITE_OK }
    }
}

unsafe extern "C" fn j_sync(f: *mut ffi::sqlite3_file, flags: c_int) -> c_int {
    unsafe {
        let s = sf(f);
        SYNCS[FileKind::Journal as usize].fetch_add(1, Relaxed);
        let st = &mut *s.st;
        if st.dirty {
            let rc = write_record(s.real, st);
            if rc != ffi::SQLITE_OK {
                return rc;
            }
        }
        sync(s.real, flags)
    }
}

unsafe extern "C" fn j_file_size(f: *mut ffi::sqlite3_file, out: *mut i64) -> c_int {
    unsafe {
        *out = (*sf(f).st).jsize as i64;
        ffi::SQLITE_OK
    }
}
unsafe extern "C" fn j_lock(_f: *mut ffi::sqlite3_file, _l: c_int) -> c_int {
    ffi::SQLITE_OK
}
unsafe extern "C" fn j_check_reserved_lock(_f: *mut ffi::sqlite3_file, out: *mut c_int) -> c_int {
    unsafe { *out = 0 };
    ffi::SQLITE_OK
}
unsafe extern "C" fn j_file_control(_f: *mut ffi::sqlite3_file, _op: c_int, _arg: *mut c_void) -> c_int {
    ffi::SQLITE_NOTFOUND
}
unsafe extern "C" fn j_sector_size(f: *mut ffi::sqlite3_file) -> c_int {
    unsafe { (rmethods(sf(f).real).xSectorSize.unwrap())(sf(f).real) }
}
unsafe extern "C" fn j_device_characteristics(f: *mut ffi::sqlite3_file) -> c_int {
    // Same device as the database; but never "safe append" / "sequential": it is not a real file.
    unsafe { (rmethods(sf(f).real).xDeviceCharacteristics.unwrap())(sf(f).real) & ffi::SQLITE_IOCAP_POWERSAFE_OVERWRITE }
}

// ---------------- test hook: crash injection ----------------

/// Test builds: after [`crash::start`], `NV_CRASH_AT=<n>` aborts the process at the n-th write or
/// sync through this shim or a migration step (counted from `start`, journal and database alike); with `NV_CRASH_LOSE=<seed>` it first
/// undoes a pseudo-random half of the writes made since the last sync (a power cut: unsynced
/// writes may or may not have reached the disk, in any order).
#[cfg(test)]
pub(crate) mod crash {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicI64, Ordering::SeqCst};
    static LEFT: AtomicI64 = AtomicI64::new(-1);
    /// Writes and syncs counted so far (what a full run of a crash child reports).
    pub static TICKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    /// Tail moves so far.
    pub static MOVES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    /// Journal tails of fixed-size volumes placed past the capacity so far.
    pub static OUTSIDE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    /// (file offset, previous bytes) of the writes since the last sync, when NV_CRASH_LOSE is set.
    static UNSYNCED: Mutex<Vec<(u64, Vec<u8>)>> = Mutex::new(Vec::new());

    /// Starts counting (and arms the crash point from NV_CRASH_AT, if set).
    pub fn start() {
        TICKS.store(0, SeqCst);
        UNSYNCED.lock().unwrap_or_else(|e| e.into_inner()).clear();
        LEFT.store(std::env::var("NV_CRASH_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(-1), SeqCst);
    }
    fn armed() -> bool {
        LEFT.load(SeqCst) >= 0
    }
    fn lose_seed() -> Option<u64> {
        std::env::var("NV_CRASH_LOSE").ok().and_then(|v| v.parse().ok())
    }
    /// Counts one write/sync; aborts (after undoing unsynced writes) when the count runs out.
    pub fn tick(real: *mut libsqlite3_sys::sqlite3_file) {
        TICKS.fetch_add(1, SeqCst);
        if !armed() {
            return;
        }
        if LEFT.fetch_sub(1, SeqCst) == 0 {
            if let Some(mut seed) = lose_seed() {
                let mut v = UNSYNCED.lock().unwrap_or_else(|e| e.into_inner());
                // newest first, so overlapping writes unwind to the oldest bytes chosen
                for (off, old) in v.drain(..).rev() {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    if seed & 1 == 1 {
                        unsafe { super::rwrite(real, &old, off) };
                    }
                }
            }
            std::process::exit(134); // simulated crash: no core dump, so no DrKonqi/abrt popup on the desktop
        }
    }
    /// A crash point outside the shim (Rust-side writes of a migration / header rewrite): counted
    /// like a write; a crash there is a plain abort (those writes are synced before the next step).
    pub fn point() {
        TICKS.fetch_add(1, SeqCst);
        if armed() && LEFT.fetch_sub(1, SeqCst) == 0 {
            std::process::exit(134); // simulated crash: no core dump, so no DrKonqi/abrt popup on the desktop
        }
    }
    /// Before a write: remember what it overwrites (power-cut mode only).
    pub fn before_write(real: *mut libsqlite3_sys::sqlite3_file, off: u64, n: usize) {
        if armed() && lose_seed().is_some() {
            let mut old = vec![0u8; n];
            unsafe { super::rread(real, &mut old, off) };
            UNSYNCED.lock().unwrap_or_else(|e| e.into_inner()).push((off, old));
        }
    }
    pub fn synced() {
        if armed() {
            UNSYNCED.lock().unwrap_or_else(|e| e.into_inner()).clear();
        }
    }
}
