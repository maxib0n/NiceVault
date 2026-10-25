//! Offset VFS shim: the SQLCipher database lives at byte 128 KiB of the `.nvault` file,
//! after the two header copies. This wraps the platform's default VFS and shifts every
//! read / write / size / truncate of the MAIN database file by [`DB_OFFSET`]. Journal,
//! WAL and shm files are opened through the same VFS but are not shifted.
//!
//! Everything else (locking, sync, shm) is forwarded untouched. Memory-mapped I/O
//! (`xFetch`/`xUnfetch`, io_methods v3) is not offered, so SQLite never maps the file
//! at the wrong offset.
//!
//! **Fixed mode** (`nvfixed`, volumes whose header has a `fixed` section): the file was
//! preallocated to its capacity and never changes size. The database starts at
//! [`FIXED_DB_OFFSET`]; between the header copies and it, a footer region records how many
//! bytes of the database are in use (two 64-byte records, newest `seq` wins, each with its own
//! checksum). The shim reports that logical size to SQLite (`xFileSize`), grows it on writes,
//! "truncates" by lowering it (the file keeps its size), refuses writes past the capacity with
//! `SQLITE_FULL`, and writes the record before every sync of the database, so the size on disk
//! is always covered by the same fsync as the pages (a rollback after a crash lowers it again).
//! The footer sits at the start of the file on purpose: writing at the end of a preallocated
//! file makes Windows (NTFS / exFAT valid data length) zero-fill everything before it.

use libsqlite3_sys as ffi;
use std::ffi::{c_char, c_int, c_void};
use std::sync::OnceLock;

/// Where the database starts inside the volume file.
pub const DB_OFFSET: i64 = 128 * 1024;
/// Name the shim is registered under.
pub const VFS_NAME: &str = "nvoffset";
/// The same shim in fixed mode (preallocated volumes).
pub const VFS_FIXED_NAME: &str = "nvfixed";
/// Footer region of a fixed volume: right after the two header copies.
pub const FOOTER_AT: i64 = DB_OFFSET;
/// Where a fixed volume's database starts (header copies + a 64 KiB footer region).
pub const FIXED_DB_OFFSET: i64 = DB_OFFSET + 64 * 1024;
/// File control (`sqlite3_file_control`, arg `*mut u64`): give the open fixed-mode database a
/// new capacity. Writes a record for it into the slot not holding the current one (which stays
/// valid: until the header names the new capacity the volume opens at the old one), synced; `SQLITE_FULL` if the database in use does not fit. The caller allocates the file
/// first (grow) and commits the capacity in the header after (vault resize, resize.rs).
pub const FCNTL_SET_CAPACITY: c_int = 0x4E56_0001;

