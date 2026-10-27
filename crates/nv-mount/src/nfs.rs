use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

pub(crate) const GATE_CLOSED: u8 = 0;
pub(crate) const GATE_OPEN: u8 = 1;
pub(crate) const GATE_USED: u8 = 2;
pub(crate) const GATE_RACED: u8 = 3;

use async_trait::async_trait;
use nfsserve::nfs::{
    fattr3, fileid3, filename3, fsinfo3, ftype3, nfs_fh3, nfspath3, nfsstat3, nfstime3, post_op_attr, sattr3, set_mtime,
    set_size3, specdata3,
};
use nfsserve::vfs::{DirEntry, NFSFileSystem, ReadDirResult, VFSCapabilities};
use nv_fs::{Error, Fs, Kind, Node, NodeId, ROOT};

use crate::util;

fn stat(e: Error) -> nfsstat3 {
    match e {
        Error::NotFound => nfsstat3::NFS3ERR_NOENT,
        Error::Exists => nfsstat3::NFS3ERR_EXIST,
        Error::NotEmpty => nfsstat3::NFS3ERR_NOTEMPTY,
        Error::NotDir => nfsstat3::NFS3ERR_NOTDIR,
        Error::IsDir => nfsstat3::NFS3ERR_ISDIR,
        Error::InvalidName => nfsstat3::NFS3ERR_INVAL,
        Error::Locked => nfsstat3::NFS3ERR_ACCES,
        Error::Corrupt(_) | Error::Io(_) => nfsstat3::NFS3ERR_IO,
    }
}

fn name_str(n: &filename3) -> Result<String, nfsstat3> {
    if n.len() > 255 {
        return Err(nfsstat3::NFS3ERR_NAMETOOLONG);
    }
    String::from_utf8(n.0.clone()).map_err(|_| nfsstat3::NFS3ERR_INVAL)
}

fn nfstime(ns: i64) -> nfstime3 {
    let ns = ns.max(0);
    nfstime3 { seconds: (ns / 1_000_000_000).min(u32::MAX as i64) as u32, nseconds: (ns % 1_000_000_000) as u32 }
}

fn time_ns(t: nfstime3) -> i64 {
    t.seconds as i64 * 1_000_000_000 + t.nseconds as i64
}

pub(crate) struct NfsAdapter {
    fs: Arc<dyn Fs>,
    uid: u32,
    gid: u32,
    read_only: bool,
    secret: [u8; 16],
    pub(crate) accept_mount: Arc<AtomicU8>,
}

