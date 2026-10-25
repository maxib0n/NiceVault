//! NiceVault core: volume format, keyslots and the encrypted SQLCipher store.
//! The on-disk format is described in `header.rs`. `Vault` implements [`nv_fs::Fs`].

mod appearance;
mod duress;
mod fresh;
mod header;
mod keys;
pub mod passwords;
mod pq;
mod props;
mod resize;
mod search;
mod single;
#[doc(hidden)]
pub mod slowio;
#[cfg(test)]
mod single_tests;
mod vfs;

pub use header::{Argon as KdfParams, Fixed, JournalRegion, KdfPreset, Layout, MAX_LABEL};
pub use appearance::{
    Appearance, AppearanceError, LockedEmblem, MAX_ACCENT_ID, MAX_ICON_ID, MAX_INITIAL, MAX_LOCKED_IMAGE, MAX_LOCKED_SIDE, image_size,
};
pub use pq::{Identity, Recipient};
pub use resize::{RESIZE_MARGIN_MIN, ResizeError, ResizeInfo, ResizePhase, ResizeProgress};
pub use props::{FILES_SCHEMA_KEY, NodeMeta};
pub use search::{
    Filters, Hit, KindGroup, MAX_TAG_NAME, MAX_TAGS, SEARCH_SCHEMA_KEY, SearchPage, SearchQuery, Sort, TAG_COLORS, Tag, TagError, TagId, fold,
    kind_group,
};
pub use vfs::{FileKind, IoCounters, io_counters};

use header::{COPY_LEN, Header, HeaderBody, MAGIC, MAX_SLOTS, Slot, SlotEntry, VERSION};
use keys::{Key32, random};
use nv_fs::{Error, Fs, Kind, Node, NodeId, ROOT, Result, Stats};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

/// Layout of the volumes this build creates (existing volumes keep theirs, see [`Layout`]).
/// Measured with scripts/bench-io.sh: 64 KiB pages / 1 MiB chunks save <10% CPU on a fast disk
/// and nothing on a USB stick (the device is the limit), cost 4x read amplification for
/// 256 KiB reads, and would lock older builds out of new volumes — so new volumes keep the
/// v1 store. Builds from this one on open v2 (stored-layout) volumes too.
pub const DEFAULT_LAYOUT: Layout = Layout::LEGACY;
/// Chunk size of v1 volumes (= [`Layout::LEGACY`]); a v2 volume stores its own.
pub const CHUNK: usize = Layout::LEGACY.chunk_size as usize;
/// Move buffered file data into the open transaction once this much is buffered (bounds memory
/// and, more to the point, how long one spill holds the store lock: encrypting and handing
/// 4 MiB to the OS takes ~10-30 ms; 64 MiB took 0.3-0.5 s, during which a listing waited).
const MAX_BUFFERED: usize = 4 << 20;
/// Before a spill, a bulk writer waits (outside the store lock) while more than this much of what
/// it handed to the OS is not written back to the device yet (see [`Writeback`]): a sync SQLite
/// makes during the spill (the journal's, when the transaction journaled a page since the last
/// one) then waits for at most this much (~100 ms on a 20 MB/s stick), while the device still has
/// work queued while the next spill is encrypted.
const WRITEBACK_AT: u64 = 2 << 20;
/// Durability point once the oldest non-durable change is this old (part of the contract:
/// a crash loses at most about this much work)...
pub const DURABLE_INTERVAL: Duration = Duration::from_secs(3);
/// ...or once this much file data went into the open transaction.
const DURABLE_BYTES: u64 = 256 << 20;
/// [`Vault::discard`] cuts a file by this much per durability point (bounds how long one step
/// holds the store lock: reading that much of the file's pages).
pub const DISCARD_STEP: u64 = 32 << 20;

/// How SQLite keeps commits atomic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
enum Journal {
    /// Rollback journal kept between commits (PERSIST), synchronous=FULL: appended pages are
    /// written once, straight into the database; every COMMIT is durable.
    Persist,
    /// Write-ahead log, synchronous=NORMAL (what volumes were opened with before): every page
    /// is written twice (WAL, then checkpoint); durable after a checkpoint.
    Wal,
}
/// Whether [`Vault::create`] makes single-file volumes (format v2, journal inside the file).
/// Builds before this one cannot open them (`unsupported_version`). Existing
/// ). Builds before this one cannot open them (`unsupported_version`). Existing
/// (v1) volumes are unchanged and become single-file only with [`Vault::upgrade_format`]
/// ("Aggiorna formato"). Fixed-size volumes too ([`Vault::create_fixed`], the region inside the
/// preallocated file).
const NEW_VOLUMES_SINGLE_FILE: bool = true;

/// [`Vault::upgrade_format`] on a fixed-size volume without room for the journal region (the app
/// answers `volume_full`: extend it first).
pub const UPGRADE_NO_ROOM: &str = "the fixed-size volume is too full for the single-file format: extend it first";

/// The journal mode this build opens every volume with. `Wal` is what builds before group
/// commit used; tests still make volumes with it to stand in for those builds.
const JOURNAL: Journal = Journal::Persist;

#[derive(Debug, thiserror::Error, Serialize, PartialEq, Eq)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum OpenError {
    #[error("wrong password or key")]
    WrongPassword,
    #[error("not a NiceVault volume")]
    NotAVault,
    #[error("corrupt volume: {0}")]
    Corrupt(String),
    #[error("io: {0}")]
    Io(String),
    /// Written by a newer NiceVault in a form this build does not understand (header version,
    /// unknown slot / emblem kind, unknown header fields). Nothing was written to the file.
    #[error("volume made by a newer NiceVault: {0}")]
    UnsupportedVersion(String),
    /// The header is older than the database last saw (an old copy of the header was written
    /// back: it may revive a changed or removed password). Nothing was written to the file.
    #[error("the volume's header was rolled back to an older copy")]
    RolledBack,
}

/// Why a keyslot operation was refused. Kept apart from [`nv_fs::Error`] so callers can
/// match on the rule that was broken instead of parsing messages.
#[derive(Debug, thiserror::Error)]
pub enum SlotError {
    #[error("no such key slot")]
    NotFound,
    #[error("slot is not a password slot")]
    NotPassword,
    #[error("cannot remove the last key slot")]
    LastSlot,
    #[error("all {MAX_SLOTS} key slots are in use")]
    Full,
    #[error("slot label is longer than {MAX_LABEL} bytes")]
    LabelTooLong,
    /// The duress password would equal another password of the volume (or vice versa).
    #[error("the duress password must differ from every other password of the volume")]
    DuressConflict,
    #[error("wrong password")]
    WrongPassword,
    #[error(transparent)]
    Fs(#[from] Error),
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SlotInfo {
    pub index: usize,
    /// "password" | "recipient" | "fido2" | "duress" (a password that destroys the keys, see duress.rs)
    pub kind: &'static str,
    /// User label (stored sealed in the header).
    pub label: Option<String>,
    /// Argon2id parameters (password and duress slots).
    pub kdf: Option<KdfParams>,
    /// Hex of the recipient's 16-byte fingerprint (recipient slots only).
    pub fingerprint: Option<String>,
    /// This slot unlocked the current session.
    pub in_use: bool,
}

/// Empty / whitespace-only label = no label.
fn norm_label(l: Option<&str>) -> std::result::Result<Option<&str>, SlotError> {
    match l.map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) if l.len() > MAX_LABEL => Err(SlotError::LabelTooLong),
        l => Ok(l),
    }
}

/// An unlocked volume. All methods take `&self` and are thread-safe.
pub struct Vault {
    path: PathBuf,
    shared: Arc<Shared>,
    flusher: Mutex<Option<JoinHandle<()>>>,
    /// Serializes keyslot changes so the (slow) KDF can run without holding the store lock.
    slot_lock: Mutex<()>,
}

struct Shared {
    // global lock — one Mutex around the connection and all write buffers.
    // Every Fs op is serialized; the ceiling is one core's worth of SQLCipher + memcpy
    // (fine for a single-user drive; per-node locks + a read-only connection pool if it ever matters).
    inner: Mutex<Option<Inner>>, // None once closed
    stop: Mutex<bool>,
    wake: Condvar,
    /// One durability point at a time ([`durable_point`]); bulk writers wait for a running one,
    /// so what its COMMIT syncs under the lock is what was there when it started, written back.
    commit: Mutex<()>,
    committing: std::sync::atomic::AtomicBool,
}

impl Shared {
    fn lock(&self) -> Result<MutexGuard<'_, Option<Inner>>> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_none() {
            return Err(Error::Locked);
        }
        Ok(g)
    }

    /// A bulk writer: waits for a durability point in progress (without the store lock).
    fn yield_to_commit(&self) {
        if self.committing.load(std::sync::atomic::Ordering::Relaxed) {
            drop(self.commit.lock().unwrap_or_else(|e| e.into_inner()));
        }
    }

    /// Durability point (see [`Inner::durable`]) with the slow part outside the store lock: the
    /// data goes to the OS under the lock (CPU only), is written back to the device without it,
    /// and the COMMIT under the lock then syncs what little is left. `due`: only if the oldest
    /// change is [`DURABLE_INTERVAL`] old (the background thread).
    fn durable_point(&self, due: bool) -> Result<()> {
        let is_due = |i: &Inner| i.dirty_since.is_some_and(|t| !due || t.elapsed() >= DURABLE_INTERVAL);
        if due && !is_due(self.lock()?.as_ref().unwrap()) {
            return Ok(());
        }
        let _c = self.commit.lock().unwrap_or_else(|e| e.into_inner());
        // (cleared on every way out, a panic included: bulk writers must not wait for a
        // durability point that is not running)
        struct Committing<'a>(&'a std::sync::atomic::AtomicBool);
        impl Drop for Committing<'_> {
            fn drop(&mut self) {
                self.0.store(false, std::sync::atomic::Ordering::Relaxed);
            }
        }
        self.committing.store(true, std::sync::atomic::Ordering::Relaxed);
        let _flag = Committing(&self.committing);
        (|| {
            let wb = {
                let mut g = self.lock()?;
                let i = g.as_mut().unwrap();
                if due && !is_due(i) {
                    return Ok(()); // made durable meanwhile
                }
                i.wb.clone()
            };
            wb.wait_below(WRITEBACK_AT);
            self.lock()?.as_mut().unwrap().spill()?;
            // (a writer already past `yield_to_commit` may spill once more meanwhile: written
            // back too before the COMMIT, a few rounds at most)
            for round in 0.. {
                wb.wait();
                let mut g = self.lock()?;
                let i = g.as_mut().unwrap();
                if round < 3 && i.wb.pending() > WRITEBACK_AT {
                    continue;
                }
                let r = i.durable();
                probe(&i.conn)?;
                return r;
            }
            unreachable!()
        })()
    }
}

struct Inner {
    // Field order matters for Drop: the connection must close before `hfile`
    // (on Unix closing any fd of the file drops SQLite's POSIX locks).
    conn: Connection,
    bufs: HashMap<NodeId, FileBuf>,
    buffered: usize,
    /// When the oldest change that is not durable yet was made (None = everything durable).
    dirty_since: Option<Instant>,
    /// Group commit: changes accumulate in one open transaction, COMMITted at the next
    /// durability point (see [`Inner::durable`]).
    in_tx: bool,
    /// File data moved into the open transaction so far.
    tx_bytes: u64,
    chunk: usize,
    journal: Journal,
    master: Key32,
    header: Header,
    /// Index of the slot that unlocked this session (None once that slot is removed).
    unlocked: Option<usize>,
    /// seq of copy A / copy B as found on disk (0 = invalid copy).
    copy_seq: [u64; 2],
    /// The header's appearance section failed its MAC at unlock (it was dropped and the header repaired).
    appearance_tampered: bool,
    /// In-memory search index of this session (search.rs), refreshed from the tables.
    search: search::Cache,
    /// Writeback of this store's data outside the lock (shared with the [`Vault`] methods).
    wb: Arc<Writeback>,
    hfile: File,
}

/// Keeps the store lock free of slow device I/O (the import lag on USB sticks): every sync made under the lock (SQLite's journal syncs, the COMMIT) waits for
/// everything dirty in the page cache to reach the device: on a 15 MB/s stick, seconds while
/// listings, thumbnails and stats queue behind it. So the data the store hands to the OS is
/// written back *outside* the lock, by the thread that wrote it (bulk writers wait there once
/// [`WRITEBACK_AT`] is pending) and before each durability point, and the syncs under the lock find
/// little left to do.
struct Writeback {
    /// A second handle on the volume file (None: could not be duplicated; then only the syncs
    /// under the lock write data back, as before).
    file: Option<File>,
    st: Mutex<WbState>,
    cv: Condvar,
}

#[derive(Default)]
struct WbState {
    /// Bytes handed to the OS and not written back yet (approximate: spilled file data)...
    pending: u64,
    /// ...and those being written back right now.
    inflight: u64,
    /// [`slowio::written`] when last counted into `pending` ([`Writeback::account`]).
    mark: u64,
}

impl Writeback {
    /// For the volume file `f`, with its writeback thread (`nv-writeback`, ends once the store
    /// is gone): it keeps the device busy with what the store wrote, so that bulk writers seldom
    /// wait and the syncs under the lock find little to do.
    fn for_file(f: &File) -> Arc<Writeback> {
        let wb = Arc::new(Writeback {
            file: f.try_clone().ok(),
            st: Mutex::new(WbState { mark: crate::slowio::written(), ..WbState::default() }),
            cv: Condvar::new(),
        });
        if wb.file.is_some() {
            let weak = Arc::downgrade(&wb);
            let _ = std::thread::Builder::new().name("nv-writeback".into()).spawn(move || Writeback::run(&weak));
        }
        wb
    }

    fn run(weak: &std::sync::Weak<Writeback>) {
        loop {
            let Some(wb) = weak.upgrade() else { return };
            let mut g = wb.st.lock().unwrap_or_else(|e| e.into_inner());
            if g.pending == 0 {
                // (wakes now and then to notice the store went away)
                let _ = wb.cv.wait_timeout(g, Duration::from_millis(500)).unwrap_or_else(|e| e.into_inner());
                continue;
            }
            g.inflight = std::mem::take(&mut g.pending);
            drop(g);
            crate::slowio::writeback();
            if let Some(f) = &wb.file {
                write_back(f);
            }
            wb.st.lock().unwrap_or_else(|e| e.into_inner()).inflight = 0;
            wb.cv.notify_all();
        }
    }

