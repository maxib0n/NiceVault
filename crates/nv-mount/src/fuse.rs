use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use fuser::{
    Config, Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, Generation, INodeNo, KernelConfig,
    LockOwner, MountOption, OpenAccMode, OpenFlags, RenameFlags, ReplyAttr, ReplyCreate, ReplyData,
    ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyStatfs, ReplyWrite, Request, SessionACL, TimeOrNow,
    WriteFlags,
};
use nv_fs::{Error, Fs, Kind, Node};

use crate::util::{self, MountDir};
use crate::{MountError, MountOpts, Result, Unmounter};

const TTL: Duration = Duration::from_secs(1);
const BLOCK: u32 = 4096;
const IO_SIZE: u32 = 1 << 20;

fn errno(e: Error) -> Errno {
    match e {
        Error::NotFound => Errno::ENOENT,
        Error::Exists => Errno::EEXIST,
        Error::NotEmpty => Errno::ENOTEMPTY,
        Error::NotDir => Errno::ENOTDIR,
        Error::IsDir => Errno::EISDIR,
        Error::InvalidName => Errno::EINVAL,
        Error::Locked => Errno::EACCES,
        Error::Corrupt(_) | Error::Io(_) => Errno::EIO,
    }
}

fn name_str(name: &OsStr) -> std::result::Result<&str, Errno> {
    let s = name.to_str().ok_or(Errno::EINVAL)?;
    if s.len() > 255 {
        return Err(Errno::ENAMETOOLONG);
    }
    Ok(s)
}

struct FuseFs {
    fs: Arc<dyn Fs>,
    uid: u32,
    gid: u32,
    read_only: bool,
}

impl FuseFs {
    fn attr(&self, n: &Node) -> FileAttr {
        let t = util::ns_to_system_time(n.mtime);
        let (kind, perm, nlink) = match n.kind {
            Kind::File => (FileType::RegularFile, 0o644, 1),
            Kind::Dir => (FileType::Directory, 0o755, 2),
        };
        FileAttr {
            ino: INodeNo(n.id),
            size: n.size,
            blocks: n.size.div_ceil(512),
            atime: t,
            mtime: t,
            ctime: t,
            crtime: t,
            kind,
            perm: if self.read_only { perm & !0o222 } else { perm },
            nlink,
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            blksize: IO_SIZE,
            flags: 0,
        }
    }

    fn rw(&self) -> std::result::Result<(), Errno> {
        if self.read_only { Err(Errno::EROFS) } else { Ok(()) }
    }

    fn child(&self, parent: INodeNo, name: &OsStr) -> std::result::Result<Node, Errno> {
        let name = name_str(name)?;
        self.fs.lookup(parent.0, name).map_err(errno)
    }

    fn do_setattr(
        &self,
        ino: INodeNo,
        size: Option<u64>,
        mtime: Option<TimeOrNow>,
    ) -> std::result::Result<Node, Errno> {
        if size.is_some() || mtime.is_some() {
            self.rw()?;
        }
        if let Some(size) = size {
            self.fs.set_size(ino.0, size).map_err(errno)?;
        }
        if let Some(t) = mtime {
            let t = match t {
                TimeOrNow::Now => SystemTime::now(),
                TimeOrNow::SpecificTime(t) => t,
            };
            self.fs.set_mtime(ino.0, util::system_time_to_ns(t)).map_err(errno)?;
        }
        self.fs.getattr(ino.0).map_err(errno)
    }

    fn do_rename(
        &self,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        flags: RenameFlags,
    ) -> std::result::Result<(), Errno> {
        self.rw()?;
        if flags.contains(RenameFlags::RENAME_EXCHANGE) || flags.contains(RenameFlags::RENAME_WHITEOUT) {
            return Err(Errno::EINVAL);
        }
        let src = self.child(parent, name)?;
        let newname = name_str(newname)?;
        if flags.contains(RenameFlags::RENAME_NOREPLACE) {
            match self.fs.lookup(newparent.0, newname) {
                Ok(_) => return Err(Errno::EEXIST),
                Err(Error::NotFound) => {}
                Err(e) => return Err(errno(e)),
            }
        }
        self.fs.rename(src.id, newparent.0, newname).map_err(|e| match e {
            Error::Exists => Errno::ENOTEMPTY,
            e => errno(e),
        })
    }