std::thread_local! {
    /// Capacity the next fixed-mode main database opened on this thread must have (0 = the
    /// file's length). See [`expect_capacity`].
    static EXPECT_CAP: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Runs `f` (which opens a fixed volume's database on this thread) with the header's capacity
/// as the one the shim takes: the footer record of that capacity is the current one and the file
/// may be longer (a resize interrupted after the file grew, or before it was cut). Without it
/// (an attached copy) the capacity is the file's length.
pub fn expect_capacity<T>(capacity: u64, f: impl FnOnce() -> T) -> T {
    EXPECT_CAP.with(|c| c.set(capacity));
    let r = f();
    EXPECT_CAP.with(|c| c.set(0));
    r
}

/// The two footer records, in different 4 KiB blocks.
const FOOTER_SLOT: i64 = 4096;
const FOOTER_MAGIC: [u8; 8] = *b"NVFIXED1";
const FOOTER_REC: usize = 64;

/// One footer record of a fixed volume: database bytes in use and the file's capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Footer {
    pub seq: u64,
    pub logical: u64,
    pub capacity: u64,
}

fn encode_footer(f: &Footer) -> [u8; FOOTER_REC] {
    let mut b = [0u8; FOOTER_REC];
    b[..8].copy_from_slice(&FOOTER_MAGIC);
    b[8..16].copy_from_slice(&f.seq.to_le_bytes());
    b[16..24].copy_from_slice(&f.logical.to_le_bytes());
    b[24..32].copy_from_slice(&f.capacity.to_le_bytes());
    let sum = blake3::hash(&b[..32]);
    b[32..].copy_from_slice(sum.as_bytes());
    b
}

fn decode_footer(b: &[u8]) -> Option<Footer> {
    if b.len() < FOOTER_REC || b[..8] != FOOTER_MAGIC || blake3::hash(&b[..32]) != blake3::Hash::from_bytes(b[32..64].try_into().ok()?) {
        return None;
    }
    let n = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
    let f = Footer { seq: n(8), logical: n(16), capacity: n(24) };
    (f.logical <= fixed_db_capacity(f.capacity)).then_some(f)
}

/// The newest valid record of the two (slot index, record).
fn newest(recs: [Option<Footer>; 2]) -> Option<(usize, Footer)> {
    match recs {
        [Some(a), Some(b)] => Some(if b.seq > a.seq { (1, b) } else { (0, a) }),
        [Some(a), None] => Some((0, a)),
        [None, Some(b)] => Some((1, b)),
        [None, None] => None,
    }
}

/// Room for the database in a fixed volume of `capacity` bytes.
pub fn fixed_db_capacity(capacity: u64) -> u64 {
    capacity.saturating_sub(FIXED_DB_OFFSET as u64)
}

/// Makes `file` (the header copies already written) a fixed volume of `capacity` bytes:
/// allocates it (blocks reserved where the file system can, else just the length), clears the
/// footer region and writes the first record (nothing in use), synced.
pub fn init_fixed(file: &std::fs::File, capacity: u64) -> std::io::Result<()> {
    allocate(file, capacity)?;
    crate::write_at(file, FOOTER_AT as u64, &vec![0u8; (FIXED_DB_OFFSET - FOOTER_AT) as usize])?;
    crate::write_at(file, FOOTER_AT as u64, &encode_footer(&Footer { seq: 1, logical: 0, capacity }))?;
    file.sync_all()
}

/// Gives `file` the length `len` with its blocks reserved where the file system can (fallocate /
/// F_PREALLOCATE / FileAllocationInfo), else just the length. Not synced.
pub fn allocate(file: &std::fs::File, len: u64) -> std::io::Result<()> {
    if fs4::fs_std::FileExt::allocate(file, len).is_err() || file.metadata()?.len() != len {
        // no allocation API on this file system (FAT/exFAT on older kernels): the length alone
        file.set_len(len)?;
    }
    Ok(())
}

fn read_records(file: &std::fs::File) -> [Option<Footer>; 2] {
    let mut recs = [None, None];
    for (i, r) in recs.iter_mut().enumerate() {
        let mut b = [0u8; FOOTER_REC];
        if crate::read_at(file, (FOOTER_AT + i as i64 * FOOTER_SLOT) as u64, &mut b).ok() == Some(FOOTER_REC) {
            *r = decode_footer(&b);
        }
    }
    recs
}

/// The newest valid footer record of `file` for `capacity` (slot, record), whatever the file's
/// length. During a resize the two records can name different capacities: the header decides
/// which one is current ([`FCNTL_SET_CAPACITY`]).
pub fn read_footer_for(file: &std::fs::File, capacity: u64) -> Option<(usize, Footer)> {
    newest(read_records(file).map(|r| r.filter(|f| f.capacity == capacity)))
}

/// Our `sqlite3_file`: SQLite allocates `szOsFile` bytes, we put the real VFS's file
/// struct right after this header (`real` points there).
#[repr(C)]
struct ShimFile {
    base: ffi::sqlite3_file, // must be first: SQLite only knows about this part
    offset: i64,
    real: *mut ffi::sqlite3_file,
    kind: usize, // index into IO (FileKind)
    /// Fixed mode (main database of a preallocated volume): sizes below are db coordinates.
    fixed: bool,
    /// Database bytes in use (what xFileSize reports).
    logical: i64,
    /// Largest database the file holds.
    cap: i64,
    /// Physical file size (= the volume's capacity).
    phys: u64,
    /// seq of the last footer record written / read.
    fseq: u64,
    /// Slot holding that record: the next one goes to the other slot, so the current record
    /// is never overwritten (a torn write leaves it valid).
    fslot: usize,
    /// `logical` changed since the last record was written.
    fdirty: bool,
}

/// Which file of the store an I/O went to (index into [`io_counters`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Db = 0,
    Wal = 1,
    Journal = 2,
    Other = 3,
}