    /// Counts what the store handed to the OS since the last call (every write of the VFS shims:
    /// file data, the journal, the pages a secure delete overwrites) as pending writeback. Called
    /// after every operation under the store lock, so that a durability point writes it back
    /// before its COMMIT, outside the lock — deleting a big file wrote GBs that were then synced
    /// under the lock (cancel-hang). (Writes of other volumes of the process may be counted too:
    /// one more writeback of this file, which finds little to do.)
    fn account(&self) {
        if self.file.is_none() {
            return;
        }
        let now = crate::slowio::written();
        let mut g = self.st.lock().unwrap_or_else(|e| e.into_inner());
        let n = now.saturating_sub(g.mark);
        g.mark = now;
        if n > 0 {
            g.pending += n;
            drop(g);
            self.cv.notify_all();
        }
    }

    /// Waits (without the store lock) while more than `limit` bytes are not written back yet.
    fn wait_below(&self, limit: u64) {
        let mut g = self.st.lock().unwrap_or_else(|e| e.into_inner());
        while g.pending + g.inflight > limit {
            g = self.cv.wait(g).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn pending(&self) -> u64 {
        let g = self.st.lock().unwrap_or_else(|e| e.into_inner());
        g.pending + g.inflight
    }

    /// The durability point's: what the store handed to the OS so far is written back.
    fn wait(&self) {
        self.wait_below(0);
    }

    /// After a COMMIT: its sync wrote everything back.
    fn clear(&self) {
        let mut g = self.st.lock().unwrap_or_else(|e| e.into_inner());
        g.pending = 0;
        g.mark = crate::slowio::written();
        drop(g);
        self.cv.notify_all();
    }
}

/// Starts writeback of the file's dirty pages and waits for it (Linux: sync_file_range, which
/// neither flushes the device cache nor touches metadata; elsewhere fdatasync — Android too:
/// the libc crate has no sync_file_range for Bionic).
fn write_back(f: &File) {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: a valid fd for the life of `f`; offset 0 / length 0 = the whole file.
        let rc = unsafe {
            libc::sync_file_range(
                f.as_raw_fd(),
                0,
                0,
                libc::SYNC_FILE_RANGE_WAIT_BEFORE | libc::SYNC_FILE_RANGE_WRITE | libc::SYNC_FILE_RANGE_WAIT_AFTER,
            )
        };
        if rc == 0 {
            return;
        }
    }
    let _ = f.sync_data();
}

/// Uncommitted state of one file.
struct FileBuf {
    size: u64,
    mtime: i64,
    /// Full images of dirty chunks (each at most `min(chunk, size - idx*chunk)` long).
    chunks: BTreeMap<u64, Vec<u8>>,
    /// Stored chunks with idx >= this are logically gone (pending truncate).
    trunc_from: Option<u64>,
}

fn now_ns() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as i64).unwrap_or(0)
}

fn dberr(e: rusqlite::Error) -> Error {
    use rusqlite::ErrorCode::*;
    match &e {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound,
        rusqlite::Error::SqliteFailure(f, _) => match f.code {
            ConstraintViolation => Error::Exists,
            // NOMEM: what SQLCipher's pager answers for good once a page failed its HMAC.
            DatabaseCorrupt | NotADatabase | OutOfMemory => Error::Corrupt(e.to_string()),
            _ => Error::Io(e.to_string()),
        },
        _ => Error::Io(e.to_string()),
    }
}

fn ioerr(e: std::io::Error) -> Error {
    Error::Io(e.to_string())
}

pub(crate) fn write_at(f: &File, off: u64, buf: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::FileExt::write_all_at(f, buf, off)
    }
    #[cfg(windows)]
    {
        let mut done = 0;
        while done < buf.len() {
            done += std::os::windows::fs::FileExt::seek_write(f, &buf[done..], off + done as u64)?;
        }
        Ok(())
    }
}

pub(crate) fn read_at(f: &File, off: u64, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut done = 0;
    while done < buf.len() {
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::read_at(f, &mut buf[done..], off + done as u64)?;
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_read(f, &mut buf[done..], off + done as u64)?;
        if n == 0 {
            break;
        }
        done += n;
    }
    Ok(done)
}

// Deviation from the usual layout: `chunks` is a rowid table, not WITHOUT ROWID. In a WITHOUT ROWID
// table the 256 KiB blob lives inside the primary-key b-tree, and SQLite loads the whole
// overflowing record for every key comparison during a lookup — ~10x more page
// decrypt+HMAC work. With a rowid table the (node, idx) index holds only the keys.
const SCHEMA: &str = "
CREATE TABLE nodes (
  id     INTEGER PRIMARY KEY,
  parent INTEGER NOT NULL REFERENCES nodes(id),
  name   TEXT NOT NULL,
  kind   INTEGER NOT NULL,
  size   INTEGER NOT NULL DEFAULT 0,
  mtime  INTEGER NOT NULL,
  UNIQUE(parent, name)
);
CREATE TABLE chunks (node INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
                     idx INTEGER NOT NULL, data BLOB NOT NULL,
                     PRIMARY KEY(node, idx));
CREATE TABLE meta (k TEXT PRIMARY KEY, v BLOB);
";

/// SQLCipher settings of every volume database, exactly what the first volumes were written
/// with (SQLCipher 4.x bundled by libsqlite3-sys 0.38.2: 4.x defaults except page size and HMAC). The
/// key is raw (`x'..'`), so `kdf_iter` is unused for the key itself; the HMAC key is still
/// derived with `cipher_kdf_algorithm`, which is why that one is pinned too. The page size here
/// is the v1 one ([`Layout::LEGACY`]); a v2 volume's stored page size is applied right after.
pub(crate) const CIPHER_PRAGMAS: &str = "
PRAGMA cipher_compatibility = 4;
PRAGMA cipher_page_size = 16384;
PRAGMA cipher_hmac_algorithm = HMAC_SHA256;
PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;
PRAGMA kdf_iter = 256000;
PRAGMA cipher_plaintext_header_size = 0;
";

/// `fixed`: a preallocated volume (header `fixed` section): opened through the shim's fixed
/// mode, which keeps the database inside the file's capacity (vfs.rs).
/// `single`: a single-file volume (header `journal` section, fixed-size or not): opened through
/// the `nvsingle` shim, which keeps the rollback journal inside the file (single.rs).
fn open_db(
    path: &Path,
    master: &Key32,
    create: bool,
    layout: Layout,
    fixed: bool,
    journal: Journal,
    single: Option<single::Map>,
) -> std::result::Result<Connection, OpenError> {
    vfs::register().map_err(|rc| OpenError::Io(format!("vfs register failed: {rc}")))?;
    single::register().map_err(|rc| OpenError::Io(format!("vfs register failed: {rc}")))?;
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let conn = match single {
        Some(m) => single::with_map(m, || Connection::open_with_flags_and_vfs(path, flags, single::VFS_SINGLE_NAME)),
        None => Connection::open_with_flags_and_vfs(path, flags, if fixed { vfs::VFS_FIXED_NAME } else { vfs::VFS_NAME }),
    }
    .map_err(open_err)?;
    let k = master.derive(b"nv sqlcipher");
    let sql = Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", pq::hex(k.bytes())));
    conn.execute_batch(&sql).map_err(open_err)?;
    // Every cipher parameter is pinned (SQLCipher applies them after the key, before the first
    // page is read): a SQLCipher upgrade that changes its defaults must not change how existing
    // volumes are read. `cipher_compatibility = 4` resets to the SQLCipher 4 defaults first,
    // then each one this format depends on is set explicitly.
    conn.execute_batch(CIPHER_PRAGMAS).map_err(open_err)?;
    if layout.page_size != Layout::LEGACY.page_size {
        conn.execute_batch(&format!("PRAGMA cipher_page_size = {};", layout.page_size)).map_err(open_err)?;
    }
    // locking_mode=EXCLUSIVE: one process per volume, no -shm file, and (on Windows) our
    // handle holds an exclusive byte-range lock so writes near SQLite's lock bytes,
    // which sit 128 KiB earlier in db coordinates, are never blocked.
    conn.execute_batch("PRAGMA locking_mode = EXCLUSIVE; PRAGMA foreign_keys = ON; PRAGMA cache_size = -16384;").map_err(open_err)?;
    // Take the exclusive lock now and keep it: with a rollback journal a session that only
    // read would otherwise hold just a shared lock, and a second open of the volume would get in.
    conn.execute_batch("BEGIN EXCLUSIVE; COMMIT;").map_err(open_err)?;
    if create {
        start_journal(&conn, journal, single)?;
        conn.execute_batch(SCHEMA).map_err(open_err)?;
    }
    // The password-manager migration runs in the callers, only once the database is known to
    // belong to this header (volume_id check), so nothing is written to a foreign database.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)).map_err(open_err)?;
    Ok(conn)
}

/// Puts the database in this build's journal mode. Called once the database is known to be
/// the header's (it may write: a volume last closed or crashed in WAL mode by an older build is
/// recovered from its WAL and switched; the journal mode is not part of the format, any build
/// opens either).
/// A single-file volume's journal is cut back to its in-file area after every commit (the
/// tail past the database is given back); a side-file journal is kept up to 8 MiB.
fn start_journal(conn: &Connection, journal: Journal, single: Option<single::Map>) -> std::result::Result<(), OpenError> {
    let (want, sync) = match (journal, single) {
        (Journal::Wal, Some(_)) => return Err(OpenError::Io("a single-file volume has no WAL".into())),
        (Journal::Persist, _) => ("persist", "FULL"),
        (Journal::Wal, None) => ("wal", "NORMAL"),
    };
    let limit = single.map_or(8 << 20, |m| m.jbase());
    let mode: String = conn.query_row(&format!("PRAGMA journal_mode = {want}"), [], |r| r.get(0)).map_err(open_err)?;
    if !mode.eq_ignore_ascii_case(want) {
        return Err(OpenError::Io(format!("could not set journal_mode {want} (got {mode})")));
    }
    conn.execute_batch(&format!("PRAGMA synchronous = {sync}; PRAGMA journal_size_limit = {limit};")).map_err(open_err)
}

fn open_err(e: rusqlite::Error) -> OpenError {
    use rusqlite::ErrorCode::*;
    match e.sqlite_error_code() {
        Some(DatabaseBusy | DatabaseLocked) => OpenError::Io("volume is already open".into()),
        Some(CannotOpen | DiskFull | ReadOnly | PermissionDenied | SystemIoFailure) => OpenError::Io(e.to_string()),
        _ => OpenError::Corrupt(e.to_string()),
    }
}

/// Reads both header copies; returns the newest valid one, the per-copy seqs, and whether
/// the copies disagree (one torn/corrupt, or a crash between the two copy writes left the
/// older copy with stale slots — e.g. a password that was since changed).
fn read_headers(f: &File) -> std::result::Result<(Header, [u64; 2], bool), OpenError> {
    let mut buf = vec![0u8; 2 * COPY_LEN];
    let n = read_at(f, 0, &mut buf).map_err(|e| OpenError::Io(e.to_string()))?;
    if n < 2 * COPY_LEN {
        return Err(if buf[..n.min(8)] == MAGIC[..n.min(8)] && n >= 8 {
            OpenError::Corrupt("truncated header".into())
        } else {
            OpenError::NotAVault
        });
    }
    let a = header::decode_copy(&buf[..COPY_LEN]);
    let b = header::decode_copy(&buf[COPY_LEN..]);
    use header::CopyError::*;
    let seq = |r: &std::result::Result<Header, header::CopyError>| r.as_ref().map(|h| h.body.seq).unwrap_or(0);
    let seqs = [seq(&a), seq(&b)];
    match (a, b) {
        // A copy from a newer build wins over everything: falling back to the other (older)
        // copy and "repairing" would overwrite the newer data.
        (Err(Unsupported(m)), _) | (_, Err(Unsupported(m))) => Err(OpenError::UnsupportedVersion(m)),
        (Ok(a), Ok(b)) => {
            let same =
                |x: &Header| postcard::to_allocvec(&(&x.body.volume_id, &x.body.slots, &x.appearance)).unwrap_or_default();
            let stale = same(&a) != same(&b);
            Ok((if b.body.seq > a.body.seq { b } else { a }, seqs, stale))
        }
        (Ok(h), Err(_)) | (Err(_), Ok(h)) => Ok((h, seqs, true)),
        (Err(NoMagic), Err(NoMagic)) => Err(OpenError::NotAVault),
        (Err(Bad(m)), _) | (_, Err(Bad(m))) => Err(OpenError::Corrupt(m)),
    }
}

/// `meta` key: the lowest header seq the database accepts. A header older than it is a rollback
/// attack: an old copy written back would revive the passwords and slots it had. Copies
/// are written in pairs (seq s-1, then s, same content), so the floor is the newest seq - 1:
/// either copy of the latest pair opens, any earlier header does not.
const HEADER_SEQ: &str = "nv.header_seq";

fn db_header_seq(conn: &Connection) -> rusqlite::Result<Option<u64>> {
    let v: Option<Vec<u8>> = conn.query_row("SELECT v FROM meta WHERE k = ?", [HEADER_SEQ], |r| r.get(0)).optional()?;
    Ok(v.and_then(|v| std::str::from_utf8(&v).ok()?.parse().ok()))
}

fn set_db_header_seq(conn: &Connection, newest: u64) -> rusqlite::Result<()> {
    let seq = newest.saturating_sub(1);
    conn.execute("INSERT OR REPLACE INTO meta(k, v) VALUES (?, ?)", params![HEADER_SEQ, seq.to_string().as_bytes()]).map(|_| ())
}

fn side_path(path: &Path, suffix: &str) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(suffix);
    p.into()
}

/// Where [`Vault::rotate_master`] builds the re-keyed volume.
fn rekey_path(path: &Path) -> PathBuf {
    side_path(path, "-rekey")
}

fn sync_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    if let Some(d) = path.parent() {
        File::open(if d.as_os_str().is_empty() { Path::new(".") } else { d })?.sync_all()?;
    }
    let _ = path;
    Ok(())
}

/// Copy `which` (0, 1) if it decodes (checksum and format), else None.
fn read_copy(f: &File, which: usize) -> Option<Header> {
    let mut buf = vec![0u8; COPY_LEN];
    (read_at(f, (which * COPY_LEN) as u64, &mut buf).ok()? == COPY_LEN).then_some(())?;
    header::decode_copy(&buf).ok()
}