    fn do_remove(&self, parent: INodeNo, name: &OsStr, want_dir: bool) -> std::result::Result<(), Errno> {
        self.rw()?;
        let n = self.child(parent, name)?;
        match (n.kind, want_dir) {
            (Kind::Dir, false) => return Err(Errno::EISDIR),
            (Kind::File, true) => return Err(Errno::ENOTDIR),
            _ => {}
        }
        self.fs.remove(n.id).map_err(errno)
    }

    fn flush_fs(&self) -> std::result::Result<(), Errno> {
        self.fs.flush().map_err(errno)
    }
}

impl Filesystem for FuseFs {
    fn init(&mut self, _req: &Request, config: &mut KernelConfig) -> std::io::Result<()> {
        let _ = config.set_max_write(1 << 20);
        let _ = config.set_max_readahead(1 << 20);
        Ok(())
    }

    fn destroy(&mut self) {
        let _ = self.fs.flush();
    }

    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        match self.child(parent, name) {
            Ok(n) => reply.entry(&TTL, &self.attr(&n), Generation(0)),
            Err(e) => reply.error(e),
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        match self.fs.getattr(ino.0) {
            Ok(n) => reply.attr(&TTL, &self.attr(&n)),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn setattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<fuser::BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        match self.do_setattr(ino, size, mtime) {
            Ok(n) => reply.attr(&TTL, &self.attr(&n)),
            Err(e) => reply.error(e),
        }
    }

    fn mkdir(&self, _req: &Request, parent: INodeNo, name: &OsStr, _mode: u32, _umask: u32, reply: ReplyEntry) {
        let r = self.rw().and_then(|_| {
            let name = name_str(name)?;
            self.fs.create(parent.0, name, Kind::Dir).map_err(errno)
        });
        match r {
            Ok(n) => reply.entry(&TTL, &self.attr(&n), Generation(0)),
            Err(e) => reply.error(e),
        }
    }

    fn unlink(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        match self.do_remove(parent, name, false) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn rmdir(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        match self.do_remove(parent, name, true) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn rename(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        flags: RenameFlags,
        reply: ReplyEmpty,
    ) {
        match self.do_rename(parent, name, newparent, newname, flags) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        if self.read_only && flags.acc_mode() != OpenAccMode::O_RDONLY {
            return reply.error(Errno::EROFS);
        }
        match self.fs.getattr(ino.0) {
            Ok(n) if n.kind == Kind::Dir => reply.error(Errno::EISDIR),
            Ok(_) => reply.opened(FileHandle(0), FopenFlags::empty()),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let mut buf = vec![0u8; size as usize];
        match self.fs.read(ino.0, offset, &mut buf) {
            Ok(n) => reply.data(&buf[..n]),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn write(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        let r = self.rw().and_then(|_| self.fs.write(ino.0, offset, data).map_err(errno));
        match r {
            Ok(n) => reply.written(n as u32),
            Err(e) => reply.error(e),
        }
    }

    fn flush(&self, _req: &Request, _ino: INodeNo, _fh: FileHandle, _lock_owner: LockOwner, reply: ReplyEmpty) {
        reply.ok();
    }

    fn release(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        reply.ok();
    }

    fn fsync(&self, _req: &Request, _ino: INodeNo, _fh: FileHandle, _datasync: bool, reply: ReplyEmpty) {
        match self.flush_fs() {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn fsyncdir(&self, _req: &Request, _ino: INodeNo, _fh: FileHandle, _datasync: bool, reply: ReplyEmpty) {
        match self.flush_fs() {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn readdir(&self, _req: &Request, ino: INodeNo, _fh: FileHandle, offset: u64, mut reply: ReplyDirectory) {
        let (me, children) = match self.fs.getattr(ino.0).and_then(|me| Ok((me, self.fs.readdir(ino.0)?))) {
            Ok(v) => v,
            Err(e) => return reply.error(errno(e)),
        };
        if me.kind != Kind::Dir {
            return reply.error(Errno::ENOTDIR);
        }
        let dots = [(me.id, FileType::Directory, "."), (me.parent, FileType::Directory, "..")];
        let all = dots.into_iter().chain(children.iter().map(|c| {
            let t = if c.kind == Kind::Dir { FileType::Directory } else { FileType::RegularFile };
            (c.id, t, c.name.as_str())
        }));
        for (i, (id, kind, name)) in all.enumerate().skip(offset as usize) {
            if reply.add(INodeNo(id), (i + 1) as u64, kind, name) {
                break;
            }
        }
        reply.ok();
    }

    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: ReplyStatfs) {
        match self.fs.stats() {
            Ok(s) => {
                let b = u64::from(BLOCK);
                let blocks = s.total_bytes / b;
                let free = (s.free_bytes / b).min(blocks);
                let ffree = u64::from(u32::MAX);
                reply.statfs(blocks, free, free, s.files + ffree, ffree, BLOCK, 255, BLOCK);
            }
            Err(e) => reply.error(errno(e)),
        }
    }

    fn create(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        let r = self.rw().and_then(|_| {
            let name = name_str(name)?;
            self.fs.create(parent.0, name, Kind::File).map_err(errno)
        });
        match r {
            Ok(n) => reply.created(&TTL, &self.attr(&n), Generation(0), FileHandle(0), FopenFlags::empty()),
            Err(e) => reply.error(e),
        }
    }
}

fn fusermount_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FUSERMOUNT_PATH") {
        return Some(PathBuf::from(p));
    }
    util::find_exe("fusermount3").or_else(|| util::find_exe("fusermount"))
}

pub(crate) fn available() -> bool {
    let dev_ok = std::fs::OpenOptions::new().read(true).write(true).open("/dev/fuse").is_ok();
    dev_ok && fusermount_bin().is_some()
}

fn clear_stale(p: &Path) {
    if let Some(bin) = fusermount_bin() {
        let _ = util::run(Command::new(bin).arg("-u").arg("-z").arg("-q").arg("--").arg(p));
    }
}

fn default_base() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(d) if !d.is_empty() => PathBuf::from(d).join("nicevault"),
        _ => std::env::temp_dir().join(format!("nicevault-{}", unsafe { libc::getuid() })),
    }
}

struct FuseMount {
    session: fuser::BackgroundSession,
    dir: MountDir,
    fs: Arc<dyn Fs>,
}

impl Unmounter for FuseMount {
    fn unmount(self: Box<Self>) -> Result<()> {
        let FuseMount { session, dir, fs } = *self;
        let bin = fusermount_bin().unwrap_or_else(|| PathBuf::from("fusermount3"));
        let res = match util::run(Command::new(&bin).arg("-u").arg("--").arg(&dir.path)) {
            Ok(()) => {
                if let Err(e) = session.join() {
                    log::warn!("fuse session ended with error: {e}");
                }
                Ok(())
            }
            Err(busy) => {
                match util::run(Command::new(&bin).arg("-u").arg("-z").arg("--").arg(&dir.path)) {
                    Ok(()) => {
                        drop(session);
                        Ok(())
                    }
                    Err(e) => {
                        std::mem::forget(session);
                        Err(MountError::Busy(format!("{busy}; lazy unmount: {e}")))
                    }
                }
            }
        };
        let _ = fs.flush();
        if res.is_ok() {
            dir.cleanup();
        }
        res
    }
}

pub(crate) fn mount(fs: Arc<dyn Fs>, opts: &MountOpts) -> Result<(PathBuf, Box<dyn Unmounter>)> {
    if !available() {
        return Err(MountError::Unavailable(
            "FUSE is not usable (/dev/fuse not accessible or fusermount3 missing)".into(),
        ));
    }
    let dir = match &opts.mountpoint {
        Some(p) => util::explicit_dir(p, &clear_stale)?,
        None => util::pick_dir_under(&default_base(), &opts.volume_name, &clear_stale)?,
    };
    let mut mount_options = vec![
        MountOption::FSName("nicevault".into()),
        MountOption::Subtype("nicevault".into()),
        MountOption::DefaultPermissions,
        MountOption::NoDev,
        MountOption::NoSuid,
        MountOption::NoAtime,
    ];
    mount_options.push(if opts.read_only { MountOption::RO } else { MountOption::RW });
    let mut config = Config::default();
    config.mount_options = mount_options;
    config.acl = SessionACL::Owner;
    config.n_threads = Some(std::thread::available_parallelism().map(|n| n.get().clamp(2, 8)).unwrap_or(4));
    config.clone_fd = true;

    let ffs = FuseFs {
        fs: fs.clone(),
        uid: unsafe { libc::getuid() },
        gid: unsafe { libc::getgid() },
        read_only: opts.read_only,
    };
    match fuser::spawn_mount(ffs, &dir.path, &config) {
        Ok(session) => {
            let path = dir.path.clone();
            Ok((path, Box::new(FuseMount { session, dir, fs })))
        }
        Err(e) => {
            dir.cleanup();
            Err(MountError::Failed(format!("{}: {e}", dir.path.display())))
        }
    }
}