fn random_secret() -> std::io::Result<[u8; 16]> {
    let mut b = [0u8; 16];
    if unsafe { libc::getentropy(b.as_mut_ptr().cast(), b.len()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(b)
}

impl NfsAdapter {
    pub(crate) fn new(fs: Arc<dyn Fs>, read_only: bool) -> std::io::Result<Self> {
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        Ok(Self { fs, uid, gid, read_only, secret: random_secret()?, accept_mount: Arc::new(AtomicU8::new(GATE_CLOSED)) })
    }

    async fn blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(&dyn Fs) -> nv_fs::Result<T> + Send + 'static,
    ) -> Result<T, nfsstat3> {
        let fs = self.fs.clone();
        match tokio::task::spawn_blocking(move || f(&*fs)).await {
            Ok(r) => r.map_err(stat),
            Err(_) => Err(nfsstat3::NFS3ERR_SERVERFAULT),
        }
    }

    fn rw(&self) -> Result<(), nfsstat3> {
        if self.read_only { Err(nfsstat3::NFS3ERR_ROFS) } else { Ok(()) }
    }

    fn fattr(&self, n: &Node) -> fattr3 {
        let t = nfstime(n.mtime);
        let (ftype, mode, nlink) = match n.kind {
            Kind::File => (ftype3::NF3REG, 0o600, 1),
            Kind::Dir => (ftype3::NF3DIR, 0o700, 2),
        };
        fattr3 {
            ftype,
            mode: if self.read_only { mode & !0o222 } else { mode },
            nlink,
            uid: self.uid,
            gid: self.gid,
            size: n.size,
            used: n.size,
            rdev: specdata3::default(),
            fsid: 0,
            fileid: n.id,
            atime: t,
            mtime: t,
            ctime: t,
        }
    }

    async fn apply_sattr(&self, id: NodeId, s: sattr3) -> Result<Node, nfsstat3> {
        let size = match s.size {
            set_size3::size(sz) => Some(sz),
            set_size3::Void => None,
        };
        let mtime = match s.mtime {
            set_mtime::DONT_CHANGE => None,
            set_mtime::SET_TO_SERVER_TIME => Some(util::now_ns()),
            set_mtime::SET_TO_CLIENT_TIME(t) => Some(time_ns(t)),
        };
        if size.is_some() || mtime.is_some() {
            self.rw()?;
        }
        self.blocking(move |fs| {
            if let Some(sz) = size {
                fs.set_size(id, sz)?;
            }
            if let Some(t) = mtime {
                fs.set_mtime(id, t)?;
            }
            fs.getattr(id)
        })
        .await
    }

    async fn create_node(&self, dirid: fileid3, name: &filename3, kind: Kind) -> Result<Node, nfsstat3> {
        self.rw()?;
        let name = name_str(name)?;
        self.blocking(move |fs| fs.create(dirid, &name, kind)).await
    }
}

#[async_trait]
impl NFSFileSystem for NfsAdapter {
    fn capabilities(&self) -> VFSCapabilities {
        if self.read_only { VFSCapabilities::ReadOnly } else { VFSCapabilities::ReadWrite }
    }

    fn root_dir(&self) -> fileid3 {
        ROOT
    }

    async fn lookup(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        match filename.as_slice() {
            b"." => return Ok(dirid),
            b".." => return self.blocking(move |fs| fs.getattr(dirid).map(|n| n.parent)).await,
            _ => {}
        }
        let name = name_str(filename)?;
        self.blocking(move |fs| fs.lookup(dirid, &name).map(|n| n.id)).await
    }

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        let n = self.blocking(move |fs| fs.getattr(id)).await?;
        Ok(self.fattr(&n))
    }

    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        let n = self.apply_sattr(id, setattr).await?;
        Ok(self.fattr(&n))
    }

    async fn read(&self, id: fileid3, offset: u64, count: u32) -> Result<(Vec<u8>, bool), nfsstat3> {
        self.blocking(move |fs| {
            let n = fs.getattr(id)?;
            if n.kind == Kind::Dir {
                return Err(Error::IsDir);
            }
            if offset >= n.size {
                return Ok((Vec::new(), true));
            }
            let want = (n.size - offset).min(count as u64) as usize;
            let mut buf = vec![0u8; want];
            let got = fs.read(id, offset, &mut buf)?;
            buf.truncate(got);
            let eof = got < want || offset + got as u64 >= n.size;
            Ok((buf, eof))
        })
        .await
    }

    async fn write(&self, id: fileid3, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        self.rw()?;
        let data = data.to_vec();
        let n = self
            .blocking(move |fs| {
                fs.write(id, offset, &data)?;
                fs.getattr(id)
            })
            .await?;
        Ok(self.fattr(&n))
    }

    async fn create(&self, dirid: fileid3, filename: &filename3, attr: sattr3) -> Result<(fileid3, fattr3), nfsstat3> {
        let n = self.create_node(dirid, filename, Kind::File).await?;
        let n = self.apply_sattr(n.id, attr).await?;
        Ok((n.id, self.fattr(&n)))
    }

    async fn create_exclusive(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        Ok(self.create_node(dirid, filename, Kind::File).await?.id)
    }

    async fn mkdir(&self, dirid: fileid3, dirname: &filename3) -> Result<(fileid3, fattr3), nfsstat3> {
        let n = self.create_node(dirid, dirname, Kind::Dir).await?;
        Ok((n.id, self.fattr(&n)))
    }

    async fn remove(&self, dirid: fileid3, filename: &filename3) -> Result<(), nfsstat3> {
        self.rw()?;
        let name = name_str(filename)?;
        self.blocking(move |fs| {
            let n = fs.lookup(dirid, &name)?;
            fs.remove(n.id)
        })
        .await
    }

    async fn rename(
        &self,
        from_dirid: fileid3,
        from_filename: &filename3,
        to_dirid: fileid3,
        to_filename: &filename3,
    ) -> Result<(), nfsstat3> {
        self.rw()?;
        let from = name_str(from_filename)?;
        let to = name_str(to_filename)?;
        let fs = self.fs.clone();
        match tokio::task::spawn_blocking(move || {
            let n = fs.lookup(from_dirid, &from)?;
            fs.rename(n.id, to_dirid, &to)
        })
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(Error::Exists)) => Err(nfsstat3::NFS3ERR_NOTEMPTY),
            Ok(Err(e)) => Err(stat(e)),
            Err(_) => Err(nfsstat3::NFS3ERR_SERVERFAULT),
        }
    }

    async fn readdir(&self, dirid: fileid3, start_after: fileid3, max_entries: usize) -> Result<ReadDirResult, nfsstat3> {
        let mut children = self
            .blocking(move |fs| {
                if fs.getattr(dirid)?.kind != Kind::Dir {
                    return Err(Error::NotDir);
                }
                fs.readdir(dirid)
            })
            .await?;
        children.sort_by_key(|n| n.id);
        let start = children.partition_point(|n| n.id <= start_after);
        let rest = &children[start..];
        let take = rest.len().min(max_entries);
        Ok(ReadDirResult {
            entries: rest[..take]
                .iter()
                .map(|n| DirEntry { fileid: n.id, name: n.name.as_bytes().into(), attr: self.fattr(n) })
                .collect(),
            end: take == rest.len(),
        })
    }

    async fn symlink(
        &self,
        _dirid: fileid3,
        _linkname: &filename3,
        _symlink: &nfspath3,
        _attr: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    async fn readlink(&self, _id: fileid3) -> Result<nfspath3, nfsstat3> {
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    fn id_to_fh(&self, id: fileid3) -> nfs_fh3 {
        let mut data = Vec::with_capacity(24);
        data.extend_from_slice(&self.secret);
        data.extend_from_slice(&id.to_le_bytes());
        nfs_fh3 { data }
    }

    fn fh_to_id(&self, fh: &nfs_fh3) -> Result<fileid3, nfsstat3> {
        if fh.data.len() != 24 {
            return Err(nfsstat3::NFS3ERR_BADHANDLE);
        }
        let diff = fh.data[..16].iter().zip(&self.secret).fold(0u8, |acc, (a, b)| acc | (a ^ b));
        if diff != 0 {
            return Err(nfsstat3::NFS3ERR_STALE);
        }
        Ok(u64::from_le_bytes(fh.data[16..24].try_into().unwrap()))
    }

    async fn path_to_id(&self, path: &[u8]) -> Result<fileid3, nfsstat3> {
        if !path.iter().all(|&c| c == b'/') {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
        match self.accept_mount.compare_exchange(GATE_OPEN, GATE_USED, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => Ok(ROOT),
            Err(GATE_USED) => {
                let _ = self.accept_mount.compare_exchange(GATE_USED, GATE_RACED, Ordering::SeqCst, Ordering::SeqCst);
                Err(nfsstat3::NFS3ERR_ACCES)
            }
            Err(_) => Err(nfsstat3::NFS3ERR_ACCES),
        }
    }

    fn serverid(&self) -> nfsserve::nfs::cookieverf3 {
        self.secret[..8].try_into().unwrap()
    }

    async fn fsinfo(&self, root_fileid: fileid3) -> Result<fsinfo3, nfsstat3> {
        let obj_attributes = match self.getattr(root_fileid).await {
            Ok(a) => post_op_attr::attributes(a),
            Err(_) => post_op_attr::Void,
        };
        Ok(fsinfo3 {
            obj_attributes,
            rtmax: 1 << 20,
            rtpref: 1 << 20,
            rtmult: 4096,
            wtmax: 1 << 20,
            wtpref: 1 << 20,
            wtmult: 4096,
            dtpref: 64 << 10,
            maxfilesize: u64::MAX >> 1,
            time_delta: nfstime3 { seconds: 0, nseconds: 1 },
            properties: nfsserve::nfs::FSF_HOMOGENEOUS | nfsserve::nfs::FSF_CANSETTIME,
        })
    }
}

pub(crate) struct Server {
    pub(crate) port: u16,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    pub(crate) fn start(adapter: NfsAdapter) -> std::io::Result<Server> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(8)
            .thread_name("nv-nfs")
            .enable_io()
            .build()?;
        let listener = rt.block_on(nfsserve::tcp::NFSTcpListener::bind("127.0.0.1:0", adapter))?;
        let port = nfsserve::tcp::NFSTcp::get_listen_port(&listener);
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let thread = std::thread::Builder::new().name("nv-nfs-server".into()).spawn(move || {
            rt.block_on(async move {
                tokio::select! {
                    r = nfsserve::tcp::NFSTcp::handle_forever(&listener) => if let Err(e) = r { log::warn!("nfs server stopped: {e}") },
                    _ = rx => {}
                }
            });
            rt.shutdown_timeout(std::time::Duration::from_secs(2));
        })?;
        Ok(Server { port, stop: Some(tx), thread: Some(thread) })
    }

    pub(crate) fn stop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}


#[cfg(target_os = "macos")]
pub(crate) use mac::{available, mount, unavailable_reason};

#[cfg(target_os = "macos")]
mod mac {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use nv_fs::Fs;

    use super::{GATE_CLOSED, GATE_OPEN, GATE_USED, NfsAdapter, Server};
    use crate::util::{self, MountDir};
    use crate::{MountError, MountOpts, Result, Unmounter};

    const MOUNT_NFS: &str = "/sbin/mount_nfs";

    pub(crate) fn available() -> bool {
        Path::new(MOUNT_NFS).exists() && home().is_some()
    }

    pub(crate) fn unavailable_reason() -> Option<(&'static str, String)> {
        if !Path::new(MOUNT_NFS).exists() {
            return Some(("nfs_missing", format!("macOS's NFS client ({MOUNT_NFS}) is missing on this Mac.")));
        }
        if home().is_none() {
            return Some(("nfs_no_home", "No home folder (HOME) to put the drive in.".into()));
        }
        None
    }

    fn home() -> Option<PathBuf> {
        std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
    }

    struct NfsMount {
        server: Server,
        dir: MountDir,
        fs: Arc<dyn Fs>,
    }

    fn do_unmount(p: &Path) -> std::result::Result<(), String> {
        let first = match util::run(Command::new("/sbin/umount").arg(p)) {
            Ok(()) => return Ok(()),
            Err(e) => e,
        };
        util::run(Command::new("/usr/sbin/diskutil").args(["unmount", "force"]).arg(p))
            .map_err(|e| format!("{first}; diskutil: {e}"))
    }

    impl Unmounter for NfsMount {
        fn unmount(self: Box<Self>) -> Result<()> {
            let NfsMount { mut server, dir, fs } = *self;
            if let Err(e) = do_unmount(&dir.path) {
                std::mem::forget(server);
                return Err(MountError::Busy(e));
            }
            server.stop();
            let _ = fs.flush();
            dir.cleanup();
            Ok(())
        }
    }

    pub(crate) fn mount(fs: Arc<dyn Fs>, opts: &MountOpts) -> Result<(PathBuf, Box<dyn Unmounter>)> {
        if !available() {
            return Err(MountError::Unavailable("mount_nfs not found".into()));
        }
        let noop = |_: &Path| {};
        let dir = match &opts.mountpoint {
            Some(p) => util::explicit_dir(p, &noop)?,
            None => util::pick_dir_under(&home().unwrap().join("NiceVault"), &opts.volume_name, &noop)?,
        };
        let adapter = match NfsAdapter::new(fs.clone(), opts.read_only) {
            Ok(a) => a,
            Err(e) => {
                dir.cleanup();
                return Err(MountError::Failed(format!("random: {e}")));
            }
        };
        let accept_mount = adapter.accept_mount.clone();
        let server = match Server::start(adapter) {
            Ok(s) => s,
            Err(e) => {
                dir.cleanup();
                return Err(MountError::Failed(format!("nfs server: {e}")));
            }
        };
        let port = server.port;
        let mut o = format!(
            "vers=3,tcp,port={port},mountport={port},locallocks,soft,intr,\
             rsize=1048576,wsize=1048576,readahead=16,actimeo=1,nosuid,nodev"
        );
        if opts.read_only {
            o.push_str(",rdonly");
        }
        accept_mount.store(GATE_OPEN, Ordering::SeqCst);
        let res = util::run(Command::new(MOUNT_NFS).arg("-o").arg(&o).arg("127.0.0.1:/").arg(&dir.path));
        let gate = accept_mount.swap(GATE_CLOSED, Ordering::SeqCst);
        if let Err(e) = res {
            drop(server);
            dir.cleanup();
            return Err(MountError::Failed(format!("mount_nfs: {e}")));
        }
        let path = dir.path.clone();
        let m = Box::new(NfsMount { server, dir, fs });
        if gate != GATE_USED {
            let _ = m.unmount();
            return Err(MountError::Failed("another process tried to mount the volume".into()));
        }
        Ok((path, m))
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use std::sync::Arc;

    use nfsserve::nfs::{nfsstat3, sattr3, set_mtime, set_size3};
    use nfsserve::vfs::NFSFileSystem;
    use nv_fs::{Fs, ROOT};

    use super::{NfsAdapter, Server};
    use crate::memfs::MemFs;

    fn n(s: &str) -> nfsserve::nfs::filename3 {
        s.as_bytes().into()
    }

    fn is(e: Result<impl std::fmt::Debug, nfsstat3>, want: nfsstat3) {
        match e {
            Err(got) => assert_eq!(got as u32, want as u32, "got {got:?}, want {want:?}"),
            Ok(v) => panic!("expected {want:?}, got Ok({v:?})"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn adapter_roundtrip() {
        let mem = Arc::new(MemFs::new());
        let a = NfsAdapter::new(mem.clone(), false).unwrap();
        assert_eq!(a.root_dir(), ROOT);

        let (d, dattr) = a.mkdir(ROOT, &n("dir")).await.unwrap();
        assert!(matches!(dattr.ftype, nfsserve::nfs::ftype3::NF3DIR));
        assert_eq!(a.lookup(ROOT, &n("dir")).await.unwrap(), d);
        assert_eq!(a.lookup(d, &n(".")).await.unwrap(), d);
        assert_eq!(a.lookup(d, &n("..")).await.unwrap(), ROOT);
        is(a.lookup(ROOT, &n("nope")).await, nfsstat3::NFS3ERR_NOENT);
        is(a.mkdir(ROOT, &n("dir")).await, nfsstat3::NFS3ERR_EXIST);

        let (f, _) = a.create(d, &n("f.bin"), sattr3::default()).await.unwrap();
        let data: Vec<u8> = (0..3_000_000u32).map(|i| (i * 7) as u8).collect();
        for (i, c) in data.chunks(1 << 20).enumerate() {
            a.write(f, (i << 20) as u64, c).await.unwrap();
        }
        assert_eq!(a.getattr(f).await.unwrap().size, 3_000_000);
        let (r, eof) = a.read(f, 1_000_000, 4096).await.unwrap();
        assert_eq!(r, &data[1_000_000..1_004_096]);
        assert!(!eof);
        let (r, eof) = a.read(f, 2_999_990, 4096).await.unwrap();
        assert_eq!(r, &data[2_999_990..]);
        assert!(eof);
        let (r, eof) = a.read(f, 5_000_000, 10).await.unwrap();
        assert!(r.is_empty() && eof);

        let mut s = sattr3::default();
        s.size = set_size3::size(10);
        s.mtime = set_mtime::SET_TO_CLIENT_TIME(nfsserve::nfs::nfstime3 { seconds: 1_000_000_000, nseconds: 5 });
        let at = a.setattr(f, s).await.unwrap();
        assert_eq!(at.size, 10);
        assert_eq!(at.mtime.seconds, 1_000_000_000);
        assert_eq!(mem.getattr(f).unwrap().mtime, 1_000_000_000_000_000_005);

        let mut s = sattr3::default();
        s.size = set_size3::size(0);
        let (g, gattr) = a.create(d, &n("g"), s).await.unwrap();
        assert_eq!(gattr.size, 0);
        is(a.create_exclusive(d, &n("g")).await, nfsstat3::NFS3ERR_EXIST);

        a.rename(d, &n("g"), d, &n("f.bin")).await.unwrap();
        assert_eq!(a.lookup(d, &n("f.bin")).await.unwrap(), g);
        is(a.lookup(d, &n("g")).await, nfsstat3::NFS3ERR_NOENT);
        a.mkdir(ROOT, &n("empty")).await.unwrap();
        is(a.rename(ROOT, &n("empty"), ROOT, &n("dir")).await, nfsstat3::NFS3ERR_NOTEMPTY);

        is(a.remove(ROOT, &n("dir")).await, nfsstat3::NFS3ERR_NOTEMPTY);
        a.remove(d, &n("f.bin")).await.unwrap();
        a.remove(ROOT, &n("dir")).await.unwrap();
        a.remove(ROOT, &n("empty")).await.unwrap();
        assert!(mem.readdir(ROOT).unwrap().is_empty());

        is(a.symlink(ROOT, &n("l"), &n("t"), &sattr3::default()).await, nfsstat3::NFS3ERR_NOTSUPP);
        is(a.create(ROOT, &"x".repeat(300).as_bytes().into(), sattr3::default()).await, nfsstat3::NFS3ERR_NAMETOOLONG);
        is(a.create(ROOT, &vec![0xff, 0xfe].into(), sattr3::default()).await, nfsstat3::NFS3ERR_INVAL);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handles_and_mount_gate() {
        let a = NfsAdapter::new(Arc::new(MemFs::new()), false).unwrap();
        let b = NfsAdapter::new(Arc::new(MemFs::new()), false).unwrap();
        is(a.path_to_id(b"/").await, nfsstat3::NFS3ERR_ACCES);
        a.accept_mount.store(super::GATE_OPEN, std::sync::atomic::Ordering::SeqCst);
        is(a.path_to_id(b"/etc").await, nfsstat3::NFS3ERR_NOENT);
        assert_eq!(a.path_to_id(b"/").await.unwrap(), ROOT);
        is(a.path_to_id(b"/").await, nfsstat3::NFS3ERR_ACCES);
        assert_eq!(a.accept_mount.load(std::sync::atomic::Ordering::SeqCst), super::GATE_RACED);
        let fh = a.id_to_fh(42);
        assert_eq!(a.fh_to_id(&fh).unwrap(), 42);
        is(b.fh_to_id(&fh), nfsstat3::NFS3ERR_STALE);
        let mut forged = fh.clone();
        forged.data[3] ^= 1;
        is(a.fh_to_id(&forged), nfsstat3::NFS3ERR_STALE);
        forged.data.truncate(16);
        is(a.fh_to_id(&forged), nfsstat3::NFS3ERR_BADHANDLE);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn readdir_pagination() {
        let mem = Arc::new(MemFs::new());
        let a = NfsAdapter::new(mem.clone(), false).unwrap();
        for i in 0..25 {
            a.create(ROOT, &n(&format!("f{i}")), sattr3::default()).await.unwrap();
        }
        let mut seen = Vec::new();
        let mut after = 0;
        loop {
            let r = a.readdir(ROOT, after, 7).await.unwrap();
            assert!(r.entries.len() <= 7);
            seen.extend(r.entries.iter().map(|e| String::from_utf8(e.name.0.clone()).unwrap()));
            if r.end {
                break;
            }
            after = r.entries.last().unwrap().fileid;
            mem.remove(after).unwrap();
        }
        assert_eq!(seen.len(), 25);
        is(a.readdir(seen.len() as u64 + 100, 0, 10).await, nfsstat3::NFS3ERR_NOENT);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn read_only_rejects_writes() {
        let mem = Arc::new(MemFs::new());
        let f = mem.create(ROOT, "f", nv_fs::Kind::File).unwrap().id;
        let a = NfsAdapter::new(mem, true).unwrap();
        assert!(matches!(a.capabilities(), nfsserve::vfs::VFSCapabilities::ReadOnly));
        is(a.write(f, 0, b"x").await, nfsstat3::NFS3ERR_ROFS);
        is(a.mkdir(ROOT, &n("d")).await, nfsstat3::NFS3ERR_ROFS);
        is(a.remove(ROOT, &n("f")).await, nfsstat3::NFS3ERR_ROFS);
        is(a.rename(ROOT, &n("f"), ROOT, &n("g")).await, nfsstat3::NFS3ERR_ROFS);
        let mut s = sattr3::default();
        s.size = set_size3::size(0);
        is(a.setattr(f, s).await, nfsstat3::NFS3ERR_ROFS);
        assert_eq!(a.getattr(f).await.unwrap().mode, 0o400);
    }

    #[test]
    fn oversized_rpc_record_drops_the_connection() {
        use std::io::{Read, Write};
        let adapter = NfsAdapter::new(Arc::new(MemFs::new()), false).unwrap();
        let mut server = Server::start(adapter).unwrap();
        for frags in [vec![3u32 << 20], vec![1 << 20, 1 << 20, 1 << 20]] {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", server.port)).unwrap();
            s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut closed = false;
            for len in frags {
                if s.write_all(&len.to_be_bytes()).is_err() {
                    closed = true;
                    break;
                }
                if len <= 1 << 20 && s.write_all(&vec![0u8; len as usize]).is_err() {
                    closed = true;
                    break;
                }
            }
            if !closed {
                let mut b = [0u8; 16];
                closed = match s.read(&mut b) {
                    Ok(0) => true,
                    Ok(_) => false,
                    Err(e) => !matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut),
                };
            }
            assert!(closed, "the server kept waiting for an oversized record");
        }
        server.stop();
    }

    #[test]
    fn over_the_wire() {
        use nfs3_client::Nfs3ConnectionBuilder;
        use nfs3_client::nfs3_types::nfs3 as w;
        use nfs3_client::nfs3_types::xdr_codec::Opaque;
        use nfs3_client::tokio::TokioConnector;

        let mem = Arc::new(MemFs::new());
        let adapter = NfsAdapter::new(mem.clone(), false).unwrap();
        let gate = adapter.accept_mount.clone();
        let mut server = Server::start(adapter).unwrap();
        let port = server.port;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let builder = || {
                Nfs3ConnectionBuilder::new(TokioConnector, "127.0.0.1", "/")
                    .connect_from_privileged_port(false)
                    .mount_port(port)
                    .nfs3_port(port)
            };
            assert!(builder().mount().await.is_err());
            gate.store(super::GATE_OPEN, std::sync::atomic::Ordering::SeqCst);
            let mut c = builder().mount().await.unwrap();
            assert!(builder().mount().await.is_err(), "second MOUNT in the window");
            gate.store(super::GATE_CLOSED, std::sync::atomic::Ordering::SeqCst);
            let root = c.root_nfs_fh3();
            assert_eq!(root.data.len(), 24);

            let name = |s: &str| w::filename3::from(s.as_bytes().to_vec());
            let created = c
                .create(&w::CREATE3args {
                    where_: w::diropargs3 { dir: root.clone(), name: name("big.bin") },
                    how: w::createhow3::UNCHECKED(w::sattr3::default()),
                })
                .await
                .unwrap()
                .unwrap();
            let w::Nfs3Option::Some(fh) = created.obj else { panic!("no handle") };

            let data: Vec<u8> = (0..(3u32 << 20)).map(|i| (i ^ (i >> 9)) as u8).collect();
            for (i, chunk) in data.chunks(1 << 20).enumerate() {
                let r = c
                    .write(&w::WRITE3args {
                        file: fh.clone(),
                        offset: (i << 20) as u64,
                        count: chunk.len() as u32,
                        stable: w::stable_how::UNSTABLE,
                        data: Opaque::borrowed(chunk),
                    })
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(r.count as usize, chunk.len());
            }
            let mut back = Vec::new();
            loop {
                let r = c
                    .read(&w::READ3args { file: fh.clone(), offset: back.len() as u64, count: 1 << 20 })
                    .await
                    .unwrap()
                    .unwrap();
                back.extend_from_slice(&r.data);
                if r.eof {
                    break;
                }
            }
            assert_eq!(back, data);

            let l = c
                .lookup(&w::LOOKUP3args { what: w::diropargs3 { dir: root.clone(), name: name("big.bin") } })
                .await
                .unwrap()
                .unwrap();
            assert_eq!(l.object.data.as_ref(), fh.data.as_ref());

            c.rename(&w::RENAME3args {
                from: w::diropargs3 { dir: root.clone(), name: name("big.bin") },
                to: w::diropargs3 { dir: root.clone(), name: name("renamed.bin") },
            })
            .await
            .unwrap()
            .unwrap();
            let rd = c
                .readdir(&w::READDIR3args {
                    dir: root.clone(),
                    cookie: 0,
                    cookieverf: w::cookieverf3::default(),
                    count: 64 * 1024,
                })
                .await
                .unwrap()
                .unwrap();
            let names: Vec<String> =
                rd.reply.entries.0.iter().map(|e| String::from_utf8(e.name.0.to_vec()).unwrap()).collect();
            assert!(names.contains(&"renamed.bin".to_string()), "{names:?}");

            let mut forged = fh.data.to_vec();
            forged[0] ^= 0x55;
            let r = c
                .getattr(&w::GETATTR3args { object: w::nfs_fh3 { data: Opaque::owned(forged) } })
                .await
                .unwrap();
            assert!(matches!(r, w::Nfs3Result::Err(_)));

            c.remove(&w::REMOVE3args { object: w::diropargs3 { dir: root.clone(), name: name("renamed.bin") } })
                .await
                .unwrap()
                .unwrap();
            let _ = c.unmount().await;
        });
        server.stop();
        assert!(mem.readdir(ROOT).unwrap().is_empty());
    }
}