/// Writes the header (`base` + `appearance`) to both copies: the older copy first, fsync, then the other.
#[allow(clippy::too_many_arguments)]
fn write_headers(
    f: &File,
    master: &Key32,
    base: &HeaderBody,
    appearance: Option<&Appearance>,
    layout: Option<Layout>,
    fixed: Option<Fixed>,
    journal: Option<JournalRegion>,
    copy_seq: &mut [u64; 2],
) -> Result<Header> {
    let mac_key = master.derive(b"nv header mac");
    let appearance = appearance.map(|a| header::sign_appearance(master, &base.volume_id, a.clone()));
    let first = if copy_seq[0] <= copy_seq[1] { 0 } else { 1 };
    let mut seq = copy_seq[0].max(copy_seq[1]);
    let mut last = None;
    for i in [first, 1 - first] {
        seq += 1;
        let mut body = base.clone();
        body.seq = seq;
        let h = Header {
            header_mac: header::mac(&mac_key, &body, layout.as_ref(), fixed.as_ref(), journal.as_ref()),
            body,
            appearance: appearance.clone(),
            layout,
            fixed,
            journal,
        };
        write_at(f, (i * COPY_LEN) as u64, &header::encode_copy(&h)).and_then(|_| f.sync_all()).map_err(ioerr)?;
        #[cfg(test)]
        single::crash::point();
        copy_seq[i] = seq;
        last = Some(h);
        if i == first {
            resize::failpoint("header-first-copy");
        }
    }
    Ok(last.unwrap())
}

/// Lays out a new (or rebuilt) volume's store area once its header copies are written: a fixed
/// size is allocated, then its footer (side-file journal) or the journal region (single-file)
/// is written, synced.
fn init_store(f: &File, fixed: Option<Fixed>, single: Option<JournalRegion>) -> std::io::Result<()> {
    match (fixed, single) {
        (Some(fx), None) => vfs::init_fixed(f, fx.capacity),
        (Some(fx), Some(j)) => {
            vfs::allocate(f, fx.capacity)?;
            single::init_region(f, j.region, 0, false, fx.capacity)
        }
        (None, Some(j)) => single::init_region(f, j.region, 0, false, 0),
        (None, None) => Ok(()),
    }
}

impl Vault {
    /// Creates a new volume with one password slot and [`DEFAULT_LAYOUT`], single-file (format v2,
    /// see [`NEW_VOLUMES_SINGLE_FILE`]). Fails if `path` exists.
    pub fn create(path: &Path, password: &SecretString, kdf: KdfPreset) -> Result<Vault> {
        Self::create_full(path, password, kdf, Some(DEFAULT_LAYOUT), None, JOURNAL, NEW_VOLUMES_SINGLE_FILE.then_some(single::DEFAULT_REGION))
    }

    /// Creates a **fixed-size** volume: the file is preallocated to `capacity` bytes (rounded
    /// down to 4 KiB) and never grows or shrinks; writes past it fail with "disk full". Its
    /// header is format v2 with a `fixed` section, so builds before this one refuse it
    /// (`unsupported_version`) instead of growing it.
    /// Single-file like [`Self::create`] ([`NEW_VOLUMES_SINGLE_FILE`]): the journal region is
    /// inside the capacity ([`single::region_for`]).
    pub fn create_fixed(path: &Path, password: &SecretString, kdf: KdfPreset, capacity: u64) -> Result<Vault> {
        Self::create_fixed_as(path, password, kdf, capacity, NEW_VOLUMES_SINGLE_FILE)
    }

    /// [`Self::create_fixed`] with the side-file journal (`single = false`): what builds before
    /// fixed-size single-file volumes made. For compat fixtures, benches and tests only.
    #[doc(hidden)]
    pub fn create_fixed_as(path: &Path, password: &SecretString, kdf: KdfPreset, capacity: u64, single: bool) -> Result<Vault> {
        let fixed = Fixed { capacity: capacity - capacity % 4096 };
        if !fixed.is_valid() {
            return Err(Error::Io(format!("fixed volume size out of range: {capacity} bytes")));
        }
        Self::create_full(path, password, kdf, Some(DEFAULT_LAYOUT), Some(fixed), JOURNAL, single.then(|| single::region_for(fixed.capacity)))
    }

    /// `layout: None` = a format v1 volume ([`Layout::LEGACY`], no layout section), exactly
    /// what builds before stored layouts made; tests use it with `Journal::Wal`. A fixed
    /// volume always has a layout section (the `fixed` section trails it).
    #[cfg(test)]
    fn create_with(
        path: &Path,
        password: &SecretString,
        kdf: KdfPreset,
        layout: Option<Layout>,
        fixed: Option<Fixed>,
        journal: Journal,
    ) -> Result<Vault> {
        Self::create_full(path, password, kdf, layout, fixed, journal, None)
    }