/// Process-wide I/O counters of every file opened through the shim (relaxed atomics:
/// cheap enough to keep in release builds; used by benches and tests).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IoCounters {
    /// Bytes handed to xWrite, per [`FileKind`].
    pub write_bytes: [u64; 4],
    pub writes: [u64; 4],
    /// xSync calls (each is an fsync / fdatasync / F_FULLFSYNC), per [`FileKind`].
    pub syncs: [u64; 4],
}

impl IoCounters {
    pub fn total_write_bytes(&self) -> u64 {
        self.write_bytes.iter().sum()
    }
    pub fn total_syncs(&self) -> u64 {
        self.syncs.iter().sum()
    }
    /// `self - earlier`, field by field.
    pub fn since(&self, earlier: &IoCounters) -> IoCounters {
        let d = |a: [u64; 4], b: [u64; 4]| std::array::from_fn(|i| a[i] - b[i]);
        IoCounters {
            write_bytes: d(self.write_bytes, earlier.write_bytes),
            writes: d(self.writes, earlier.writes),
            syncs: d(self.syncs, earlier.syncs),
        }
    }
}

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
#[allow(clippy::declare_interior_mutable_const)]
const Z: AtomicU64 = AtomicU64::new(0);
pub(crate) static W_BYTES: [AtomicU64; 4] = [Z; 4];
pub(crate) static W_CALLS: [AtomicU64; 4] = [Z; 4];
pub(crate) static SYNCS: [AtomicU64; 4] = [Z; 4];

/// Snapshot of the process-wide shim I/O counters.
pub fn io_counters() -> IoCounters {
    let l = |a: &[AtomicU64; 4]| std::array::from_fn(|i| a[i].load(Relaxed));
    IoCounters { write_bytes: l(&W_BYTES), writes: l(&W_CALLS), syncs: l(&SYNCS) }
}

static VFS: OnceLock<Result<(), c_int>> = OnceLock::new();

/// Register the shim (idempotent). Returns the SQLite error code on failure.
pub fn register() -> Result<(), c_int> {
    *VFS.get_or_init(|| unsafe {
        crate::slowio::from_env();
        // SAFETY: sqlite3_vfs_find(NULL) initializes SQLite and returns the default VFS,
        // a static that outlives the process. We copy it and only replace what we need.
        let real = ffi::sqlite3_vfs_find(std::ptr::null());
        if real.is_null() {
            return Err(ffi::SQLITE_ERROR);
        }
        let mut v: ffi::sqlite3_vfs = *real;
        v.iVersion = (*real).iVersion.min(3);
        v.szOsFile = std::mem::size_of::<ShimFile>() as c_int + (*real).szOsFile;
        v.pNext = std::ptr::null_mut();
        v.zName = c"nvoffset".as_ptr();
        v.pAppData = real as *mut c_void;
        v.xOpen = Some(x_open);
        // Every other vfs method receives our vfs pointer; hand the real one to the
        // real implementation (Windows' VFS reads pAppData in some of them).
        v.xDelete = Some(x_delete);
        v.xAccess = Some(x_access);
        v.xFullPathname = Some(x_full_pathname);
        v.xDlOpen = Some(x_dl_open);
        v.xDlError = Some(x_dl_error);
        v.xDlSym = Some(x_dl_sym);
        v.xDlClose = Some(x_dl_close);
        v.xRandomness = Some(x_randomness);
        v.xSleep = Some(x_sleep);
        v.xCurrentTime = Some(x_current_time);
        v.xGetLastError = Some(x_get_last_error);
        if v.iVersion >= 2 {
            v.xCurrentTimeInt64 = Some(x_current_time_i64);
        }
        if v.iVersion >= 3 {
            v.xSetSystemCall = Some(x_set_system_call);
            v.xGetSystemCall = Some(x_get_system_call);
            v.xNextSystemCall = Some(x_next_system_call);
        }
        let mut fixed = v;
        fixed.zName = c"nvfixed".as_ptr();
        // leaked on purpose: SQLite keeps the pointers
        for v in [v, fixed] {
            match ffi::sqlite3_vfs_register(Box::into_raw(Box::new(v)), 0) {
                ffi::SQLITE_OK => {}
                rc => return Err(rc),
            }
        }
        Ok(())
    })
}

