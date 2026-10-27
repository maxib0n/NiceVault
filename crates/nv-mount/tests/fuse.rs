#![cfg(target_os = "linux")]

mod common;

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use common::memfs::MemFs;
use nv_mount::{Backend, MountOpts};

fn fuse_usable() -> bool {
    if !nv_mount::available().contains(&Backend::Fuse) {
        eprintln!("SKIP: FUSE not usable here (/dev/fuse or fusermount3 missing)");
        return false;
    }
    true
}

fn is_mounted(p: &Path) -> bool {
    let p = p.to_string_lossy().replace(' ', "\\040");
    fs::read_to_string("/proc/mounts").unwrap().lines().any(|l| l.split(' ').nth(1) == Some(p.as_str()))
}

fn mount_at(fs: Arc<MemFs>, dir: &Path) -> Option<nv_mount::MountHandle> {
    let opts = MountOpts { volume_name: "Test".into(), mountpoint: Some(dir.to_path_buf()), read_only: false };
    match nv_mount::mount(fs, opts) {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("SKIP: FUSE mount failed in this environment: {e}");
            None
        }
    }
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i.wrapping_mul(31) ^ (i >> 11)) as u8).collect()
}

#[test]
fn end_to_end() {
    if !fuse_usable() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let mem = Arc::new(MemFs::new());
    let Some(h) = mount_at(mem.clone(), tmp.path()) else { return };
    let root = h.mountpoint().to_path_buf();
    assert!(is_mounted(&root));

    fs::create_dir_all(root.join("a/b/c")).unwrap();
    assert!(root.join("a/b/c").is_dir());

    let big = root.join("a/b/big.bin");
    let mut data = pattern(3 << 20);
    {
        let mut f = fs::File::create(&big).unwrap();
        for chunk in data.chunks(100_003) {
            f.write_all(chunk).unwrap();
        }
        f.seek(SeekFrom::Start(1_000_000)).unwrap();
        f.write_all(b"PATCHED-IN-THE-MIDDLE").unwrap();
        data[1_000_000..1_000_000 + 21].copy_from_slice(b"PATCHED-IN-THE-MIDDLE");
        f.sync_all().unwrap();
    }
    assert_eq!(fs::metadata(&big).unwrap().len(), 3 << 20);
    assert_eq!(fs::read(&big).unwrap(), data);
    {
        let mut f = fs::File::open(&big).unwrap();
        let mut buf = [0u8; 4096];
        f.seek(SeekFrom::Start(2_999_000)).unwrap();
        f.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[..], &data[2_999_000..2_999_000 + 4096]);
        f.seek(SeekFrom::End(-10)).unwrap();
        let mut tail = Vec::new();
        f.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, &data[data.len() - 10..]);
    }
    {
        let mut f = OpenOptions::new().write(true).open(&big).unwrap();
        f.seek(SeekFrom::Start((3 << 20) + 100)).unwrap();
        f.write_all(b"end").unwrap();
    }
    let got = fs::read(&big).unwrap();
    assert_eq!(got.len(), (3 << 20) + 103);
    assert!(got[3 << 20..(3 << 20) + 100].iter().all(|&b| b == 0));

    OpenOptions::new().write(true).open(&big).unwrap().set_len(10).unwrap();
    assert_eq!(fs::read(&big).unwrap(), &data[..10]);
    OpenOptions::new().write(true).open(&big).unwrap().set_len(20).unwrap();
    let got = fs::read(&big).unwrap();
    assert_eq!(&got[..10], &data[..10]);
    assert_eq!(&got[10..], &[0u8; 10]);
    fs::write(&big, b"short").unwrap();
    assert_eq!(fs::read(&big).unwrap(), b"short");

    fs::write(root.join("a/x.txt"), b"x").unwrap();
    fs::write(root.join("a/y.txt"), b"yyyy").unwrap();
    fs::rename(root.join("a/x.txt"), root.join("a/y.txt")).unwrap();
    assert!(!root.join("a/x.txt").exists());
    assert_eq!(fs::read(root.join("a/y.txt")).unwrap(), b"x");
    fs::rename(root.join("a/y.txt"), root.join("a/b/c/moved.txt")).unwrap();
    assert_eq!(fs::read(root.join("a/b/c/moved.txt")).unwrap(), b"x");
    fs::create_dir(root.join("empty")).unwrap();
    assert!(fs::rename(root.join("empty"), root.join("a/b")).is_err());

    let mut names: Vec<String> =
        fs::read_dir(root.join("a/b")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    assert_eq!(names, ["big.bin", "c"]);
    fs::create_dir(root.join("many")).unwrap();
    for i in 0..500 {
        fs::write(root.join(format!("many/file-with-a-longish-name-{i:04}.txt")), i.to_string()).unwrap();
    }
    assert_eq!(fs::read_dir(root.join("many")).unwrap().count(), 500);

    fs::remove_file(root.join("a/b/big.bin")).unwrap();
    assert!(!root.join("a/b/big.bin").exists());
    assert!(fs::remove_dir(root.join("a/b")).is_err(), "non-empty dir must not be removed");
    assert!(fs::remove_file(root.join("a/b")).is_err(), "unlink on a dir must fail");
    fs::remove_dir_all(root.join("a")).unwrap();
    fs::remove_dir_all(root.join("many")).unwrap();
    fs::remove_dir(root.join("empty")).unwrap();
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);

    let st = unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        let c = std::ffi::CString::new(root.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(libc::statvfs(c.as_ptr(), &mut s), 0);
        s
    };
    assert!(st.f_blocks > 0);

    fs::write(root.join("t"), b"t").unwrap();
    let f = OpenOptions::new().write(true).open(root.join("t")).unwrap();
    let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    f.set_modified(when).unwrap();
    drop(f);
    assert_eq!(fs::metadata(root.join("t")).unwrap().modified().unwrap(), when);

    assert!(mem.flush_count() > 0, "fsync must reach Fs::flush");

    h.unmount().unwrap();
    assert!(!is_mounted(&root));
    assert!(root.exists(), "caller-provided mountpoint must be left in place");
}