    /// [`Self::create_with`], plus `single`: a single-file volume with this journal region
    /// (format v2 with a `journal` section).
    #[allow(clippy::too_many_arguments)]
    fn create_full(
        path: &Path,
        password: &SecretString,
        kdf: KdfPreset,
        layout: Option<Layout>,
        fixed: Option<Fixed>,
        journal: Journal,
        single: Option<u64>,
    ) -> Result<Vault> {
        let single = single.map(|r| if fixed.is_some() { JournalRegion::fresh_fixed(r) } else { JournalRegion::fresh(r) });
        let layout = match (fixed, single) {
            (None, None) => layout.filter(|l| *l != Layout::LEGACY), // same store as v1: stay readable by older builds
            _ => Some(layout.unwrap_or(Layout::LEGACY)),
        };
        let hfile = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists { Error::Exists } else { ioerr(e) }
        })?;
        let r = (|| {
            let master = Key32::random();
            let volume_id: [u8; 16] = random();
            let (argon2, salt, kek) = header::new_password_kek(password.expose_secret().as_bytes(), kdf);
            let (nonce, wrapped) = header::wrap(&kek, &master, &volume_id, 0, 0);
            let base = HeaderBody {
                magic: MAGIC,
                version: if layout.is_some() { header::VERSION_LAYOUT } else { VERSION },
                volume_id,
                seq: 0,
                slots: vec![Some(SlotEntry::new(Slot::Password { argon2, salt, nonce, wrapped }, None))],
            };
            let mut copy_seq = [0, 0];
            let header = write_headers(&hfile, &master, &base, None, layout, fixed, single, &mut copy_seq)?;
            init_store(&hfile, fixed, single).map_err(ioerr)?;
            let l = layout.unwrap_or(Layout::LEGACY);
            let map = single.map(|j| single::Map::new(j, fixed));
            let conn = open_db(path, &master, true, l, fixed.is_some(), journal, map).map_err(|e| Error::Io(e.to_string()))?;
            conn.execute("INSERT INTO nodes(id, parent, name, kind, size, mtime) VALUES (1, 1, '', 1, 0, ?)", [now_ns()])
                .map_err(dberr)?;
            conn.execute("INSERT INTO meta(k, v) VALUES ('volume_id', ?)", [&volume_id[..]]).map_err(dberr)?;
            set_db_header_seq(&conn, header.body.seq).map_err(dberr)?;
            fresh::record_appearance(&conn, "main", &header).map_err(dberr)?;
            passwords::migrate(&conn).map_err(dberr)?;
            props::migrate(&conn).map_err(dberr)?;
            search::migrate(&conn).map_err(dberr)?;
            if journal == Journal::Wal {
                conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE)").map_err(dberr)?;
            }
            Ok(Inner {
                conn,
                bufs: HashMap::new(),
                buffered: 0,
                dirty_since: None,
                in_tx: false,
                tx_bytes: 0,
                chunk: l.chunk_size as usize,
                journal,
                master,
                header,
                unlocked: Some(0),
                copy_seq,
                appearance_tampered: false,
                search: search::Cache::default(),
                wb: Writeback::for_file(&hfile),
                hfile,
            })
        })();
        match r {
            Ok(inner) => Ok(Vault::start(path, inner)),
            Err(e) => {
                for suffix in ["", "-wal", "-shm", "-journal"] {
                    let mut p = path.as_os_str().to_owned();
                    p.push(suffix);
                    let _ = std::fs::remove_file(p);
                }
                Err(e)
            }
        }
    }

    pub fn open(path: &Path, password: &SecretString) -> std::result::Result<Vault, OpenError> {
        Self::open_with(path, |h, f, copy_seq| {
            let id = &h.body.volume_id;
            let mut duress = false;
            for (i, s) in h.body.slots.iter().enumerate() {
                if let Some(SlotEntry { slot: Slot::Password { argon2, salt, nonce, wrapped }, .. }) = s {
                    let Some(kek) = header::argon2_kek(password.expose_secret().as_bytes(), *argon2, salt) else { continue };
                    if let Some(m) = header::unwrap(&kek, nonce, wrapped, id, i, 0) {
                        return Ok((m, i));
                    }
                    // Always tried (same cost for every attempt); the loop goes on either way,
                    // so a duress attempt runs every slot's Argon2id like a wrong password.
                    duress |= header::unwrap(&kek, nonce, wrapped, id, i, duress::KIND).is_some();
                }
            }
            if duress {
                // Best effort: whatever happens, the answer is the wrong-password one.
                let _ = duress::destroy(f, h, copy_seq, 2);
            }
            Err(OpenError::WrongPassword)
        })
    }

    pub fn open_with_identity(path: &Path, identity: &Identity) -> std::result::Result<Vault, OpenError> {
        let fpr = identity.recipient().fingerprint();
        Self::open_with(path, |h, _, _| {
            let id = &h.body.volume_id;
            for (i, s) in h.body.slots.iter().enumerate() {
                if let Some(SlotEntry { slot: Slot::Recipient { x25519_eph_pub, mlkem_ct, recipient_fpr, nonce, wrapped }, .. }) = s {
                    if *recipient_fpr != fpr {
                        continue;
                    }
                    let Some(kek) = identity.slot_kek(id, x25519_eph_pub, mlkem_ct) else { continue };
                    if let Some(m) = header::unwrap(&kek, nonce, wrapped, id, i, 1) {
                        return Ok((m, i));
                    }
                }
            }
            Err(OpenError::WrongPassword)
        })
    }

    fn open_with(
        path: &Path,
        unlock: impl Fn(&Header, &File, [u64; 2]) -> std::result::Result<(Key32, usize), OpenError>,
    ) -> std::result::Result<Vault, OpenError> {
        let hfile = std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(|e| OpenError::Io(e.to_string()))?;
        let (mut header, mut copy_seq, mut needs_repair) = read_headers(&hfile)?;
        let authentic = |r: &std::result::Result<(Key32, usize), OpenError>, h: &Header| {
            r.as_ref().is_ok_and(|(m, _)| header::mac_ok(&m.derive(b"nv header mac"), h))
        };
        let mut tried = unlock(&header, &hfile, copy_seq);
        if tried.is_ok() && !authentic(&tried, &header) {
            // The checksum is unkeyed: anyone can write a "newest" copy. When the password opens it
            // but it does not authenticate, the other copy still may. Never after a
            // failed unlock: the other copy may be a stale, authentic one with an old password.
            let picked = if copy_seq[1] > copy_seq[0] { 1 } else { 0 };
            if let Some(other) = read_copy(&hfile, 1 - picked) {
                let r = unlock(&other, &hfile, copy_seq);
                if authentic(&r, &other) {
                    copy_seq[picked] = 0; // the forged copy is rewritten first by the repair below
                    header = other;
                    tried = r;
                }
            }
        }
        let (master, unlocked) = tried?;
        if !header::mac_ok(&master.derive(b"nv header mac"), &header) {
            return Err(OpenError::Corrupt("header authentication failed".into()));
        }
        let layout = header.layout.unwrap_or(Layout::LEGACY);
        let journal = JOURNAL;
        if let Some(f) = header.fixed {
            // The footer the shim keeps must have a record of this capacity (a single-file one's
            // records are checked by its shim), and the file must be at least that long (longer:
            // a resize was interrupted, see resize.rs; settled below).
            let len = hfile.metadata().map_err(|e| OpenError::Io(e.to_string()))?.len();
            if len < f.capacity || (header.journal.is_none() && vfs::read_footer_for(&hfile, f.capacity).is_none()) {
                return Err(OpenError::Corrupt("fixed-size volume: the file does not have its size or footer".into()));
            }
        }
        let map = header.journal.map(|j| single::Map::new(j, header.fixed));
        let conn = match (header.fixed, map) {
            (Some(f), None) => vfs::expect_capacity(f.capacity, || open_db(path, &master, false, layout, true, journal, None))?,
            (f, m) => open_db(path, &master, false, layout, f.is_some(), journal, m)?,
        };
        let vid: Option<Vec<u8>> = conn
            .query_row("SELECT v FROM meta WHERE k = 'volume_id'", [], |r| r.get(0))
            .optional()
            .map_err(|e| OpenError::Corrupt(e.to_string()))?;
        if vid.as_deref() != Some(&header.body.volume_id[..]) {
            return Err(OpenError::Corrupt("database does not belong to this header".into()));
        }
        // Headers are written before the database records their seq, so a crash in between leaves
        // the header newer, never older. Volumes from before this check have no record yet.
        let seen = db_header_seq(&conn).map_err(open_err)?;
        if seen.is_some_and(|s| header.body.seq < s) {
            return Err(OpenError::RolledBack);
        }
        start_journal(&conn, journal, map)?;
        if let Some(f) = header.fixed {
            // Under the exclusive lock now: an interrupted grow is undone / shrink finished.
            resize::settle(&hfile, f.capacity).map_err(|e| OpenError::Io(e.to_string()))?;
        }
        // A key rotation that crashed before its rename: this (old) volume is intact. Removed only
        // now that the exclusive lock is held, so never under a rotation in progress.
        for t in [rekey_path(path), side_path(&rekey_path(path), "-journal"), resize::side(path), side_path(&resize::side(path), "-journal")] {
            let _ = std::fs::remove_file(t);
        }
        passwords::migrate(&conn).map_err(open_err)?;
        passwords::repair_once(&conn);
        props::migrate(&conn).map_err(open_err)?;
        search::migrate(&conn).map_err(open_err)?;
        // The appearance has its own MAC: a forged one is dropped (and the header repaired
        // without it) instead of refusing the volume. The app rewrites it from the volume.
        // An authentic but older one (or one stripped) under this header is caught by the
        // database's record of what this header carried.
        let replayed = fresh::appearance_replayed(&conn, &header).map_err(open_err)?;
        let appearance_tampered = replayed || header.appearance.as_ref().is_some_and(|a| !header::appearance_ok(&master, &header.body.volume_id, a));
        if appearance_tampered {
            header.appearance = None;
            needs_repair = true;
        }
        if needs_repair {
            // Rewrite both copies from the newest one, now that we know it is authentic.
            let a = header.appearance.as_ref().map(|a| &a.appearance);
            header = write_headers(&hfile, &master, &header.body, a, header.layout, header.fixed, header.journal, &mut copy_seq)
                .map_err(|e| OpenError::Io(e.to_string()))?;
        }
        if seen.is_none_or(|s| s < header.body.seq.saturating_sub(1)) {
            set_db_header_seq(&conn, header.body.seq).map_err(open_err)?;
        }
        if !fresh::appearance_current(&conn, &header).map_err(open_err)? {
            fresh::record_appearance(&conn, "main", &header).map_err(open_err)?;
        }
        let inner = Inner {
            conn,
            bufs: HashMap::new(),
            buffered: 0,
            dirty_since: None,
            in_tx: false,
            tx_bytes: 0,
            chunk: layout.chunk_size as usize,
            journal,
            master,
            header,
            unlocked: Some(unlocked),
            copy_seq,
            appearance_tampered,
            search: search::Cache::default(),
            wb: Writeback::for_file(&hfile),
            hfile,
        };
        Ok(Vault::start(path, inner))
    }

    fn start(path: &Path, inner: Inner) -> Vault {
        let shared = Arc::new(Shared {
            inner: Mutex::new(Some(inner)),
            stop: Mutex::new(false),
            wake: Condvar::new(),
            commit: Mutex::new(()),
            committing: Default::default(),
        });
        let weak = Arc::downgrade(&shared);
        // Background durability: a durability point once the oldest change is DURABLE_INTERVAL old.
        let flusher = std::thread::Builder::new()
            .name("nv-commit".into())
            .spawn(move || {
                loop {
                    let Some(s) = weak.upgrade() else { return };
                    {
                        let stop = s.stop.lock().unwrap_or_else(|e| e.into_inner());
                        let (stop, _) = s.wake.wait_timeout_while(stop, DURABLE_INTERVAL / 8, |stop| !*stop).unwrap_or_else(|e| e.into_inner());
                        if *stop {
                            return;
                        }
                    }
                    let _ = s.durable_point(true); // retried on the next tick / op on failure
                }
            })
            .expect("spawn commit thread");
        Vault { path: path.to_owned(), shared, flusher: Mutex::new(Some(flusher)), slot_lock: Mutex::new(()) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The volume's random id (header, plaintext): what its document number is made from.
    pub fn volume_id(&self) -> Option<[u8; 16]> {
        let g = self.lock().ok()?;
        g.as_ref().map(|i| i.header.body.volume_id)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Option<Inner>>> {
        self.shared.lock()
    }

    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> Result<T>) -> Result<T> {
        let mut g = self.lock()?;
        let i = g.as_mut().unwrap();
        let r = f(i);
        i.wb.account();
        probe(&i.conn)?;
        r
    }

    /// Removes a file nobody asked to keep (a cancelled or failed import) without holding up the
    /// volume (cancel-hang): it is cut from its end, [`DISCARD_STEP`] at a time, each step a
    /// durability point (whose slow part runs outside the store lock), then removed. One DELETE of
    /// a 2 GB file held the store lock for minutes: SQLCipher's `secure_delete` journals and
    /// overwrites every freed page. Here the freed pages are left as they are
    /// (`secure_delete = FAST`, for these statements only): what they hold is ciphertext of a
    /// file that is still on the host, as the pages a crash in the middle of the import leaves
    /// behind; they are reused by the next writes. Already gone: Ok. A crash in the middle leaves
    /// a shorter file (each step is committed).
    pub fn discard(&self, id: NodeId) -> Result<()> {
        loop {
            let size = match self.getattr(id) {
                Ok(n) if n.kind == Kind::File => n.size,
                Ok(_) => return Err(Error::IsDir),
                Err(Error::NotFound) => return Ok(()),
                Err(e) => return Err(e),
            };
            let last = size <= DISCARD_STEP;
            self.with(|i| {
                let was: i64 = i.conn.query_row("PRAGMA secure_delete", [], |r| r.get(0)).map_err(dberr)?;
                i.conn.query_row("PRAGMA secure_delete = FAST", [], |_| Ok(())).map_err(dberr)?;
                let r = if last { i.remove(id) } else { i.set_size(id, (size - 1) / DISCARD_STEP * DISCARD_STEP) }.and_then(|_| i.spill());
                // (restored on every way out)
                let back = i.conn.query_row(&format!("PRAGMA secure_delete = {was}"), [], |_| Ok(())).map_err(dberr);
                r.and(back)
            })?;
            self.shared.durable_point(false)?;
            if last {
                return Ok(());
            }
        }
    }

    /// Durability point, then close the database (no side file is left behind) and wipe keys.
    pub fn close(self) -> Result<()> {
        self.shutdown()
    }

    fn shutdown(&self) -> Result<()> {
        *self.shared.stop.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.shared.wake.notify_all();
        if let Some(h) = self.flusher.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = h.join();
        }
        let Some(mut inner) = self.shared.inner.lock().unwrap_or_else(|e| e.into_inner()).take() else { return Ok(()) };
        let r1 = inner.durable();
        let r2 = match inner.journal {
            Journal::Wal => inner.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").map_err(dberr),
            // Leaving PERSIST deletes the journal file (nothing to roll back once durable).
            Journal::Persist if r1.is_ok() => inner.conn.query_row("PRAGMA journal_mode = DELETE", [], |_| Ok(())).map_err(dberr),
            Journal::Persist => Ok(()),
        };
        let Inner { conn, master, hfile, .. } = inner;
        let r3 = conn.close().map_err(|(_, e)| dberr(e));
        drop(master); // zeroized
        drop(hfile);
        if r1.is_ok() && r2.is_ok() && r3.is_ok() {
            // SQLite removes the WAL after a clean last close; tidy anything empty left behind.
            for suffix in ["-wal", "-shm", "-journal"] {
                let mut p = self.path.as_os_str().to_owned();
                p.push(suffix);
                if std::fs::metadata(&p).is_ok_and(|m| m.len() == 0 || suffix == "-shm") {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
        r1.and(r2).and(r3)
    }

    // ---------------- keyslots ----------------

    pub fn slots(&self) -> Vec<SlotInfo> {
        let Ok(g) = self.lock() else { return vec![] };
        let i = g.as_ref().unwrap();
        let vid = &i.header.body.volume_id;
        i.header
            .body
            .slots
            .iter()
            .enumerate()
            .filter_map(|(index, e)| {
                let e = e.as_ref()?;
                let label = e.label.as_ref().and_then(|l| header::open_label(&i.master, vid, index, l));
                let (kind, kdf, fingerprint) = match &e.slot {
                    s @ Slot::Password { argon2, .. } if duress::is_duress(&i.master, vid, index, s) => ("duress", Some(*argon2), None),
                    Slot::Password { argon2, .. } => ("password", Some(*argon2), None),
                    Slot::Recipient { recipient_fpr, .. } => ("recipient", None, Some(pq::hex(recipient_fpr))),
                    Slot::Fido2(_) => ("fido2", None, None),
                };
                Some(SlotInfo { index, kind, label, kdf, fingerprint, in_use: i.unlocked == Some(index) })
            })
            .collect()
    }

    /// The slot that unlocked this session (`None` if it has since been removed, or locked).
    pub fn unlocked_slot(&self) -> Option<usize> {
        self.lock().ok()?.as_ref()?.unlocked
    }

    /// Whether `pw` opens password slot `slot`. Runs the slot's Argon2id, then the AEAD unwrap
    /// (whose tag check is constant-time); false for a missing or non-password slot.
    pub fn verify_password(&self, slot: usize, pw: &SecretString) -> bool {
        let Ok(Some((argon2, salt, nonce, wrapped, vid))) = self.with(|i| {
            Ok(match i.header.body.slots.get(slot) {
                Some(Some(SlotEntry { slot: Slot::Password { argon2, salt, nonce, wrapped }, .. })) => {
                    Some((*argon2, *salt, *nonce, wrapped.clone(), i.header.body.volume_id))
                }
                _ => None,
            })
        }) else {
            return false;
        };
        // Slow KDF without the store lock held.
        header::argon2_kek(pw.expose_secret().as_bytes(), argon2, &salt)
            .and_then(|kek| header::unwrap(&kek, &nonce, &wrapped, &vid, slot, 0))
            .is_some()
    }

    /// (free index, volume_id, master copy) — read under the store lock.
    fn slot_ctx(&self, want: Option<usize>) -> std::result::Result<(usize, [u8; 16], Key32), SlotError> {
        self.with(|i| {
            let slots = &i.header.body.slots;
            let idx = match want {
                Some(w) => Some(w),
                None => slots.iter().position(Option::is_none).or((slots.len() < MAX_SLOTS).then_some(slots.len())),
            };
            Ok(idx.map(|idx| (idx, i.header.body.volume_id, i.master.clone())))
        })?
        .ok_or(SlotError::Full)
    }

    /// Writes `slot` at `idx` (None = remove). `label`: `Some(x)` sets (None inside = clear), `None` keeps.
    fn put_slot(&self, idx: usize, slot: Option<Slot>, label: Option<Option<&str>>) -> std::result::Result<(), SlotError> {
        self.with(|i| {
            let mut body = i.header.body.clone();
            if idx >= body.slots.len() {
                body.slots.resize(idx + 1, None);
            }
            let vid = body.volume_id;
            let old_label = body.slots[idx].take().and_then(|e| e.label);
            body.slots[idx] = slot.map(|s| {
                let label = match label {
                    Some(l) => l.map(|l| header::seal_label(&i.master, &vid, idx, l)),
                    None => old_label,
                };
                SlotEntry::new(s, label)
            });
            while body.slots.last().is_some_and(Option::is_none) {
                body.slots.pop();
            }
            let a = i.header.appearance.as_ref().map(|a| a.appearance.clone());
            i.header = write_headers(&i.hfile, &i.master, &body, a.as_ref(), i.header.layout, i.header.fixed, i.header.journal, &mut i.copy_seq)?;
            i.record_header_seq()?;
            if body.slots.get(idx).is_none_or(Option::is_none) && i.unlocked == Some(idx) {
                i.unlocked = None;
            }
            Ok(())
        })?;
        Ok(())
    }

    fn password_slot(pw: &SecretString, kdf: KdfPreset, master: &Key32, vid: &[u8; 16], idx: usize) -> Slot {
        let (argon2, salt, kek) = header::new_password_kek(pw.expose_secret().as_bytes(), kdf);
        let (nonce, wrapped) = header::wrap(&kek, master, vid, idx, 0);
        Slot::Password { argon2, salt, nonce, wrapped }
    }

    /// The slot at `idx` (cloned), or `NotFound`.
    fn slot_at(&self, idx: usize) -> std::result::Result<Slot, SlotError> {
        self.with(|i| Ok(i.header.body.slots.get(idx).cloned().flatten()))?.map(|e| e.slot).ok_or(SlotError::NotFound)
    }

    /// Adds a password slot (optionally labelled) in the first free position; returns its index.
    pub fn add_password(&self, pw: &SecretString, kdf: KdfPreset, label: Option<&str>) -> std::result::Result<usize, SlotError> {
        let label = norm_label(label)?;
        let _s = self.slot_lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.pw_matches(pw, |duress| duress)? {
            return Err(SlotError::DuressConflict);
        }
        let (idx, vid, master) = self.slot_ctx(None)?;
        let slot = Self::password_slot(pw, kdf, &master, &vid, idx); // slow KDF, store unlocked
        self.put_slot(idx, Some(slot), Some(label))?;
        Ok(idx)
    }

    /// Whether `pw` opens one of the password-shaped slots `pick(is_duress)` selects (a duress
    /// slot "opens" when its duress wrap authenticates). Runs those slots' Argon2id.
    fn pw_matches(&self, pw: &SecretString, pick: impl Fn(bool) -> bool) -> Result<bool> {
        type Cand = (usize, KdfParams, [u8; 32], [u8; 24], Vec<u8>, bool);
        let (vid, cands): ([u8; 16], Vec<Cand>) = self.with(|i| {
            let vid = i.header.body.volume_id;
            let c = i.header.body.slots.iter().enumerate().filter_map(|(idx, e)| match &e.as_ref()?.slot {
                s @ Slot::Password { argon2, salt, nonce, wrapped } => {
                    Some((idx, *argon2, *salt, *nonce, wrapped.clone(), duress::is_duress(&i.master, &vid, idx, s)))
                }
                _ => None,
            });
            Ok((vid, c.filter(|c| pick(c.5)).collect()))
        })?;
        let pw = pw.expose_secret().as_bytes();
        Ok(cands.iter().any(|(idx, a, salt, nonce, w, d)| {
            let kind = if *d { duress::KIND } else { 0 };
            header::argon2_kek(pw, *a, salt).is_some_and(|k| header::unwrap(&k, nonce, w, &vid, *idx, kind).is_some())
        }))
    }

    fn is_duress_slot(&self, idx: usize) -> Result<bool> {
        self.with(|i| {
            let vid = i.header.body.volume_id;
            Ok(i.header.body.slots.get(idx).cloned().flatten().is_some_and(|e| duress::is_duress(&i.master, &vid, idx, &e.slot)))
        })
    }

    /// Adds a duress ("autodistruzione") slot: entering `pw` at unlock destroys every key of the
    /// volume, then fails like a wrong password. Its Argon2id
    /// parameters are copied from the first password slot so it looks like one; `kdf` is used
    /// only when the volume has no password slot. Refused (`DuressConflict`) when `pw` opens
    /// another slot. Returns its index.
    pub fn add_duress(&self, pw: &SecretString, kdf: KdfPreset, label: Option<&str>) -> std::result::Result<usize, SlotError> {
        let label = norm_label(label)?;
        let _s = self.slot_lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.pw_matches(pw, |_| true)? {
            return Err(SlotError::DuressConflict);
        }
        let (idx, vid, master) = self.slot_ctx(None)?;
        let params = self.with(|i| {
            Ok(i.header.body.slots.iter().enumerate().find_map(|(j, e)| match &e.as_ref()?.slot {
                s @ Slot::Password { argon2, .. } if !duress::is_duress(&i.master, &vid, j, s) => Some(*argon2),
                _ => None,
            }))
        })?;
        let pwb = pw.expose_secret().as_bytes();
        let (argon2, salt, kek) = match params {
            Some(a) => {
                let (salt, kek) = header::password_kek_with(pwb, a);
                (a, salt, kek)
            }
            None => header::new_password_kek(pwb, kdf),
        };
        let slot = duress::new_slot(&kek, argon2, salt, &master, &vid, idx);
        self.put_slot(idx, Some(slot), Some(label))?;
        Ok(idx)
    }

    /// Re-keys password slot `slot` with `new`. `label`: `Some` replaces it (empty clears),
    /// `None` keeps the current one. Does not check the old password: see [`Self::verify_password`].
    pub fn change_password(
        &self,
        slot: usize,
        new: &SecretString,
        kdf: KdfPreset,
        label: Option<&str>,
    ) -> std::result::Result<(), SlotError> {
        let label = label.map(|l| norm_label(Some(l))).transpose()?;
        let _s = self.slot_lock.lock().unwrap_or_else(|e| e.into_inner());
        if !matches!(self.slot_at(slot)?, Slot::Password { .. }) || self.is_duress_slot(slot)? {
            return Err(SlotError::NotPassword);
        }
        if self.pw_matches(new, |duress| duress)? {
            return Err(SlotError::DuressConflict);
        }
        let (idx, vid, master) = self.slot_ctx(Some(slot))?;
        let s = Self::password_slot(new, kdf, &master, &vid, idx);
        self.put_slot(idx, Some(s), label)
    }

    /// Adds a recipient (public-key) slot; returns its index.
    pub fn add_recipient(&self, r: &Recipient, label: Option<&str>) -> std::result::Result<usize, SlotError> {
        let label = norm_label(label)?;
        let _s = self.slot_lock.lock().unwrap_or_else(|e| e.into_inner());
        let (idx, vid, master) = self.slot_ctx(None)?;
        let ((x25519_eph_pub, mlkem_ct), kek) = r.seal(&vid);
        let (nonce, wrapped) = header::wrap(&kek, &master, &vid, idx, 1);
        let slot = Slot::Recipient { x25519_eph_pub, mlkem_ct, recipient_fpr: r.fingerprint(), nonce, wrapped };
        self.put_slot(idx, Some(slot), Some(label))?;
        Ok(idx)
    }

    /// Sets (or with `None` / empty, clears) the label of any slot.
    pub fn set_slot_label(&self, slot: usize, label: Option<&str>) -> std::result::Result<(), SlotError> {
        let label = norm_label(label)?;
        let _s = self.slot_lock.lock().unwrap_or_else(|e| e.into_inner());
        let s = self.slot_at(slot)?;
        self.put_slot(slot, Some(s), Some(label))
    }

    /// Removes a keyslot. Refuses to remove the last one that opens the volume (a duress slot
    /// never counts: it cannot be the only one left).
    pub fn remove_slot(&self, slot: usize) -> std::result::Result<(), SlotError> {
        let _s = self.slot_lock.lock().unwrap_or_else(|e| e.into_inner());
        self.slot_at(slot)?;
        let openers_left = self.with(|i| {
            let vid = i.header.body.volume_id;
            Ok(i.header.body.slots.iter().enumerate().filter(|&(j, e)| {
                j != slot && e.as_ref().is_some_and(|e| !duress::is_duress(&i.master, &vid, j, &e.slot))
            }).count())
        })?;
        if openers_left == 0 {
            return Err(SlotError::LastSlot);
        }
        self.put_slot(slot, None, None)
    }

    /// Re-encrypts the whole volume under a NEW master key: afterwards no old copy
    /// of the header, whatever password it holds, opens what is written from now on. Keeps only
    /// password slot `slot` (re-wrapped with `pw`, which must open it; label kept) and returns the
    /// indices of every other slot, which are dropped: their keys cannot be re-wrapped without
    /// their passwords (recipients: without their public keys). Duress slots go too.
    ///
    /// Crash-safe by construction: the new volume is written next to this one
    /// (`<vol>-rekey`, needs as much free space as the volume) and renamed over it only once
    /// complete and synced. A crash before the rename leaves the old volume as it was (the
    /// leftover is removed at the next open), after it the new one.
    pub fn rotate_master(&self, slot: usize, pw: &SecretString) -> std::result::Result<Vec<usize>, SlotError> {
        let _s = self.slot_lock.lock().unwrap_or_else(|e| e.into_inner());
        if !matches!(self.slot_at(slot)?, Slot::Password { .. }) || self.is_duress_slot(slot)? {
            return Err(SlotError::NotPassword);
        }
        if !self.verify_password(slot, pw) {
            return Err(SlotError::WrongPassword);
        }
        let tmp = rekey_path(&self.path);
        let mut g = self.lock()?;
        let i = g.as_mut().unwrap();
        i.durable()?;
        let m2 = Key32::random();
        let built = (|| -> Result<Vec<usize>> {
            let _ = std::fs::remove_file(&tmp);
            let vid = i.header.body.volume_id;
            let Some(Some(SlotEntry { slot: Slot::Password { argon2, .. }, label })) = i.header.body.slots.get(slot).cloned() else {
                return Err(Error::NotFound);
            };
            let (salt, kek) = header::password_kek_with(pw.expose_secret().as_bytes(), argon2);
            let (nonce, wrapped) = header::wrap(&kek, &m2, &vid, slot, 0);
            let label = label.and_then(|l| header::open_label(&i.master, &vid, slot, &l)).map(|l| header::seal_label(&m2, &vid, slot, &l));
            let mut slots = vec![None; slot + 1];
            slots[slot] = Some(SlotEntry::new(Slot::Password { argon2, salt, nonce, wrapped }, label));
            let dropped = (0..i.header.body.slots.len()).filter(|&j| j != slot && i.header.body.slots[j].is_some()).collect();
            let body = HeaderBody { slots, seq: 0, ..i.header.body.clone() };
            let f = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&tmp).map_err(ioerr)?;
            let appearance = i.header.appearance.as_ref().map(|a| a.appearance.clone());
            // A single-file volume's copy is single-file too (made in one piece: no moved bytes);
            // a fixed-size one's is as large as the volume (and attached through the same mode).
            let fixed = i.header.fixed;
            let single = i.header.journal.map(|j| if fixed.is_some() { JournalRegion::fresh_fixed(j.region) } else { JournalRegion::fresh(j.region) });
            let h = write_headers(&f, &m2, &body, appearance.as_ref(), i.header.layout, fixed, single, &mut [0, 0])?;
            init_store(&f, fixed, single).map_err(ioerr)?;
            drop(f);
            // The export goes through the same offset VFS (an attached database uses the
            // connection's VFS) and gets exactly the cipher settings `open_db` pins.
            let name = tmp.to_str().ok_or(Error::InvalidName)?.replace('\'', "''");
            let key = m2.derive(b"nv sqlcipher");
            let page = i.header.layout.unwrap_or(Layout::LEGACY).page_size;
            let attach = Zeroizing::new(format!("ATTACH DATABASE '{name}' AS rekey KEY \"x'{}'\";", pq::hex(key.bytes())));
            match single {
                Some(m) => single::with_map(single::Map::new(m, fixed), || i.conn.execute_batch(&attach)),
                None => i.conn.execute_batch(&attach),
            }
            .map_err(dberr)?;
            let r = i
                .conn
                .execute_batch(&format!(
                    "{} PRAGMA rekey.cipher_page_size = {page};",
                    CIPHER_PRAGMAS.replace("PRAGMA ", "PRAGMA rekey.")
                ))
                .and_then(|_| i.conn.query_row("SELECT sqlcipher_export('rekey')", [], |_| Ok(())))
                .and_then(|_| {
                    i.conn.execute(
                        "INSERT OR REPLACE INTO rekey.meta(k, v) VALUES (?, ?)",
                        params![HEADER_SEQ, h.body.seq.saturating_sub(1).to_string().as_bytes()],
                    )
                })
                .and_then(|_| fresh::record_appearance(&i.conn, "rekey", &h))
                .map_err(dberr);
            let d = i.conn.execute_batch("DETACH DATABASE rekey").map_err(dberr);
            r.and(d)?;
            Ok(dropped)
        })();
        let dropped = match built {
            Ok(d) => d,
            Err(e) => {
                for t in [tmp.clone(), side_path(&tmp, "-journal")] {
                    let _ = std::fs::remove_file(t);
                }
                return Err(e.into());
            }
        };
        // Swap: close this one (no journal left behind), rename the new one over it, reopen.
        let old = g.take().unwrap();
        let m1 = old.master.clone();
        let _ = old.conn.query_row("PRAGMA journal_mode = DELETE", [], |_| Ok(()));
        let Inner { conn, hfile, .. } = old;
        let _ = conn.close();
        drop(hfile);
        let renamed = std::fs::rename(&tmp, &self.path).and_then(|_| sync_dir(&self.path));
        let (m, r) = match renamed {
            Ok(()) => (m2, Ok(dropped)),
            Err(e) => (m1, Err(ioerr(e).into())),
        };
        let v = Self::open_with(&self.path, |_, _, _| Ok((m.clone(), slot))).map_err(|e| Error::Io(format!("reopen after key rotation: {e}")))?;
        *g = v.shared.inner.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(v);
        if r.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        r
    }

    // ---------------- format ----------------

    /// The volume is single-file (format v2: its rollback journal lives inside the `.nvault`).
    pub fn is_single_file(&self) -> bool {
        self.with(|i| Ok(i.header.journal.is_some())).unwrap_or(false)
    }

    /// [`Self::upgrade_format`] can make this volume single-file (it is not yet; growable or
    /// fixed-size alike — a fixed-size one needs room for the region inside it, see
    /// [`single::region_to_upgrade`]; a nearly full one must be extended first).
    pub fn can_upgrade_format(&self) -> bool {
        self.with(|i| Ok(i.header.journal.is_none() && i.upgrade_region(single::DEFAULT_REGION)?.is_some())).unwrap_or(false)
    }

    /// "Aggiorna formato": makes this volume single-file, in place,
    /// in about the time it takes to copy 4 MiB. Crash-safe at every step: the database's first
    /// region-sized bytes are copied past its end and synced (old format untouched), then the
    /// header is rewritten as v2 with a `journal` section (the switch: two-copy seq protocol), then
    /// the region where those bytes were becomes the journal's records and area. Afterwards builds
    /// before this one refuse the volume (`unsupported_version`); it is never converted back.
    /// Returns `false` if the volume already was single-file. The session goes on (reopened).
    pub fn upgrade_format(&self) -> Result<bool> {
        self.upgrade_format_with(single::DEFAULT_REGION)
    }

    fn upgrade_format_with(&self, region: u64) -> Result<bool> {
        let _s = self.slot_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut g = self.lock()?;
        let i = g.as_mut().unwrap();
        if i.header.journal.is_some() {
            return Ok(false);
        }
        i.durable()?;
        let fixed = i.header.fixed;
        let region = i.upgrade_region(region)?.ok_or_else(|| Error::Io(UPGRADE_NO_ROOM.into()))?;
        let Inner { conn, master, header, mut copy_seq, hfile, unlocked, .. } = g.take().unwrap();
        // Leaving PERSIST deletes the side journal (nothing to roll back once durable).
        let r = conn
            .query_row("PRAGMA journal_mode = DELETE", [], |_| Ok(()))
            .map_err(dberr)
            .and(conn.close().map_err(|(_, e)| dberr(e)))
            .and_then(|_| {
                let layout = header.layout.unwrap_or(Layout::LEGACY);
                // a fixed-size volume's database size is in its footer (the file is the capacity)
                let fx = match fixed {
                    Some(f) => Some((
                        f.capacity,
                        vfs::read_footer_for(&hfile, f.capacity).ok_or_else(|| Error::Corrupt("fixed-size volume: no footer".into()))?.1.logical,
                    )),
                    None => None,
                };
                let (map, db) = single::move_out(&hfile, layout.page_size, region, fx).map_err(ioerr)?;
                let body = HeaderBody { version: header::VERSION_LAYOUT, ..header.body.clone() };
                let a = header.appearance.as_ref().map(|a| &a.appearance);
                write_headers(&hfile, &master, &body, a, Some(layout), fixed, Some(map), &mut copy_seq)?;
                single::init_region(&hfile, map.region, db, true, fixed.map_or(0, |f| f.capacity)).map_err(ioerr)
            });
        drop(hfile);
        // Reopen in whichever format the file is now (the old one if this failed before the switch).
        let v = Self::open_with(&self.path, |_, _, _| Ok((master.clone(), unlocked.unwrap_or(0))))
            .map_err(|e| Error::Io(format!("reopen after format upgrade: {e}")))?;
        *g = v.shared.inner.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(i) = g.as_mut() {
            i.unlocked = unlocked;
        }
        drop(v);
        r.map(|_| true)
    }

    // ---------------- appearance ----------------

    /// `Some(bytes)` for a fixed-size (preallocated) volume: the file's constant size.
    pub fn capacity(&self) -> Option<u64> {
        self.with(|i| Ok(i.header.fixed.map(|f| f.capacity))).ok().flatten()
    }

    /// The header's plaintext appearance section (`None` = absent: neutral locked screens).
    pub fn appearance(&self) -> Option<Appearance> {
        self.with(|i| Ok(i.header.appearance.as_ref().map(|a| a.appearance.clone()))).ok().flatten()
    }

    /// Writes (or with `None` clears) the header's plaintext appearance section, both copies
    /// (two-copy seq protocol). Refuses anything over the caps in [`Appearance::validate`].
    /// A no-op when nothing changes.
    pub fn set_appearance(&self, appearance: Option<&Appearance>) -> std::result::Result<(), AppearanceError> {
        if let Some(a) = appearance {
            a.validate()?;
        }
        let _s = self.slot_lock.lock().unwrap_or_else(|e| e.into_inner());
        self.with(|i| {
            if i.header.appearance.as_ref().map(|a| &a.appearance) == appearance {
                return Ok(());
            }
            let body = i.header.body.clone();
            i.header = write_headers(&i.hfile, &i.master, &body, appearance, i.header.layout, i.header.fixed, i.header.journal, &mut i.copy_seq)?;
            i.record_header_seq()?;
            Ok(())
        })
        .map_err(AppearanceError::Fs)
    }

    /// How many durability points this volume has recorded (`meta nv.commit_seq`).
    /// Only grows while this build writes; an older copy of the file has a smaller number. Kept
    /// OUTSIDE the file (the app's per-volume anchor) it shows that a volume was replaced by an
    /// older copy of itself — a rollback the file alone can never reveal.
    pub fn commit_seq(&self) -> Result<u64> {
        self.with(|i| fresh::commit_seq(&i.conn).map_err(dberr))
    }

    /// The appearance section found at unlock did not authenticate (someone edited it while
    /// locked). It has been dropped; the app should warn and rewrite it from the volume.
    pub fn appearance_tampered(&self) -> bool {
        self.with(|i| Ok(i.appearance_tampered)).unwrap_or(false)
    }

    // ---------------- extras for the app ----------------

    /// Deletes a file or a whole directory tree.
    pub fn remove_recursive(&self, id: NodeId) -> Result<()> {
        if id == ROOT {
            return Err(Error::InvalidName);
        }
        self.with(|i| {
            let parent = i.node(id)?.parent;
            let ids: Vec<NodeId> = {
                let mut st = i
                    .conn
                    .prepare_cached(
                        "WITH RECURSIVE t(id) AS (SELECT ?1 UNION ALL SELECT n.id FROM nodes n JOIN t ON n.parent = t.id WHERE n.id != 1)
                         SELECT id FROM t",
                    )
                    .map_err(dberr)?;
                st.query_map([id as i64], |r| r.get::<_, i64>(0)).map_err(dberr)?.map(|r| r.map(|v| v as u64)).collect::<rusqlite::Result<_>>().map_err(dberr)?
            };
            i.op(|c| {
                let mut del = c.prepare_cached("DELETE FROM nodes WHERE id = ?").map_err(dberr)?;
                // Children first so the parent foreign key never dangles.
                for n in ids.iter().rev() {
                    del.execute([*n as i64]).map_err(dberr)?;
                }
                c.execute("UPDATE nodes SET mtime = ? WHERE id = ?", params![now_ns(), parent as i64]).map_err(dberr)?;
                Ok(())
            })?;
            i.search.touch(ids.iter().copied().chain([parent]));
            for n in ids {
                i.drop_buf(n);
            }
            Ok(())
        })
    }

    /// Runs SQLite's integrity check over the whole store.
    pub fn check_integrity(&self) -> Result<()> {
        self.with(|i| {
            i.spill()?;
            let r: String = i.conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).map_err(dberr)?;
            if r == "ok" { Ok(()) } else { Err(Error::Corrupt(r)) }
        })
    }

    // ---------------- app metadata ----------------

    /// Reads an app-defined value from the encrypted `meta` table (`None` = not set).
    pub fn meta_get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.with(|i| {
            i.conn.query_row("SELECT v FROM meta WHERE k = ?", [key], |r| r.get(0)).optional().map_err(dberr)
        })
    }

    /// Stores (or with `None` removes) an app-defined value in the encrypted `meta` table.
    /// Durable on return. Reserved keys (`volume_id`) and values over [`MAX_META`] are refused.
    pub fn meta_set(&self, key: &str, value: Option<&[u8]>) -> Result<()> {
        if key.is_empty() || key == "volume_id" || value.is_some_and(|v| v.len() > MAX_META) {
            return Err(Error::InvalidName);
        }
        self.with(|i| {
            i.op(|c| {
                match value {
                    Some(v) => c.execute("INSERT OR REPLACE INTO meta(k, v) VALUES (?, ?)", params![key, v]),
                    None => c.execute("DELETE FROM meta WHERE k = ?", [key]),
                }
                .map_err(dberr)
            })?;
            i.durable()
        })
    }
}