#[inline]
pub(crate) unsafe fn real_vfs(v: *mut ffi::sqlite3_vfs) -> *mut ffi::sqlite3_vfs {
    unsafe { (*v).pAppData as *mut ffi::sqlite3_vfs }
}
#[inline]
unsafe fn shim(f: *mut ffi::sqlite3_file) -> &'static mut ShimFile {
    unsafe { &mut *(f as *mut ShimFile) }
}
#[inline]
unsafe fn methods(f: *mut ffi::sqlite3_file) -> &'static ffi::sqlite3_io_methods {
    // Only called after a successful open, where the real file's pMethods is non-null.
    unsafe { &*(*shim(f).real).pMethods }
}

static IO_METHODS: ffi::sqlite3_io_methods = ffi::sqlite3_io_methods {
    iVersion: 2, // v2 = shm support, no xFetch/xUnfetch (mmap would bypass the offset)
    xClose: Some(io_close),
    xRead: Some(io_read),
    xWrite: Some(io_write),
    xTruncate: Some(io_truncate),
    xSync: Some(io_sync),
    xFileSize: Some(io_file_size),
    xLock: Some(io_lock),
    xUnlock: Some(io_unlock),
    xCheckReservedLock: Some(io_check_reserved_lock),
    xFileControl: Some(io_file_control),
    xSectorSize: Some(io_sector_size),
    xDeviceCharacteristics: Some(io_device_characteristics),
    xShmMap: Some(io_shm_map),
    xShmLock: Some(io_shm_lock),
    xShmBarrier: Some(io_shm_barrier),
    xShmUnmap: Some(io_shm_unmap),
    xFetch: None,
    xUnfetch: None,
};

unsafe extern "C" fn x_open(
    vfs: *mut ffi::sqlite3_vfs,
    name: ffi::sqlite3_filename,
    file: *mut ffi::sqlite3_file,
    flags: c_int,
    out_flags: *mut c_int,
) -> c_int {
    unsafe {
        let real = real_vfs(vfs);
        let s = file as *mut ShimFile;
        let real_file = (file as *mut u8).add(std::mem::size_of::<ShimFile>()) as *mut ffi::sqlite3_file;
        (*s).base.pMethods = std::ptr::null();
        (*s).real = real_file;
        let main = flags & ffi::SQLITE_OPEN_MAIN_DB != 0;
        let fixed = main && std::ffi::CStr::from_ptr((*vfs).zName) == c"nvfixed";
        (*s).fixed = fixed;
        (*s).fdirty = false;
        (*s).fslot = 0;
        (*s).offset = if fixed { FIXED_DB_OFFSET } else if main { DB_OFFSET } else { 0 };
        (*s).kind = if flags & ffi::SQLITE_OPEN_MAIN_DB != 0 {
            FileKind::Db
        } else if flags & ffi::SQLITE_OPEN_WAL != 0 {
            FileKind::Wal
        } else if flags & ffi::SQLITE_OPEN_MAIN_JOURNAL != 0 {
            FileKind::Journal
        } else {
            FileKind::Other
        } as usize;
        let rc = ((*real).xOpen.unwrap())(real, name, real_file, flags, out_flags);
        if rc == ffi::SQLITE_OK && !(*real_file).pMethods.is_null() {
            let want = EXPECT_CAP.with(|c| c.replace(0));
            if fixed && !load_footer(&mut *s, want) {
                // not a fixed volume of the size its footer says: never let SQLite grow it
                ((*(*real_file).pMethods).xClose.unwrap())(real_file);
                return ffi::SQLITE_CORRUPT;
            }
            (*s).base.pMethods = &IO_METHODS;
        }
        rc
    }
}