#[test]
fn drop_unmounts_and_default_mountpoint() {
    if !fuse_usable() {
        return;
    }
    let mem = Arc::new(MemFs::new());
    let h = match nv_mount::mount(mem, MountOpts::new("Drop Test")) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("SKIP: FUSE mount failed in this environment: {e}");
            return;
        }
    };
    let root = h.mountpoint().to_path_buf();
    assert!(root.ends_with("Drop Test") || root.file_name().unwrap().to_string_lossy().starts_with("Drop Test"));
    assert!(is_mounted(&root));
    fs::write(root.join("f"), b"hello").unwrap();
    drop(h);
    assert!(!is_mounted(&root), "drop must unmount");
    assert!(!root.exists(), "auto-created mountpoint dir must be removed");
}

#[test]
fn busy_unmount_detaches() {
    if !fuse_usable() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let Some(h) = mount_at(Arc::new(MemFs::new()), tmp.path()) else { return };
    let root = h.mountpoint().to_path_buf();
    let mut open = fs::File::create(root.join("held-open")).unwrap();
    h.unmount().unwrap();
    assert!(!is_mounted(&root));
    open.write_all(b"still alive").unwrap();
}

#[test]
fn read_only() {
    if !fuse_usable() {
        return;
    }
    let mem = Arc::new(MemFs::new());
    {
        use nv_fs::Fs;
        let n = mem.create(nv_fs::ROOT, "r.txt", nv_fs::Kind::File).unwrap();
        mem.write(n.id, 0, b"ro").unwrap();
    }
    let tmp = tempfile::tempdir().unwrap();
    let opts = MountOpts { volume_name: "RO".into(), mountpoint: Some(tmp.path().to_path_buf()), read_only: true };
    let h = match nv_mount::mount(mem, opts) {
        Ok(h) => h,
        Err(e) => return eprintln!("SKIP: {e}"),
    };
    let root = h.mountpoint();
    assert_eq!(fs::read(root.join("r.txt")).unwrap(), b"ro");
    assert!(fs::write(root.join("new"), b"x").is_err());
    assert!(OpenOptions::new().write(true).open(root.join("r.txt")).is_err());
    assert!(fs::create_dir(root.join("d")).is_err());
    assert!(fs::remove_file(root.join("r.txt")).is_err());
}

struct Spy {
    v: Arc<nv_core::Vault>,
    flushes: std::sync::atomic::AtomicU64,
    max_write: std::sync::atomic::AtomicUsize,
}