// ---------------- peek (no password) ----------------

/// What a locked volume tells without a password: read from the header only.
#[derive(Clone, Debug, Serialize)]
pub struct Peek {
    /// The plaintext appearance section (`None` = absent). Display-only: it cannot be
    /// authenticated before unlock; `Vault::open` checks its MAC.
    pub appearance: Option<Appearance>,
    /// Argon2id parameters of each password slot, in slot order.
    pub password_kdf: Vec<KdfParams>,
    pub recipient_slots: usize,
    /// Every occupied key slot (passwords, recipients, any other kind).
    pub slots: usize,
    /// The volume's random id (plaintext in the header).
    pub volume_id: [u8; 16],
    /// Estimated unlock time with the first password slot on this machine, ms.
    pub unlock_ms: Option<u64>,
}

/// Reads the newest valid header copy of `path` (read-only, no key). Works while the volume
/// is open elsewhere in this process.
pub fn peek(path: &Path) -> std::result::Result<Peek, OpenError> {
    let f = File::open(path).map_err(|e| OpenError::Io(e.to_string()))?;
    let (h, _, _) = read_headers(&f)?;
    let password_kdf: Vec<KdfParams> = h
        .body
        .slots
        .iter()
        .flatten()
        .filter_map(|e| match &e.slot {
            Slot::Password { argon2, .. } => Some(*argon2),
            _ => None,
        })
        .collect();
    let recipient_slots = h.body.slots.iter().flatten().filter(|e| matches!(e.slot, Slot::Recipient { .. })).count();
    Ok(Peek {
        appearance: h.appearance.map(|a| a.appearance),
        unlock_ms: password_kdf.first().map(|k| estimate_unlock_ms(*k)),
        password_kdf,
        recipient_slots,
        slots: h.body.slots.iter().flatten().count(),
        volume_id: h.body.volume_id,
    })
}