/// Reads the footer of a fixed volume through the real file (right after it was opened).
/// `want`: the capacity the header names (0 = the file's length); the file must be at least
/// that long.
unsafe fn load_footer(s: &mut ShimFile, want: u64) -> bool {
    unsafe {
        let m = &*(*s.real).pMethods;
        let mut phys = 0i64;
        if (m.xFileSize.unwrap())(s.real, &mut phys) != ffi::SQLITE_OK {
            return false;
        }
        if want != 0 {
            if (phys as u64) < want {
                return false;
            }
            phys = want as i64;
        }
        let mut recs = [None, None];
        for (i, r) in recs.iter_mut().enumerate() {
            let mut b = [0u8; FOOTER_REC];
            if (m.xRead.unwrap())(s.real, b.as_mut_ptr() as *mut c_void, FOOTER_REC as c_int, FOOTER_AT + i as i64 * FOOTER_SLOT) == ffi::SQLITE_OK {
                *r = decode_footer(&b).filter(|f| f.capacity == phys as u64);
            }
        }
        let Some((slot, f)) = newest(recs) else { return false };
        s.fslot = slot;
        s.phys = f.capacity;
        s.cap = fixed_db_capacity(f.capacity) as i64;
        s.logical = f.logical as i64;
        s.fseq = f.seq;
        true
    }
}

/// Writes the next footer record (into the slot not holding the current one), not synced: the
/// caller's sync covers it.
unsafe fn write_footer(s: &mut ShimFile) -> c_int {
    unsafe {
        let f = Footer { seq: s.fseq + 1, logical: s.logical as u64, capacity: s.phys };
        let b = encode_footer(&f);
        let slot = 1 - s.fslot;
        let rc = ((*(*s.real).pMethods).xWrite.unwrap())(s.real, b.as_ptr() as *const c_void, FOOTER_REC as c_int, FOOTER_AT + slot as i64 * FOOTER_SLOT);
        if rc == ffi::SQLITE_OK {
            s.fseq = f.seq;
            s.fslot = slot;
            s.fdirty = false;
        }
        rc
    }
}