impl nv_fs::Fs for Spy {
    fn getattr(&self, id: nv_fs::NodeId) -> nv_fs::Result<nv_fs::Node> { self.v.getattr(id) }
    fn lookup(&self, p: nv_fs::NodeId, n: &str) -> nv_fs::Result<nv_fs::Node> { self.v.lookup(p, n) }
    fn readdir(&self, d: nv_fs::NodeId) -> nv_fs::Result<Vec<nv_fs::Node>> { self.v.readdir(d) }
    fn create(&self, p: nv_fs::NodeId, n: &str, k: nv_fs::Kind) -> nv_fs::Result<nv_fs::Node> { self.v.create(p, n, k) }
    fn read(&self, id: nv_fs::NodeId, off: u64, buf: &mut [u8]) -> nv_fs::Result<usize> { self.v.read(id, off, buf) }
    fn write(&self, id: nv_fs::NodeId, off: u64, data: &[u8]) -> nv_fs::Result<usize> {
        self.max_write.fetch_max(data.len(), std::sync::atomic::Ordering::Relaxed);
        self.v.write(id, off, data)
    }
    fn set_size(&self, id: nv_fs::NodeId, size: u64) -> nv_fs::Result<()> { self.v.set_size(id, size) }
    fn set_mtime(&self, id: nv_fs::NodeId, t: i64) -> nv_fs::Result<()> { self.v.set_mtime(id, t) }
    fn rename(&self, id: nv_fs::NodeId, p: nv_fs::NodeId, n: &str) -> nv_fs::Result<()> { self.v.rename(id, p, n) }
    fn remove(&self, id: nv_fs::NodeId) -> nv_fs::Result<()> { self.v.remove(id) }
    fn flush(&self) -> nv_fs::Result<()> {
        self.flushes.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.v.flush()
    }
    fn stats(&self) -> nv_fs::Result<nv_fs::Stats> { self.v.stats() }
}

#[test]
fn vault_big_writes_no_sync_on_close() {
    use nv_fs::Fs as _;
    use std::os::unix::fs::MetadataExt;
    use std::sync::atomic::Ordering::Relaxed;
    if !fuse_usable() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("v.nvault");
    let pw = secrecy::SecretString::from("pw".to_owned());
    let v = Arc::new(nv_core::Vault::create(&path, &pw, nv_core::KdfPreset::Fast).unwrap());
    let spy = Arc::new(Spy { v: v.clone(), flushes: 0.into(), max_write: 0.into() });
    let mnt = tmp.path().join("mnt");
    fs::create_dir(&mnt).unwrap();
    let opts = MountOpts { volume_name: "Spy".into(), mountpoint: Some(mnt.clone()), read_only: false };
    let h = match nv_mount::mount(spy.clone(), opts) {
        Ok(h) => h,
        Err(e) => return eprintln!("SKIP: FUSE mount failed in this environment: {e}"),
    };
    let root = h.mountpoint().to_path_buf();
    let big = pattern(8 << 20);
    let syncs0 = nv_core::io_counters().total_syncs();
    {
        let mut f = fs::File::create(root.join("big.bin")).unwrap();
        for c in big.chunks(4 << 20) {
            f.write_all(c).unwrap();
        }
    }
    for i in 0..200 {
        fs::write(root.join(format!("small-{i:03}")), pattern(3000 + i)).unwrap();
    }
    assert_eq!(fs::metadata(root.join("big.bin")).unwrap().blksize(), 1 << 20, "preferred I/O size");
    assert_eq!(spy.flushes.load(Relaxed), 0, "close(2) must not be a durability point");
    let syncs = nv_core::io_counters().total_syncs() - syncs0;
    assert!(syncs <= 8, "{syncs} syncs while copying 201 files");
    assert!(spy.max_write.load(Relaxed) >= 1 << 20, "max write {} B: big_writes / max_pages not negotiated", spy.max_write.load(Relaxed));

    let f = fs::OpenOptions::new().write(true).open(root.join("small-007")).unwrap();
    f.sync_all().unwrap();
    assert_eq!(spy.flushes.load(Relaxed), 1, "fsync(2) is a durability point");
    drop(f);
    h.unmount().unwrap();
    drop(spy);
    Arc::try_unwrap(v).ok().expect("vault still shared").close().unwrap();

    let v = nv_core::Vault::open(&path, &pw).unwrap();
    let n = v.lookup(nv_fs::ROOT, "big.bin").unwrap();
    let mut got = vec![0u8; n.size as usize];
    let mut off = 0;
    while off < got.len() {
        off += v.read(n.id, off as u64, &mut got[off..]).unwrap();
    }
    assert!(got == big);
    let n = v.lookup(nv_fs::ROOT, "small-199").unwrap();
    let mut got = vec![0u8; n.size as usize];
    assert_eq!(v.read(n.id, 0, &mut got).unwrap(), 3199);
    assert_eq!(got, pattern(3199));
    v.close().unwrap();
}