/// [`peek`], appearance only.
pub fn peek_appearance(path: &Path) -> std::result::Result<Option<Appearance>, OpenError> {
    Ok(peek(path)?.appearance)
}

/// Rough Argon2id time for `k` on this machine: one 32 MiB, t=1 calibration pass (cached
/// for the process) scaled by memory x passes. The argon2 crate fills lanes on one thread,
/// so `p` does not divide the cost. Good to ~2x; used only to pace the unlock animation.
pub fn estimate_unlock_ms(k: KdfParams) -> u64 {
    static MS_PER_KIB_PASS: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    let rate = *MS_PER_KIB_PASS.get_or_init(|| {
        let cal = KdfParams { m_kib: 32 * 1024, t: 1, p: 1 };
        let t0 = Instant::now();
        let _ = header::argon2_kek(b"calibration", cal, &[0; 32]);
        t0.elapsed().as_secs_f64() * 1000.0 / cal.m_kib as f64
    });
    (rate * k.m_kib as f64 * k.t.max(1) as f64).round().max(1.0) as u64
}

/// Largest value [`Vault::meta_set`] accepts.
pub const MAX_META: usize = 1024 * 1024;

impl Drop for Vault {
    fn drop(&mut self) {
        let _ = self.shutdown(); // best effort: flush + zeroize
    }
}

// ---------------- store ----------------

fn row_node(r: &rusqlite::Row<'_>) -> rusqlite::Result<Node> {
    Ok(Node {
        id: r.get::<_, i64>(0)? as u64,
        parent: r.get::<_, i64>(1)? as u64,
        name: r.get(2)?,
        kind: if r.get::<_, i64>(3)? == 1 { Kind::Dir } else { Kind::File },
        size: r.get::<_, i64>(4)? as u64,
        mtime: r.get(5)?,
    })
}

const NODE_COLS: &str = "id, parent, name, kind, size, mtime";

/// Runs `f` inside a savepoint of `conn` (released on success, rolled back on error).
fn savepoint<T>(conn: &mut Connection, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let sp = conn.savepoint().map_err(dberr)?;
    let v = f(&sp)?;
    sp.commit().map_err(dberr)?;
    Ok(v)
}

fn load_chunk(conn: &Connection, id: NodeId, idx: u64) -> Result<Vec<u8>> {
    let mut st = conn.prepare_cached("SELECT data FROM chunks WHERE node = ? AND idx = ?").map_err(dberr)?;
    let data = st.query_row([id as i64, idx as i64], |r| r.get(0)).optional().map_err(dberr)?.unwrap_or_default();
    probe(conn)?;
    Ok(data)
}

/// A page that fails its HMAC is zero-filled by SQLCipher and handed back as data; only the
/// NEXT statement fails (the pager is then in a permanent error state). So after anything that
/// read pages, one cheap statement: an error here means what was just read cannot be trusted.
/// (The last overflow page of a chunk otherwise read back as zeros, silently.)
fn probe(conn: &Connection) -> Result<()> {
    let mut st = conn.prepare_cached("SELECT 1 FROM meta WHERE k = 'volume_id'").map_err(corrupt_page)?;
    st.query_row([], |_| Ok(())).map_err(corrupt_page)
}

fn corrupt_page(e: rusqlite::Error) -> Error {
    Error::Corrupt(format!("volume damaged or tampered with (page authentication failed): {e}"))
}

impl Inner {
    fn overlay(&self, mut n: Node) -> Node {
        if let Some(b) = self.bufs.get(&n.id) {
            n.size = b.size;
            n.mtime = b.mtime;
        }
        n
    }

    fn node(&self, id: NodeId) -> Result<Node> {
        let mut st = self.conn.prepare_cached(&format!("SELECT {NODE_COLS} FROM nodes WHERE id = ?")).map_err(dberr)?;
        let n = st.query_row([id as i64], row_node).map_err(dberr)?;
        Ok(self.overlay(n))
    }

    fn dir(&self, id: NodeId) -> Result<Node> {
        let n = self.node(id)?;
        if n.kind != Kind::Dir {
            return Err(Error::NotDir);
        }
        Ok(n)
    }

    fn child(&self, parent: NodeId, name: &str) -> Result<Option<Node>> {
        let mut st =
            self.conn.prepare_cached(&format!("SELECT {NODE_COLS} FROM nodes WHERE parent = ? AND name = ? AND id != 1")).map_err(dberr)?;
        Ok(st.query_row(params![parent as i64, name], row_node).optional().map_err(dberr)?.map(|n| self.overlay(n)))
    }

    fn has_children(&self, id: NodeId) -> Result<bool> {
        let mut st = self.conn.prepare_cached("SELECT EXISTS(SELECT 1 FROM nodes WHERE parent = ? AND id != 1)").map_err(dberr)?;
        st.query_row([id as i64], |r| r.get(0)).map_err(dberr)
    }

    /// The write buffer for file `id`, creating it from the stored node if needed.
    fn buf(&mut self, id: NodeId) -> Result<&mut FileBuf> {
        if !self.bufs.contains_key(&id) {
            let n = self.node(id)?;
            if n.kind == Kind::Dir {
                return Err(Error::IsDir);
            }
            self.bufs.insert(id, FileBuf { size: n.size, mtime: n.mtime, chunks: BTreeMap::new(), trunc_from: None });
        }
        self.touch();
        Ok(self.bufs.get_mut(&id).unwrap())
    }

    fn touch(&mut self) {
        self.dirty_since.get_or_insert_with(Instant::now);
    }

    /// Opens the group-commit transaction if none is open.
    fn begin(&mut self) -> Result<()> {
        if !self.in_tx {
            self.conn.execute_batch("BEGIN IMMEDIATE").map_err(dberr)?;
            self.in_tx = true;
        }
        self.touch();
        Ok(())
    }

    /// Runs `f` as one atomic step of the open transaction (a savepoint: a failing step leaves
    /// no trace). It becomes durable at the next durability point, together with the rest.
    fn op<T>(&mut self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        self.begin()?;
        let r = savepoint(&mut self.conn, f);
        if r.is_err() {
            self.check_tx();
        }
        r
    }

    /// After a failed statement: on disk full / I/O errors SQLite may roll the whole
    /// transaction back by itself — everything since the last durability point is then gone,
    /// as after a crash.
    fn check_tx(&mut self) {
        if self.in_tx && self.conn.is_autocommit() {
            self.in_tx = false;
            self.tx_bytes = 0;
            self.search.reset();
        }
    }

    fn drop_buf(&mut self, id: NodeId) {
        if let Some(b) = self.bufs.remove(&id) {
            self.buffered = self.buffered.saturating_sub(b.chunks.values().map(Vec::len).sum());
        }
    }

    /// Moves all buffered file data into the open transaction (atomic per call; durable at
    /// the next durability point).
    fn spill(&mut self) -> Result<()> {
        if self.bufs.is_empty() {
            return Ok(());
        }
        self.begin()?;
        let Inner { conn, bufs, .. } = self;
        let r = savepoint(conn, |c| {
            let mut del = c.prepare_cached("DELETE FROM chunks WHERE node = ? AND idx >= ?").map_err(dberr)?;
            let mut put = c.prepare_cached("INSERT OR REPLACE INTO chunks(node, idx, data) VALUES (?, ?, ?)").map_err(dberr)?;
            let mut meta = c.prepare_cached("UPDATE nodes SET size = ?, mtime = ? WHERE id = ?").map_err(dberr)?;
            for (id, b) in bufs.iter() {
                // 0 rows: the node went with a transaction SQLite rolled back (see check_tx).
                if meta.execute(params![b.size as i64, b.mtime, *id as i64]).map_err(dberr)? == 0 {
                    continue;
                }
                if let Some(t) = b.trunc_from {
                    del.execute([*id as i64, t as i64]).map_err(dberr)?;
                }
                for (idx, data) in &b.chunks {
                    put.execute(params![*id as i64, *idx as i64, data]).map_err(dberr)?;
                }
            }
            Ok(())
        });
        // the pages to the OS now (not at the COMMIT, under the lock, all at once): the writer
        // writes them back outside the lock (Writeback)
        let r = r.and_then(|_| {
            self.conn.cache_flush().map_err(|e| {
                // a failed write-out (disk full, I/O) leaves the pager in its error state, which
                // only a rollback ends (a failing statement would make SQLite roll back by itself):
                // the same loss as SQLite aborting the transaction, see check_tx
                if !self.conn.is_autocommit() {
                    let _ = self.conn.execute_batch("ROLLBACK");
                }
                dberr(e)
            })
        });
        if let Err(e) = r {
            self.check_tx();
            return Err(e);
        }
        self.wb.account();
        self.tx_bytes += self.buffered as u64;
        self.search.touch(self.bufs.keys().copied());
        self.bufs.clear();
        self.buffered = 0;
        Ok(())
    }

    /// Durability point: everything done so far survives a crash or power loss once this
    /// returns (one COMMIT; with the WAL also a checkpoint).
    fn durable(&mut self) -> Result<()> {
        self.spill()?;
        if self.in_tx {
            // one more durability point (what a host-side anchor compares against)
            if let Err(e) = fresh::bump_commit(&self.conn).and_then(|_| self.conn.execute_batch("COMMIT")) {
                self.check_tx();
                return Err(dberr(e));
            }
            self.in_tx = false;
            self.tx_bytes = 0;
            self.wb.clear(); // synced
        }
        if self.journal == Journal::Wal {
            self.conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(())).map_err(dberr)?;
        }
        self.dirty_since = None;
        Ok(())
    }

    /// The journal region [`Vault::upgrade_format`] gives this volume (`want`, or less for a small
    /// fixed-size one); None: a fixed-size volume without room for one.
    fn upgrade_region(&self, want: u64) -> Result<Option<u64>> {
        let Some(f) = self.header.fixed else { return Ok(Some(want)) };
        let page = self.header.layout.unwrap_or(Layout::LEGACY).page_size;
        let pages: i64 = self.conn.query_row("PRAGMA page_count", [], |r| r.get(0)).map_err(dberr)?;
        // (at most what a new volume of this size gets)
        Ok(single::region_to_upgrade(f.capacity, pages.max(0) as u64 * page as u64, page, want.min(single::region_for(f.capacity))))
    }

    /// Largest database a fixed-size volume holds (None: a growable one).
    fn db_room(&self) -> Option<u64> {
        let f = self.header.fixed?;
        Some(match self.header.journal {
            Some(j) => single::Map::new(j, Some(f)).db_room(f.capacity),
            None => vfs::fixed_db_capacity(f.capacity),
        })
    }

    /// After a header write: the database learns the new seq (durable on return).
    fn record_header_seq(&mut self) -> Result<()> {
        let seq = self.header.body.seq;
        let h = self.header.clone();
        self.op(|c| set_db_header_seq(c, seq).and_then(|_| fresh::record_appearance(c, "main", &h)).map_err(dberr))?;
        self.durable()
    }

    fn set_size(&mut self, id: NodeId, size: u64) -> Result<()> {
        let i = self;
        i.buf(id)?;
        let Inner { conn, bufs, buffered, chunk, .. } = i;
        let b = bufs.get_mut(&id).unwrap();
        if size < b.size {
            let c = *chunk as u64;
            let first_gone = size.div_ceil(c);
            let old_trunc = b.trunc_from;
            let dropped = b.chunks.split_off(&first_gone);
            *buffered = buffered.saturating_sub(dropped.values().map(Vec::len).sum());
            b.trunc_from = Some(b.trunc_from.map_or(first_gone, |t| t.min(first_gone)));
            let keep = (size % c) as usize;
            if keep != 0 {
                let k = size / c;
                if !b.chunks.contains_key(&k) && !old_trunc.is_some_and(|t| k >= t) {
                    let d = load_chunk(conn, id, k)?;
                    if d.len() > keep {
                        b.chunks.insert(k, d);
                    }
                }
                if let Some(d) = b.chunks.get_mut(&k) {
                    d.truncate(keep);
                }
            }
        }
        b.size = size;
        b.mtime = now_ns();
        Ok(())
    }

    fn remove(&mut self, id: NodeId) -> Result<()> {
        if id == ROOT {
            return Err(Error::InvalidName);
        }
        let i = self;
        let n = i.node(id)?;
        if n.kind == Kind::Dir && i.has_children(id)? {
            return Err(Error::NotEmpty);
        }
        i.op(|c| {
            c.execute("DELETE FROM nodes WHERE id = ?", [id as i64]).map_err(dberr)?;
            c.execute("UPDATE nodes SET mtime = ? WHERE id = ?", params![now_ns(), n.parent as i64]).map_err(dberr)?;
            Ok(())
        })?;
        i.search.touch([id, n.parent]);
        i.drop_buf(id);
        Ok(())
    }

    /// After buffering file data: whether a spill is due ([`MAX_BUFFERED`]), which the caller
    /// makes once the device caught up, see [`WRITEBACK_AT`].
    fn spill_due(&self) -> bool {
        self.buffered >= MAX_BUFFERED
    }

    /// After a spill: whether a durability point is due ([`DURABLE_BYTES`]), which the caller
    /// makes without the lock held.
    fn commit_due(&self) -> bool {
        self.tx_bytes >= DURABLE_BYTES
    }
}