unsafe extern "C" fn io_close(f: *mut ffi::sqlite3_file) -> c_int {
    unsafe {
        let s = shim(f);
        if s.fixed && s.fdirty {
            let _ = write_footer(s);
        }
        (methods(f).xClose.unwrap())(s.real)
    }
}
unsafe extern "C" fn io_read(f: *mut ffi::sqlite3_file, buf: *mut c_void, n: c_int, off: i64) -> c_int {
    unsafe {
        let s = shim(f);
        if s.fixed && off + n as i64 > s.logical {
            // past the database in use: what a short file would give (zeros, SHORT_READ)
            let have = (s.logical - off).clamp(0, n as i64) as c_int;
            std::ptr::write_bytes((buf as *mut u8).add(have as usize), 0, (n - have) as usize);
            if have > 0 {
                let rc = (methods(f).xRead.unwrap())(s.real, buf, have, off + s.offset);
                if rc != ffi::SQLITE_OK && rc != ffi::SQLITE_IOERR_SHORT_READ {
                    return rc;
                }
            }
            return ffi::SQLITE_IOERR_SHORT_READ;
        }
        (methods(f).xRead.unwrap())(s.real, buf, n, off + s.offset)
    }
}
unsafe extern "C" fn io_write(f: *mut ffi::sqlite3_file, buf: *const c_void, n: c_int, off: i64) -> c_int {
    unsafe {
        let s = shim(f);
        if s.fixed && off + n as i64 > s.cap {
            return ffi::SQLITE_FULL;
        }
        let k = s.kind;
        W_BYTES[k].fetch_add(n.max(0) as u64, Relaxed);
        W_CALLS[k].fetch_add(1, Relaxed);
        crate::slowio::write(n.max(0) as usize);
        let rc = (methods(f).xWrite.unwrap())(s.real, buf, n, off + s.offset);
        if rc == ffi::SQLITE_OK && s.fixed && off + n as i64 > s.logical {
            s.logical = off + n as i64;
            s.fdirty = true;
        }
        rc
    }
}
unsafe extern "C" fn io_truncate(f: *mut ffi::sqlite3_file, size: i64) -> c_int {
    unsafe {
        let s = shim(f);
        if s.fixed {
            // the file keeps its size: only the database in use shrinks (or grows, zero-filled
            // by the reads above until written)
            if size > s.cap {
                return ffi::SQLITE_FULL;
            }
            if size != s.logical {
                s.logical = size;
                s.fdirty = true;
            }
            return ffi::SQLITE_OK;
        }
        (methods(f).xTruncate.unwrap())(s.real, size + s.offset)
    }
}
unsafe extern "C" fn io_sync(f: *mut ffi::sqlite3_file, flags: c_int) -> c_int {
    unsafe {
        let s = shim(f);
        SYNCS[s.kind].fetch_add(1, Relaxed);
        crate::slowio::sync();
        if s.fixed && s.fdirty {
            let rc = write_footer(s);
            if rc != ffi::SQLITE_OK {
                return rc;
            }
        }
        (methods(f).xSync.unwrap())(s.real, flags)
    }
}
unsafe extern "C" fn io_file_size(f: *mut ffi::sqlite3_file, out: *mut i64) -> c_int {
    unsafe {
        if shim(f).fixed {
            *out = shim(f).logical;
            return ffi::SQLITE_OK;
        }
        let rc = (methods(f).xFileSize.unwrap())(shim(f).real, out);
        if rc == ffi::SQLITE_OK {
            *out = (*out - shim(f).offset).max(0);
        }
        rc
    }
}
unsafe extern "C" fn io_lock(f: *mut ffi::sqlite3_file, l: c_int) -> c_int {
    unsafe { (methods(f).xLock.unwrap())(shim(f).real, l) }
}
unsafe extern "C" fn io_unlock(f: *mut ffi::sqlite3_file, l: c_int) -> c_int {
    unsafe { (methods(f).xUnlock.unwrap())(shim(f).real, l) }
}
unsafe extern "C" fn io_check_reserved_lock(f: *mut ffi::sqlite3_file, out: *mut c_int) -> c_int {
    unsafe { (methods(f).xCheckReservedLock.unwrap())(shim(f).real, out) }
}
unsafe extern "C" fn io_file_control(f: *mut ffi::sqlite3_file, op: c_int, arg: *mut c_void) -> c_int {
    unsafe {
        if op == ffi::SQLITE_FCNTL_SIZE_HINT && shim(f).fixed {
            return ffi::SQLITE_OK; // already allocated
        }
        if op == FCNTL_SET_CAPACITY {
            return set_capacity(shim(f), arg as *const u64);
        }
        if op == ffi::SQLITE_FCNTL_SIZE_HINT && !arg.is_null() {
            // The hint is a db size; translate it to a file size.
            let mut sz = *(arg as *mut i64) + shim(f).offset;
            return (methods(f).xFileControl.unwrap())(shim(f).real, op, &mut sz as *mut i64 as *mut c_void);
        }
        (methods(f).xFileControl.unwrap())(shim(f).real, op, arg)
    }
}
unsafe fn set_capacity(s: &mut ShimFile, arg: *const u64) -> c_int {
    unsafe {
        if !s.fixed || arg.is_null() {
            return ffi::SQLITE_NOTFOUND;
        }
        let to = *arg;
        if !(crate::header::Fixed { capacity: to }).is_valid() {
            return ffi::SQLITE_MISUSE;
        }
        if s.logical as u64 > fixed_db_capacity(to) {
            return ffi::SQLITE_FULL;
        }
        let (phys, cap) = (s.phys, s.cap);
        s.phys = to;
        s.cap = fixed_db_capacity(to) as i64;
        let mut rc = write_footer(s);
        if rc == ffi::SQLITE_OK {
            rc = ((*(*s.real).pMethods).xSync.unwrap())(s.real, ffi::SQLITE_SYNC_FULL);
        }
        if rc != ffi::SQLITE_OK {
            // the next record (if any) names the old capacity again
            (s.phys, s.cap) = (phys, cap);
            s.fdirty = true;
        }
        rc
    }
}
unsafe extern "C" fn io_sector_size(f: *mut ffi::sqlite3_file) -> c_int {
    unsafe { (methods(f).xSectorSize.unwrap())(shim(f).real) }
}
unsafe extern "C" fn io_device_characteristics(f: *mut ffi::sqlite3_file) -> c_int {
    unsafe { (methods(f).xDeviceCharacteristics.unwrap())(shim(f).real) }
}
unsafe extern "C" fn io_shm_map(f: *mut ffi::sqlite3_file, pg: c_int, sz: c_int, ext: c_int, pp: *mut *mut c_void) -> c_int {
    unsafe {
        match methods(f).xShmMap {
            Some(m) if methods(f).iVersion >= 2 => m(shim(f).real, pg, sz, ext, pp),
            _ => ffi::SQLITE_IOERR_SHMMAP,
        }
    }
}
unsafe extern "C" fn io_shm_lock(f: *mut ffi::sqlite3_file, off: c_int, n: c_int, flags: c_int) -> c_int {
    unsafe {
        match methods(f).xShmLock {
            Some(m) if methods(f).iVersion >= 2 => m(shim(f).real, off, n, flags),
            _ => ffi::SQLITE_IOERR_SHMLOCK,
        }
    }
}
unsafe extern "C" fn io_shm_barrier(f: *mut ffi::sqlite3_file) {
    unsafe {
        if let Some(m) = methods(f).xShmBarrier.filter(|_| methods(f).iVersion >= 2) {
            m(shim(f).real)
        }
    }
}
unsafe extern "C" fn io_shm_unmap(f: *mut ffi::sqlite3_file, del: c_int) -> c_int {
    unsafe {
        match methods(f).xShmUnmap {
            Some(m) if methods(f).iVersion >= 2 => m(shim(f).real, del),
            _ => ffi::SQLITE_OK,
        }
    }
}