impl Fs for Vault {
    fn getattr(&self, id: NodeId) -> Result<Node> {
        self.with(|i| i.node(id))
    }

    fn lookup(&self, parent: NodeId, name: &str) -> Result<Node> {
        self.with(|i| {
            i.dir(parent)?;
            i.child(parent, name)?.ok_or(Error::NotFound)
        })
    }

    fn readdir(&self, dir: NodeId) -> Result<Vec<Node>> {
        self.with(|i| {
            i.dir(dir)?;
            let mut st = i
                .conn
                .prepare_cached(&format!("SELECT {NODE_COLS} FROM nodes WHERE parent = ? AND id != 1 ORDER BY name"))
                .map_err(dberr)?;
            let v = st.query_map([dir as i64], row_node).map_err(dberr)?.collect::<rusqlite::Result<Vec<_>>>().map_err(dberr)?;
            Ok(v.into_iter().map(|n| i.overlay(n)).collect())
        })
    }

    fn create(&self, parent: NodeId, name: &str, kind: Kind) -> Result<Node> {
        if !nv_fs::valid_name(name) {
            return Err(Error::InvalidName);
        }
        self.with(|i| {
            i.dir(parent)?;
            let now = now_ns();
            let k = if kind == Kind::Dir { 1 } else { 0 };
            let id = i.op(|c| {
                c.execute("INSERT INTO nodes(parent, name, kind, size, mtime) VALUES (?, ?, ?, 0, ?)", params![parent as i64, name, k, now])
                    .map_err(dberr)?;
                let id = c.last_insert_rowid() as u64;
                c.execute("UPDATE nodes SET mtime = ? WHERE id = ?", params![now, parent as i64]).map_err(dberr)?;
                Ok(id)
            })?;
            i.search.touch([id, parent]);
            Ok(Node { id, parent, name: name.to_owned(), kind, size: 0, mtime: now })
        })
    }

    fn read(&self, id: NodeId, offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.with(|i| {
            let n = i.node(id)?;
            if n.kind == Kind::Dir {
                return Err(Error::IsDir);
            }
            if offset >= n.size || buf.is_empty() {
                return Ok(0);
            }
            let len = (buf.len() as u64).min(n.size - offset) as usize;
            let chunk = i.chunk;
            let fb = i.bufs.get(&id);
            let mut done = 0;
            while done < len {
                let pos = offset + done as u64;
                let idx = pos / chunk as u64;
                let within = (pos % chunk as u64) as usize;
                let take = (chunk - within).min(len - done);
                let out = &mut buf[done..done + take];
                let stored;
                let data: &[u8] = match fb.and_then(|b| b.chunks.get(&idx)) {
                    Some(d) => d,
                    None if fb.and_then(|b| b.trunc_from).is_some_and(|t| idx >= t) => &[],
                    None => {
                        stored = load_chunk(&i.conn, id, idx)?;
                        &stored
                    }
                };
                let avail = data.len().saturating_sub(within).min(take);
                if avail > 0 {
                    out[..avail].copy_from_slice(&data[within..within + avail]);
                }
                out[avail..].fill(0); // sparse / short chunk
                done += take;
            }
            Ok(len)
        })
    }

    fn write(&self, id: NodeId, offset: u64, data: &[u8]) -> Result<usize> {
        if data.is_empty() {
            return self.with(|i| i.node(id).and_then(|n| if n.kind == Kind::Dir { Err(Error::IsDir) } else { Ok(0) }));
        }
        let end = offset.checked_add(data.len() as u64).ok_or(Error::Io("offset overflow".into()))?;
        self.shared.yield_to_commit();
        let (due, wb) = self.with(|i| {
            i.buf(id)?;
            let Inner { conn, bufs, buffered, chunk, .. } = i;
            let chunk = *chunk;
            let b = bufs.get_mut(&id).unwrap();
            let mut done = 0;
            while done < data.len() {
                let pos = offset + done as u64;
                let idx = pos / chunk as u64;
                let within = (pos % chunk as u64) as usize;
                let take = (chunk - within).min(data.len() - done);
                let chunk_start = idx * chunk as u64;
                if !b.chunks.contains_key(&idx) {
                    let whole = within == 0 && take == chunk;
                    let gone = chunk_start >= b.size || b.trunc_from.is_some_and(|t| idx >= t);
                    let existing = if whole || gone { Vec::new() } else { load_chunk(conn, id, idx)? };
                    *buffered += existing.len();
                    b.chunks.insert(idx, existing);
                }
                let c = b.chunks.get_mut(&idx).unwrap();
                if c.len() < within + take {
                    *buffered += within + take - c.len();
                    c.resize(within + take, 0);
                }
                c[within..within + take].copy_from_slice(&data[done..done + take]);
                done += take;
            }
            b.size = b.size.max(end);
            b.mtime = now_ns();
            Ok((i.spill_due(), i.wb.clone()))
        })?;
        // the slow part of writing, without the lock: the device catches up with this writer
        if due {
            wb.wait_below(WRITEBACK_AT);
            if self.with(|i| if i.spill_due() { i.spill().map(|_| i.commit_due()) } else { Ok(false) })? {
                self.shared.durable_point(false)?;
            }
        }
        Ok(data.len())
    }

    fn set_size(&self, id: NodeId, size: u64) -> Result<()> {
        self.with(|i| i.set_size(id, size))
    }

    fn set_mtime(&self, id: NodeId, mtime: i64) -> Result<()> {
        self.with(|i| {
            if let Some(b) = i.bufs.get_mut(&id) {
                b.mtime = mtime;
                return Ok(());
            }
            match i.op(|c| c.execute("UPDATE nodes SET mtime = ? WHERE id = ?", params![mtime, id as i64]).map_err(dberr))? {
                0 => Err(Error::NotFound),
                _ => {
                    i.search.touch([id]);
                    Ok(())
                }
            }
        })
    }

    fn rename(&self, id: NodeId, new_parent: NodeId, new_name: &str) -> Result<()> {
        if !nv_fs::valid_name(new_name) || id == ROOT {
            return Err(Error::InvalidName);
        }
        self.with(|i| {
            let src = i.node(id)?;
            i.dir(new_parent)?;
            if src.kind == Kind::Dir {
                // Refuse moving a directory into itself or its own subtree.
                let mut p = new_parent;
                loop {
                    if p == id {
                        return Err(Error::InvalidName);
                    }
                    if p == ROOT {
                        break;
                    }
                    p = i.node(p)?.parent;
                }
            }
            let target = i.child(new_parent, new_name)?;
            if let Some(t) = &target {
                if t.id == id {
                    return Ok(());
                }
                match (src.kind, t.kind) {
                    (Kind::File, Kind::Dir) => return Err(Error::IsDir),
                    (Kind::Dir, Kind::File) => return Err(Error::NotDir),
                    (Kind::Dir, Kind::Dir) if i.has_children(t.id)? => return Err(Error::Exists),
                    _ => {}
                }
            }
            let now = now_ns();
            i.op(|c| {
                if let Some(t) = &target {
                    c.execute("DELETE FROM nodes WHERE id = ?", [t.id as i64]).map_err(dberr)?;
                }
                c.execute("UPDATE nodes SET parent = ?, name = ? WHERE id = ?", params![new_parent as i64, new_name, id as i64])
                    .map_err(dberr)?;
                c.execute("UPDATE nodes SET mtime = ? WHERE id IN (?, ?)", params![now, src.parent as i64, new_parent as i64])
                    .map_err(dberr)?;
                Ok(())
            })?;
            i.search.touch([id, src.parent, new_parent].into_iter().chain(target.as_ref().map(|t| t.id)));
            if let Some(t) = target {
                i.drop_buf(t.id);
            }
            Ok(())
        })
    }

    fn remove(&self, id: NodeId) -> Result<()> {
        self.with(|i| i.remove(id))
    }

    /// Durability point: everything written so far survives a crash once this returns.
    fn flush(&self) -> Result<()> {
        self.shared.durable_point(false)
    }

    fn stats(&self) -> Result<Stats> {
        let (used_bytes, files) = self.with(|i| {
            let (mut used, files): (i64, i64) = i
                .conn
                .query_row("SELECT COALESCE(SUM(size), 0), COUNT(*) FROM nodes WHERE kind = 0", [], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(dberr)?;
            for (id, b) in &i.bufs {
                let stored: i64 = i.conn.query_row("SELECT size FROM nodes WHERE id = ?", [*id as i64], |r| r.get(0)).map_err(dberr)?;
                used += b.size as i64 - stored;
            }
            Ok((used.max(0) as u64, files as u64))
        })?;
        if let Some(cap) = self.capacity() {
            // A fixed volume: its own capacity, not the drive's. Free = never-used room left in
            // the file + pages the store freed (reused before the store grows again).
            let (pages, freelist, page) = self.with(|i| {
                let q = |p: &str| i.conn.query_row(p, [], |r| r.get::<_, i64>(0)).map_err(dberr);
                // (SQLCipher answers page_size as text: the stored layout is the page size)
                let page = i.header.layout.unwrap_or(Layout::LEGACY).page_size as i64;
                Ok((q("PRAGMA page_count")?, q("PRAGMA freelist_count")?, page))
            })?;
            let room = self.with(|i| Ok(i.db_room()))?.unwrap_or(0).saturating_sub((pages * page).max(0) as u64);
            return Ok(Stats { total_bytes: cap, free_bytes: room + (freelist * page).max(0) as u64, used_bytes, files });
        }
        let dir = self.path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        Ok(Stats {
            total_bytes: fs4::total_space(dir).map_err(ioerr)?,
            free_bytes: fs4::available_space(dir).map_err(ioerr)?,
            used_bytes,
            files,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_is_send_sync() {
        fn assert<T: Send + Sync>() {}
        assert::<super::Vault>();
    }

    /// The cipher settings in force are exactly the pinned ones (and the bundled SQLCipher is a
    /// 4.x); the compat fixtures prove they are what existing volumes were written with.
    #[test]
    fn sqlcipher_settings_are_pinned() {
        let t = tempfile::tempdir().unwrap();
        let v = Vault::create(&t.path().join("p.nvault"), &SecretString::from("pw".to_owned()), KdfPreset::Fast).unwrap();
        let got = v
            .with(|i| {
                let q = |p: &str| i.conn.query_row(&format!("PRAGMA {p}"), [], |r| r.get::<_, String>(0)).map_err(dberr);
                Ok([
                    q("cipher_version")?,
                    q("cipher_page_size")?,
                    q("cipher_hmac_algorithm")?,
                    q("cipher_kdf_algorithm")?,
                    q("kdf_iter")?,
                    q("cipher_plaintext_header_size")?,
                ])
            })
            .unwrap();
        assert!(got[0].starts_with("4."), "SQLCipher {}", got[0]);
        assert_eq!(&got[1..], ["16384", "HMAC_SHA256", "PBKDF2_HMAC_SHA512", "256000", "0"]);
        v.close().unwrap();
    }

    /// The password-manager migration only runs once the database is known to be this
    /// header's: a database with another volume_id is refused and left byte-identical.
    #[test]
    fn no_migration_on_a_foreign_database() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("f.nvault");
        let v = Vault::create(&p, &SecretString::from("pw".to_owned()), KdfPreset::Fast).unwrap();
        v.with(|i| {
            i.conn
                .execute_batch(
                    "DROP TABLE pm_history; DROP TABLE pm_entries; DELETE FROM meta WHERE k = 'nv.pm_schema';
                     UPDATE meta SET v = x'00112233445566778899aabbccddeeff' WHERE k = 'volume_id';",
                )
                .map_err(dberr)
        })
        .unwrap();
        v.close().unwrap();
        let before = std::fs::read(&p).unwrap();
        let r = Vault::open(&p, &SecretString::from("pw".to_owned()));
        assert!(matches!(r, Err(OpenError::Corrupt(ref m)) if m.contains("does not belong")), "{:?}", r.err());
        assert!(std::fs::read(&p).unwrap() == before, "a foreign database was written to");
    }

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

    /// (header copies' (version, layout), SQLite page size, largest stored chunk).
    fn on_disk(path: &Path, v: &Vault) -> ([(u16, Option<Layout>); 2], i64, i64) {
        let raw = std::fs::read(path).unwrap();
        let h = |i: usize| {
            let h = header::decode_copy(&raw[i * COPY_LEN..(i + 1) * COPY_LEN]).ok().unwrap();
            (h.body.version, h.layout)
        };
        let (page, chunk) = v
            .with(|i| {
                i.spill()?;
                // SQLCipher answers page_size with cipher_page_size (as text); the store only
                // decrypts (per-page HMAC) with the right one, so this is the real page size.
                let page: String = i.conn.query_row("PRAGMA page_size", [], |r| r.get(0)).map_err(dberr)?;
                let page: i64 = page.parse().unwrap();
                let chunk = i.conn.query_row("SELECT COALESCE(MAX(length(data)), 0) FROM chunks", [], |r| r.get(0)).map_err(dberr)?;
                Ok((page, chunk))
            })
            .unwrap();
        ([h(0), h(1)], page, chunk)
    }

    /// A volume made with the constants of the builds before stored layouts (format v1,
    /// 16 KiB pages, 256 KiB chunks, WAL) opens, reads and writes unchanged with this build,
    /// and is never re-laid-out: its header stays v1 and its store keeps 16 KiB / 256 KiB.
    #[test]
    fn old_default_volume_opens_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.nvault");
        let a = pat(1, 3 * 262_144 + 1234);
        let v = Vault::create_with(&path, &pw(), KdfPreset::Fast, None, None, Journal::Wal).unwrap();
        let d = v.create(ROOT, "docs", Kind::Dir).unwrap();
        let f = v.create(d.id, "a.bin", Kind::File).unwrap();
        v.write(f.id, 0, &a).unwrap();
        v.flush().unwrap();
        let legacy = ([(VERSION, None), (VERSION, None)], 16384, 262_144);
        assert_eq!(on_disk(&path, &v), legacy);
        v.close().unwrap();

        let v = Vault::open(&path, &pw()).unwrap();
        assert_eq!(read_file(&v, f.id), a);
        // Writes with this build (group commit, rollback journal) keep the old layout.
        let b = pat(2, 2_000_000);
        let g = v.create(ROOT, "b.bin", Kind::File).unwrap();
        v.write(g.id, 0, &b).unwrap();
        v.write(f.id, 100_000, b"patched").unwrap();
        v.add_password(&SecretString::from("second".to_owned()), KdfPreset::Fast, Some("x")).unwrap();
        assert_eq!(on_disk(&path, &v), legacy);
        v.close().unwrap();
        let v = Vault::open(&path, &SecretString::from("second".to_owned())).unwrap();
        let mut want = a.clone();
        want[100_000..100_007].copy_from_slice(b"patched");
        assert_eq!(read_file(&v, f.id), want);
        assert_eq!(read_file(&v, g.id), b);
        v.check_integrity().unwrap();
        assert_eq!(on_disk(&path, &v), legacy);
        v.close().unwrap();
        for s in ["-wal", "-shm", "-journal"] {
            assert!(!dir.path().join(format!("old.nvault{s}")).exists(), "{s} left behind");
        }
    }

    /// New volumes store their layout (v2 header) and use it.
    #[test]
    fn new_volume_stores_its_layout() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.nvault");
        let l = Layout { page_size: 65536, chunk_size: 1 << 20 };
        let v = Vault::create_with(&path, &pw(), KdfPreset::Fast, Some(l), None, Journal::Persist).unwrap();
        let f = v.create(ROOT, "a", Kind::File).unwrap();
        let a = pat(3, 2 * (1 << 20) + 77);
        v.write(f.id, 0, &a).unwrap();
        assert_eq!(on_disk(&path, &v), ([(header::VERSION_LAYOUT, Some(l)); 2], 65536, 1 << 20));
        v.close().unwrap();
        let v = Vault::open(&path, &pw()).unwrap();
        assert_eq!(read_file(&v, f.id), a);
        assert_eq!(on_disk(&path, &v).1, 65536);
        v.close().unwrap();
        // The legacy layout itself is written as v1, so older builds can still open it.
        let p2 = dir.path().join("legacy-values.nvault");
        let v = Vault::create_with(&p2, &pw(), KdfPreset::Fast, Some(Layout::LEGACY), None, Journal::Persist).unwrap();
        assert_eq!(on_disk(&p2, &v).0, [(VERSION, None); 2]);
        v.close().unwrap();
    }

    /// Child of `old_build_crash_leftovers_are_recovered`: leaves committed data only in the WAL.
    #[test]
    fn legacy_wal_child() {
        let Ok(path) = std::env::var("NV_LEGACY_CHILD") else { return };
        let v = Vault::create_with(Path::new(&path), &pw(), KdfPreset::Fast, None, None, Journal::Wal).unwrap();
        let f = v.create(ROOT, "in-wal", Kind::File).unwrap();
        v.write(f.id, 0, &pat(4, 3_000_000)).unwrap(); // < wal_autocheckpoint (1000 pages)
        v.with(|i| {
            i.spill()?;
            i.conn.execute_batch("COMMIT").map_err(dberr) // committed, not checkpointed
        })
        .unwrap();
        std::process::exit(134); // simulated crash: no core dump, so no DrKonqi/abrt popup on the desktop
    }

    /// An old build that crashed leaves a WAL with committed transactions: this build replays
    /// it, then switches the volume to its own journal mode.
    #[test]
    fn old_build_crash_leftovers_are_recovered() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crashed.nvault");
        let st = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["tests::legacy_wal_child", "--exact", "--nocapture", "--test-threads=1"])
            .env("NV_LEGACY_CHILD", &path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(!st.success());
        let wal = dir.path().join("crashed.nvault-wal");
        assert!(std::fs::metadata(&wal).unwrap().len() > 3_000_000, "the data is only in the WAL");
        let v = Vault::open(&path, &pw()).unwrap();
        let n = v.lookup(ROOT, "in-wal").unwrap();
        assert_eq!(read_file(&v, n.id), pat(4, 3_000_000));
        v.check_integrity().unwrap();
        assert!(!wal.exists(), "WAL folded in and removed");
        v.close().unwrap();
    }

    const FIXED_CAP: u64 = 24 << 20;

    /// A fixed-size volume (single-file, and with the side-file journal of earlier builds):
    /// preallocated to its capacity, never grows or shrinks, refuses writes past it, keeps
    /// working after reopen, key rotation and deletes.
    #[test]
    fn fixed_volume_keeps_its_size() {
        for single in [true, false] {
            fixed_volume_keeps_its_size_as(single);
        }
    }

    fn fixed_volume_keeps_its_size_as(single: bool) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixed.nvault");
        let v = Vault::create_fixed_as(&path, &pw(), KdfPreset::Fast, FIXED_CAP + 123, single).unwrap();
        let len = || std::fs::metadata(&path).unwrap().len();
        assert_eq!(len(), FIXED_CAP, "rounded down to 4 KiB");
        assert_eq!(v.capacity(), Some(FIXED_CAP));
        assert_eq!(v.is_single_file(), single);
        let raw = std::fs::read(&path).unwrap();
        let h = header::decode_copy(&raw[..COPY_LEN]).ok().unwrap();
        assert_eq!((h.body.version, h.layout, h.fixed), (header::VERSION_LAYOUT, Some(Layout::LEGACY), Some(Fixed { capacity: FIXED_CAP })));
        let region = single::region_for(FIXED_CAP);
        assert_eq!(h.journal, single.then(|| JournalRegion::fresh_fixed(region)));
        let room = if single { FIXED_CAP - header::STORE_AT - region } else { vfs::fixed_db_capacity(FIXED_CAP) };
        let st = v.stats().unwrap();
        assert_eq!(st.total_bytes, FIXED_CAP);
        assert!(st.free_bytes > room - (1 << 20) && st.free_bytes <= room, "{} of {room}", st.free_bytes);
        let a = pat(5, 6 << 20);
        let f = v.create(ROOT, "a.bin", Kind::File).unwrap();
        v.write(f.id, 0, &a).unwrap();
        v.flush().unwrap();
        assert_eq!(len(), FIXED_CAP);
        let free_after = v.stats().unwrap().free_bytes;
        assert!(free_after < st.free_bytes - (5 << 20));
        v.close().unwrap();
        assert_eq!(len(), FIXED_CAP);

        let v = Vault::open(&path, &pw()).unwrap();
        assert_eq!(read_file(&v, f.id), a);
        // More than fits: refused as disk full, the file stays as it was.
        let g = v.create(ROOT, "big.bin", Kind::File).unwrap();
        let big = pat(6, 1 << 20);
        let mut err = None;
        for k in 0..40u64 {
            if let Err(e) = v.write(g.id, k << 20, &big).and_then(|_| v.flush()) {
                err = Some(e);
                break;
            }
        }
        assert!(err.is_some(), "a 40 MiB file does not fit 24 MiB");
        assert_eq!(len(), FIXED_CAP);
        let _ = v.remove(g.id);
        let _ = v.flush();
        v.close().unwrap();

        let v = Vault::open(&path, &pw()).unwrap();
        assert_eq!(read_file(&v, f.id), a);
        v.check_integrity().unwrap();
        // Deleting frees room inside the file (reused), the file keeps its size.
        v.remove(f.id).unwrap();
        v.flush().unwrap();
        assert!(v.stats().unwrap().free_bytes > free_after + (5 << 20));
        let f2 = v.create(ROOT, "again.bin", Kind::File).unwrap();
        v.write(f2.id, 0, &a).unwrap();
        v.flush().unwrap();
        assert_eq!(len(), FIXED_CAP);
        v.rotate_master(0, &pw()).unwrap();
        assert_eq!(len(), FIXED_CAP, "the rotated copy is fixed too");
        assert_eq!(v.capacity(), Some(FIXED_CAP));
        v.close().unwrap();
        let v = Vault::open(&path, &pw()).unwrap();
        assert_eq!(read_file(&v, v.lookup(ROOT, "again.bin").unwrap().id), a);
        v.check_integrity().unwrap();
        v.close().unwrap();
        assert!(Vault::create_fixed_as(&dir.path().join("tiny.nvault"), &pw(), KdfPreset::Fast, 1 << 20, single).is_err());
    }

    /// A fixed volume whose file was cut (or whose footer / journal records name another size,
    /// or are gone) is not opened: SQLite must never write past (or trust) a capacity that is not
    /// there. A longer file is a resize that stopped between its steps (resize.rs): the bytes past
    /// the capacity are cut at the open.
    #[test]
    fn fixed_volume_refuses_a_changed_file() {
        for single in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("fixed.nvault");
            let v = Vault::create_fixed_as(&path, &pw(), KdfPreset::Fast, FIXED_CAP, single).unwrap();
            let g = v.create(ROOT, "g", Kind::File).unwrap();
            v.write(g.id, 0, &pat(3, 3 << 20)).unwrap(); // more than the region displaced
            v.close().unwrap();
            let f = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
            f.set_len(FIXED_CAP - 4096).unwrap();
            assert!(matches!(Vault::open(&path, &pw()), Err(OpenError::Corrupt(_))), "{single}");
            f.set_len(FIXED_CAP + 4096).unwrap();
            Vault::open(&path, &pw()).unwrap().close().unwrap();
            assert_eq!(std::fs::metadata(&path).unwrap().len(), FIXED_CAP);
            if single {
                // records of another capacity only: refused (a resize writes the new one next to the current)
                let mut rec = single::read_record(&f).unwrap();
                rec.cap = FIXED_CAP * 2;
                for slot in 0..2 {
                    single::write_record_at(&f, slot, &rec).unwrap();
                }
                assert!(matches!(Vault::open(&path, &pw()), Err(OpenError::Corrupt(_))), "{single}");
            }
            write_at(&f, vfs::FOOTER_AT as u64, &[0u8; 8192]).unwrap();
            assert!(matches!(Vault::open(&path, &pw()), Err(OpenError::Corrupt(_))), "{single}");
        }
    }

    /// Child of `fixed_volume_crash_rolls_back`: committed data, then a big uncommitted
    /// transaction written into the file, then a crash.
    #[test]
    fn fixed_crash_child() {
        let Ok(path) = std::env::var("NV_FIXED_CHILD") else { return };
        let single = std::env::var("NV_FIXED_SINGLE").is_ok_and(|v| v == "1");
        let v = Vault::create_fixed_as(Path::new(&path), &pw(), KdfPreset::Fast, FIXED_CAP, single).unwrap();
        let f = v.create(ROOT, "kept", Kind::File).unwrap();
        v.write(f.id, 0, &pat(7, 2 << 20)).unwrap();
        v.flush().unwrap();
        let g = v.create(ROOT, "lost", Kind::File).unwrap();
        v.write(g.id, 0, &pat(8, 8 << 20)).unwrap();
        v.with(|i| i.spill().map(|_| ())).unwrap(); // pages in the file, transaction open
        std::process::exit(134); // simulated crash: no core dump, so no DrKonqi/abrt popup on the desktop
    }

    #[test]
    fn fixed_volume_crash_rolls_back() {
        for single in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("crashed.nvault");
            let st = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["tests::fixed_crash_child", "--exact", "--nocapture", "--test-threads=1"])
                .env("NV_FIXED_CHILD", &path)
                .env("NV_FIXED_SINGLE", if single { "1" } else { "0" })
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(!st.success());
            assert_eq!(std::fs::metadata(&path).unwrap().len(), FIXED_CAP);
            let v = Vault::open(&path, &pw()).unwrap();
            assert_eq!(read_file(&v, v.lookup(ROOT, "kept").unwrap().id), pat(7, 2 << 20));
            assert!(v.lookup(ROOT, "lost").is_err());
            v.check_integrity().unwrap();
            let free = v.stats().unwrap().free_bytes;
            let overhead = if single { single::region_for(FIXED_CAP) } else { 0 };
            assert!(free > FIXED_CAP - overhead - (4 << 20), "the rolled-back pages are free again: {free}");
            v.close().unwrap();
            assert_eq!(std::fs::metadata(&path).unwrap().len(), FIXED_CAP);
            let side: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).collect();
            assert_eq!(side.len(), 1, "{side:?}");
        }
    }

    /// `discard` turns secure delete down for its own statements only (cancel-hang).
    #[test]
    fn discard_restores_secure_delete() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::create(&dir.path().join("sd.nvault"), &pw(), KdfPreset::Fast).unwrap();
        let mode = |v: &Vault| v.with(|i| i.conn.query_row("PRAGMA secure_delete", [], |r| r.get::<_, i64>(0)).map_err(dberr)).unwrap();
        assert_eq!(mode(&v), 1, "SQLCipher's default");
        let f = v.create(ROOT, "f", Kind::File).unwrap();
        v.write(f.id, 0, &vec![1u8; (DISCARD_STEP + (3 << 20)) as usize]).unwrap();
        v.discard(f.id).unwrap();
        assert_eq!(mode(&v), 1);
        // also after a step failed (the volume locked under it)
        let g = v.create(ROOT, "g", Kind::File).unwrap();
        v.write(g.id, 0, &[2u8; 1000]).unwrap();
        let d = v.create(ROOT, "d", Kind::Dir).unwrap();
        assert!(v.discard(d.id).is_err());
        assert_eq!(mode(&v), 1);
    }
}