// --- vfs-level pass-throughs (swap our vfs pointer for the real one) ---

pub(crate) unsafe extern "C" fn x_delete(v: *mut ffi::sqlite3_vfs, z: *const c_char, sync_dir: c_int) -> c_int {
    unsafe { let r = real_vfs(v); ((*r).xDelete.unwrap())(r, z, sync_dir) }
}
pub(crate) unsafe extern "C" fn x_access(v: *mut ffi::sqlite3_vfs, z: *const c_char, fl: c_int, out: *mut c_int) -> c_int {
    unsafe { let r = real_vfs(v); ((*r).xAccess.unwrap())(r, z, fl, out) }
}
pub(crate) unsafe extern "C" fn x_full_pathname(v: *mut ffi::sqlite3_vfs, z: *const c_char, n: c_int, out: *mut c_char) -> c_int {
    unsafe { let r = real_vfs(v); ((*r).xFullPathname.unwrap())(r, z, n, out) }
}
pub(crate) unsafe extern "C" fn x_dl_open(v: *mut ffi::sqlite3_vfs, z: *const c_char) -> *mut c_void {
    unsafe { let r = real_vfs(v); ((*r).xDlOpen.unwrap())(r, z) }
}
pub(crate) unsafe extern "C" fn x_dl_error(v: *mut ffi::sqlite3_vfs, n: c_int, z: *mut c_char) {
    unsafe { let r = real_vfs(v); ((*r).xDlError.unwrap())(r, n, z) }
}
pub(crate) type DlSym = Option<unsafe extern "C" fn(*mut ffi::sqlite3_vfs, *mut c_void, *const c_char)>;
pub(crate) unsafe extern "C" fn x_dl_sym(v: *mut ffi::sqlite3_vfs, h: *mut c_void, z: *const c_char) -> DlSym {
    unsafe { let r = real_vfs(v); ((*r).xDlSym.unwrap())(r, h, z) }
}
pub(crate) unsafe extern "C" fn x_dl_close(v: *mut ffi::sqlite3_vfs, h: *mut c_void) {
    unsafe { let r = real_vfs(v); ((*r).xDlClose.unwrap())(r, h) }
}
pub(crate) unsafe extern "C" fn x_randomness(v: *mut ffi::sqlite3_vfs, n: c_int, out: *mut c_char) -> c_int {
    unsafe { let r = real_vfs(v); ((*r).xRandomness.unwrap())(r, n, out) }
}
pub(crate) unsafe extern "C" fn x_sleep(v: *mut ffi::sqlite3_vfs, us: c_int) -> c_int {
    unsafe { let r = real_vfs(v); ((*r).xSleep.unwrap())(r, us) }
}
pub(crate) unsafe extern "C" fn x_current_time(v: *mut ffi::sqlite3_vfs, out: *mut f64) -> c_int {
    unsafe { let r = real_vfs(v); ((*r).xCurrentTime.unwrap())(r, out) }
}
pub(crate) unsafe extern "C" fn x_get_last_error(v: *mut ffi::sqlite3_vfs, n: c_int, out: *mut c_char) -> c_int {
    unsafe { let r = real_vfs(v); ((*r).xGetLastError.unwrap())(r, n, out) }
}
pub(crate) unsafe extern "C" fn x_current_time_i64(v: *mut ffi::sqlite3_vfs, out: *mut i64) -> c_int {
    unsafe { let r = real_vfs(v); ((*r).xCurrentTimeInt64.unwrap())(r, out) }
}
pub(crate) unsafe extern "C" fn x_set_system_call(v: *mut ffi::sqlite3_vfs, z: *const c_char, p: ffi::sqlite3_syscall_ptr) -> c_int {
    unsafe { let r = real_vfs(v); ((*r).xSetSystemCall.unwrap())(r, z, p) }
}
pub(crate) unsafe extern "C" fn x_get_system_call(v: *mut ffi::sqlite3_vfs, z: *const c_char) -> ffi::sqlite3_syscall_ptr {
    unsafe { let r = real_vfs(v); ((*r).xGetSystemCall.unwrap())(r, z) }
}
pub(crate) unsafe extern "C" fn x_next_system_call(v: *mut ffi::sqlite3_vfs, z: *const c_char) -> *const c_char {
    unsafe { let r = real_vfs(v); ((*r).xNextSystemCall.unwrap())(r, z) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{Connection, OpenFlags};

    #[test]
    fn main_db_is_shifted_and_wal_is_not() {
        register().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.db");
        // Pre-fill the header area with a sentinel the shim must never touch.
        std::fs::write(&p, vec![0xAB; DB_OFFSET as usize]).unwrap();
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
        {
            let c = Connection::open_with_flags_and_vfs(&p, flags, VFS_NAME).unwrap();
            c.pragma_update(None, "journal_mode", "WAL").unwrap();
            c.execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES (zeroblob(100000));").unwrap();
            assert!(dir.path().join("t.db-wal").exists());
            let wal = std::fs::read(dir.path().join("t.db-wal")).unwrap();
            assert_eq!(&wal[..3], &[0x37, 0x7f, 0x06], "WAL must start with its magic (unshifted)");
        }
        let raw = std::fs::read(&p).unwrap();
        assert!(raw[..DB_OFFSET as usize].iter().all(|&b| b == 0xAB), "header area touched");
        assert_eq!(&raw[DB_OFFSET as usize..][..16], b"SQLite format 3\0");
        // Reopen and truncate path: VACUUM shrinks the file through xTruncate.
        let c = Connection::open_with_flags_and_vfs(&p, flags, VFS_NAME).unwrap();
        assert_eq!(c.query_row("SELECT length(x) FROM t", [], |r| r.get::<_, i64>(0)).unwrap(), 100000);
        c.execute_batch("DELETE FROM t; PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
        drop(c);
        let raw = std::fs::read(&p).unwrap();
        assert!(raw[..DB_OFFSET as usize].iter().all(|&b| b == 0xAB));
        assert!(raw.len() < DB_OFFSET as usize + 64 * 1024, "truncate not shifted: {}", raw.len());
    }
}
