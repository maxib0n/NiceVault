use nv_core::{Identity, KdfPreset, OpenError, Recipient, SlotError, Vault};
use nv_fs::{Error, Fs, Kind, NodeId, ROOT};
use secrecy::SecretString;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

static HEAVY: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn heavy() -> std::sync::MutexGuard<'static, ()> {
    HEAVY.lock().unwrap_or_else(|e| e.into_inner())
}

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn pat(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E3779B97F4A7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

fn read_all(v: &Vault, id: NodeId) -> Vec<u8> {
    let size = v.getattr(id).unwrap().size as usize;
    let mut out = vec![0u8; size + 10];
    let mut off = 0;
    while off < size {
        let n = v.read(id, off as u64, &mut out[off..(off + 77_777).min(size + 10)]).unwrap();
        assert!(n > 0);
        off += n;
    }
    assert_eq!(off, size, "short read only at EOF");
    out.truncate(size);
    out
}

fn side(path: &Path, suffix: &str) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(suffix);
    p.into()
}

struct Model(HashMap<NodeId, Vec<u8>>);
impl Model {
    fn write(&mut self, v: &Vault, id: NodeId, off: usize, data: &[u8]) {
        assert_eq!(v.write(id, off as u64, data).unwrap(), data.len());
        let m = self.0.entry(id).or_default();
        if m.len() < off + data.len() {
            m.resize(off + data.len(), 0);
        }
        m[off..off + data.len()].copy_from_slice(data);
    }
    fn set_size(&mut self, v: &Vault, id: NodeId, size: usize) {
        v.set_size(id, size as u64).unwrap();
        self.0.entry(id).or_default().resize(size, 0);
    }
    fn check(&self, v: &Vault) {
        for (id, want) in &self.0 {
            let got = read_all(v, *id);
            assert_eq!(got.len(), want.len(), "size of {id}");
            assert!(got == *want, "content of node {id} differs");
        }
    }
}

#[test]
fn roundtrip_files_and_dirs() {
    let _g = heavy();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.nvault");
    let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
    assert!(matches!(Vault::create(&path, &pw("pw"), KdfPreset::Fast), Err(Error::Exists)));
    let mut m = Model(HashMap::new());

    let docs = v.create(ROOT, "docs", Kind::Dir).unwrap();
    let a = v.create(docs.id, "a.txt", Kind::File).unwrap();
    m.write(&v, a.id, 0, b"hello world");
    assert!(matches!(v.create(docs.id, "a.txt", Kind::File), Err(Error::Exists)));
    assert!(matches!(v.create(ROOT, "x/y", Kind::File), Err(Error::InvalidName)));
    assert!(matches!(v.create(a.id, "z", Kind::File), Err(Error::NotDir)));

    let big = v.create(ROOT, "big.bin", Kind::File).unwrap();
    let data = pat(1, 1_500_017);
    let mut off = 0;
    for (i, step) in [100_003usize, 262_144, 1, 333_333, 7].iter().cycle().enumerate() {
        if off >= data.len() {
            break;
        }
        let n = (*step).min(data.len() - off);
        m.write(&v, big.id, off, &data[off..off + n]);
        off += n;
        if i == 3 {
            v.flush().unwrap();
        }
    }
    m.write(&v, big.id, 262_144 - 5, &pat(2, 20));

    let sparse = v.create(docs.id, "sparse", Kind::File).unwrap();
    m.write(&v, sparse.id, 0, b"head");
    m.write(&v, sparse.id, 5 * 1024 * 1024 + 7, b"tail");

    let t = v.create(ROOT, "trunc", Kind::File).unwrap();
    m.write(&v, t.id, 0, &pat(3, 900_000));
    v.flush().unwrap();
    m.set_size(&v, t.id, 300_001);
    m.write(&v, t.id, 600_000, b"after");
    m.set_size(&v, t.id, 1_000_000);
    m.set_size(&v, t.id, 262_144);

    m.check(&v);

    let tmp = v.create(ROOT, "tmp", Kind::File).unwrap();
    v.write(tmp.id, 0, b"bye").unwrap();
    v.rename(tmp.id, docs.id, "renamed").unwrap();
    assert_eq!(v.lookup(docs.id, "renamed").unwrap().id, tmp.id);
    assert!(matches!(v.lookup(ROOT, "tmp"), Err(Error::NotFound)));
    assert!(matches!(v.remove(docs.id), Err(Error::NotEmpty)));
    assert!(matches!(v.rename(docs.id, docs.id, "loop"), Err(Error::InvalidName)));
    v.remove(tmp.id).unwrap();
    let rep = v.create(ROOT, "replace-me", Kind::File).unwrap();
    let src = v.create(ROOT, "src", Kind::File).unwrap();
    v.write(src.id, 0, b"new").unwrap();
    v.rename(src.id, ROOT, "replace-me").unwrap();
    assert!(matches!(v.getattr(rep.id), Err(Error::NotFound)));
    assert_eq!(read_all(&v, src.id), b"new");
    m.0.insert(src.id, b"new".to_vec());
    v.set_mtime(a.id, 1234).unwrap();

    let names: Vec<_> = v.readdir(docs.id).unwrap().into_iter().map(|n| n.name).collect();
    assert_eq!(names, ["a.txt", "sparse"]);
    let st = v.stats().unwrap();
    assert_eq!(st.files, 5);
    assert_eq!(st.used_bytes, m.0.values().map(|d| d.len() as u64).sum::<u64>());

    v.close().unwrap();
    assert!(!side(&path, "-wal").exists(), "WAL left behind");
    assert!(!side(&path, "-shm").exists(), "shm left behind");

    let v = Vault::open(&path, &pw("pw")).unwrap();
    m.check(&v);
    assert_eq!(v.getattr(a.id).unwrap().mtime, 1234);
    assert_eq!(v.lookup(ROOT, "replace-me").unwrap().id, src.id);
    let root_names: Vec<_> = v.readdir(ROOT).unwrap().into_iter().map(|n| n.name).collect();
    assert_eq!(root_names, ["big.bin", "docs", "replace-me", "trunc"]);
    v.check_integrity().unwrap();

    v.remove_recursive(docs.id).unwrap();
    assert!(matches!(v.getattr(sparse.id), Err(Error::NotFound)));
    assert!(matches!(v.getattr(a.id), Err(Error::NotFound)));
    v.check_integrity().unwrap();
    drop(v);
    assert!(!side(&path, "-wal").exists());
    let v = Vault::open(&path, &pw("pw")).unwrap();
    assert!(matches!(v.lookup(ROOT, "docs"), Err(Error::NotFound)));
    v.close().unwrap();
}

#[test]
fn wrong_password_and_not_a_vault() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.nvault");
    Vault::create(&path, &pw("right"), KdfPreset::Fast).unwrap().close().unwrap();
    assert_eq!(Vault::open(&path, &pw("wrong")).err(), Some(OpenError::WrongPassword));
    let junk = dir.path().join("junk.nvault");
    std::fs::write(&junk, pat(9, 400_000)).unwrap();
    assert_eq!(Vault::open(&junk, &pw("right")).err(), Some(OpenError::NotAVault));
    std::fs::write(&junk, b"tiny").unwrap();
    assert_eq!(Vault::open(&junk, &pw("right")).err(), Some(OpenError::NotAVault));
    assert!(matches!(Vault::open(&dir.path().join("missing"), &pw("x")), Err(OpenError::Io(_))));
    assert_eq!(serde_json::to_string(&OpenError::WrongPassword).unwrap(), r#"{"code":"wrong_password"}"#);
}

fn flip(path: &Path, off: u64) {
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
    let mut b = [0u8; 16];
    f.seek(SeekFrom::Start(off)).unwrap();
    f.read_exact(&mut b).unwrap();
    for x in &mut b {
        *x ^= 0x5a;
    }
    f.seek(SeekFrom::Start(off)).unwrap();
    f.write_all(&b).unwrap();
}

#[test]
fn header_copy_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.nvault");
    let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
    let f = v.create(ROOT, "f", Kind::File).unwrap();
    v.write(f.id, 0, b"payload").unwrap();
    v.close().unwrap();

    flip(&path, 40);
    let v = Vault::open(&path, &pw("pw")).expect("opens via copy B");
    assert_eq!(read_all(&v, f.id), b"payload");
    v.close().unwrap();

    flip(&path, 65536 + 40);
    Vault::open(&path, &pw("pw")).expect("opens via repaired copy A").close().unwrap();

    flip(&path, 40);
    flip(&path, 65536 + 40);
    assert!(matches!(Vault::open(&path, &pw("pw")), Err(OpenError::Corrupt(_))));
}

#[test]
#[cfg(unix)]
fn stale_header_copy_never_unlocks_old_password() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.nvault");
    Vault::create(&path, &pw("old"), KdfPreset::Fast).unwrap().close().unwrap();
    let before = std::fs::read(&path).unwrap()[..131072].to_vec();
    let v = Vault::open(&path, &pw("old")).unwrap();
    v.change_password(0, &pw("new"), KdfPreset::Fast, None).unwrap();
    v.close().unwrap();
    for copy in [0usize, 1] {
        use std::os::unix::fs::FileExt;
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.write_all_at(&before[copy * 65536..(copy + 1) * 65536], (copy * 65536) as u64).unwrap();
        drop(f);
        assert_eq!(Vault::open(&path, &pw("old")).err(), Some(OpenError::WrongPassword), "copy {copy}");
        Vault::open(&path, &pw("new")).unwrap().close().unwrap();
        let now = std::fs::read(&path).unwrap();
        assert_ne!(&now[copy * 65536..(copy + 1) * 65536], &before[copy * 65536..(copy + 1) * 65536]);
    }
}

const B_FILES: usize = 6;
const B_SIZE: usize = 12 * 1024 * 1024;
const B_FLUSH_EVERY: usize = 5 << 20;

#[test]
fn crash_child() {
    let Ok(path) = std::env::var("NV_CRASH_CHILD") else { return };
    let stop_after: usize = std::env::var("NV_CRASH_AFTER").unwrap().parse().unwrap();
    if let Ok(ms) = std::env::var("NV_CRASH_MS") {
        let ms: u64 = ms.parse().unwrap();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            std::process::exit(134);
        });
    }
    let v = Vault::open(Path::new(&path), &pw("pw")).unwrap();
    let mut written = 0usize;
    for i in 0..B_FILES {
        let f = v.create(ROOT, &format!("b{i}"), Kind::File).unwrap();
        let data = pat(100 + i as u64, B_SIZE);
        for c in data.chunks(100_000) {
            v.write(f.id, v.getattr(f.id).unwrap().size, c).unwrap();
            written += c.len();
            if written >= stop_after {
                std::process::exit(134);
            }
            if written % B_FLUSH_EVERY < c.len() {
                v.flush().unwrap();
            }
        }
    }
    std::process::exit(134);
}

#[test]
fn crash_mid_write() {
    let _g = heavy();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.nvault");
    let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
    let a = v.create(ROOT, "a", Kind::File).unwrap();
    let a_data = pat(7, 3_000_000);
    v.write(a.id, 0, &a_data).unwrap();
    v.flush().unwrap();
    v.close().unwrap();

    let exe = std::env::current_exe().unwrap();
    let runs = [(1_000, 0), (20 << 20, 0), (33 << 20, 0), (50 << 20, 0), (70 << 20, 0), (usize::MAX, 250), (usize::MAX, 400), (usize::MAX, 650), (usize::MAX, 900), (usize::MAX, 1100), (usize::MAX, 1500)];
    for (stop_after, ms) in runs {
        let mut cmd = std::process::Command::new(&exe);
        if ms > 0 {
            cmd.env("NV_CRASH_MS", ms.to_string());
        }
        let st = cmd
            .args(["crash_child", "--exact", "--nocapture", "--test-threads=1"])
            .env("NV_CRASH_CHILD", &path)
            .env("NV_CRASH_AFTER", stop_after.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(!st.success(), "child should have aborted");

        let v = Vault::open(&path, &pw("pw")).expect("reopen after crash");
        v.check_integrity().unwrap();
        assert_eq!(read_all(&v, a.id), a_data, "set A intact");
        let mut seen = 0;
        for i in 0..B_FILES {
            let Ok(n) = v.lookup(ROOT, &format!("b{i}")) else { continue };
            let got = read_all(&v, n.id);
            assert!(got.len() <= B_SIZE);
            assert!(got == pat(100 + i as u64, B_SIZE)[..got.len()], "b{i} is a consistent prefix");
            seen += got.len();
        }
        eprintln!("crash after {stop_after} B bytes / {ms} ms: {seen} B bytes survived");
        if ms == 0 {
            assert!(seen + B_FLUSH_EVERY + 100_000 >= stop_after.min(B_FILES * B_SIZE), "lost data that was flushed: {seen} B");
        }
        for i in 0..B_FILES {
            if let Ok(n) = v.lookup(ROOT, &format!("b{i}")) {
                v.remove(n.id).unwrap();
            }
        }
        v.close().unwrap();
    }
}

#[test]
fn durability_child() {
    let Ok(path) = std::env::var("NV_DUR_CHILD") else { return };
    let v = Vault::open(Path::new(&path), &pw("pw")).unwrap();
    let put = |name: &str, seed: u64| {
        let f = v.create(ROOT, name, Kind::File).unwrap();
        let data = pat(seed, 5_000_000);
        for (i, c) in data.chunks(1 << 20).enumerate() {
            v.write(f.id, (i << 20) as u64, c).unwrap();
        }
    };
    put("flushed", 201);
    v.flush().unwrap();
    put("by-timer", 202);
    std::thread::sleep(nv_core::DURABLE_INTERVAL + std::time::Duration::from_millis(1500));
    put("lost", 203);
    std::process::exit(134);
}

#[test]
fn durability_points() {
    let _g = heavy();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.nvault");
    Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap().close().unwrap();
    let st = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["durability_child", "--exact", "--nocapture", "--test-threads=1"])
        .env("NV_DUR_CHILD", &path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(!st.success(), "child should have aborted");
    let v = Vault::open(&path, &pw("pw")).expect("reopen after crash");
    v.check_integrity().unwrap();
    for (name, seed) in [("flushed", 201), ("by-timer", 202)] {
        let n = v.lookup(ROOT, name).unwrap_or_else(|_| panic!("{name} missing"));
        assert!(read_all(&v, n.id) == pat(seed, 5_000_000), "{name} incomplete");
    }
    if let Ok(n) = v.lookup(ROOT, "lost") {
        let got = read_all(&v, n.id);
        assert!(got == pat(203, 5_000_000)[..got.len()], "a consistent prefix at worst");
    }
    v.close().unwrap();
}

#[test]
fn slots_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.nvault");
    let fast = KdfPreset::Fast;
    let v = Vault::create(&path, &pw("one"), fast).unwrap();
    assert_eq!(v.unlocked_slot(), Some(0), "create unlocks slot 0");
    assert_eq!(v.add_password(&pw("two"), fast, Some(" Busta in cassaforte ")).unwrap(), 1);
    let id = Identity::generate();
    let r = Recipient::parse(&id.recipient().to_string()).unwrap();
    assert_eq!(v.add_recipient(&r, Some("Chiavetta")).unwrap(), 2);
    let s = v.slots();
    assert_eq!(s.iter().map(|s| s.kind).collect::<Vec<_>>(), ["password", "password", "recipient"]);
    assert_eq!(s[2].fingerprint.as_deref().unwrap().len(), 32);
    assert_eq!(s.iter().map(|s| s.label.as_deref()).collect::<Vec<_>>(), [None, Some("Busta in cassaforte"), Some("Chiavetta")]);
    assert_eq!(s.iter().map(|s| s.in_use).collect::<Vec<_>>(), [true, false, false]);
    let k = s[1].kdf.unwrap();
    assert_eq!((k.m_kib, k.t, k.p), (8 * 1024, 1, 1));
    assert!(s[2].kdf.is_none());
    assert!(matches!(v.add_password(&pw("x"), fast, Some(&"é".repeat(65))), Err(SlotError::LabelTooLong)));
    v.close().unwrap();

    let v = Vault::open(&path, &pw("two")).unwrap();
    assert_eq!(v.unlocked_slot(), Some(1));
    assert_eq!(v.slots().iter().map(|s| s.in_use).collect::<Vec<_>>(), [false, true, false]);
    assert!(v.verify_password(0, &pw("one")));
    assert!(!v.verify_password(0, &pw("two")), "password of another slot");
    assert!(!v.verify_password(0, &pw("")));
    assert!(!v.verify_password(2, &pw("one")), "recipient slot");
    assert!(!v.verify_password(7, &pw("one")), "missing slot");
    v.change_password(0, &pw("three"), fast, Some("Principale")).unwrap();
    v.change_password(1, &pw("two"), fast, None).unwrap();
    assert_eq!(v.slots()[1].label.as_deref(), Some("Busta in cassaforte"), "None keeps the label");
    assert!(v.verify_password(0, &pw("three")) && !v.verify_password(0, &pw("one")));
    assert!(matches!(v.change_password(2, &pw("x"), fast, None), Err(SlotError::NotPassword)));
    assert!(matches!(v.change_password(6, &pw("x"), fast, None), Err(SlotError::NotFound)));
    v.set_slot_label(2, Some("")).unwrap();
    assert_eq!(v.slots()[2].label, None, "empty label clears");
    v.set_slot_label(2, Some("Chiavetta blu")).unwrap();
    v.close().unwrap();
    assert_eq!(Vault::open(&path, &pw("one")).err(), Some(OpenError::WrongPassword));
    Vault::open(&path, &pw("three")).unwrap().close().unwrap();

    let v = Vault::open_with_identity(&path, &id).unwrap();
    assert_eq!(v.unlocked_slot(), Some(2));
    let s = v.slots();
    assert_eq!(s.iter().map(|s| s.label.as_deref()).collect::<Vec<_>>(), [Some("Principale"), Some("Busta in cassaforte"), Some("Chiavetta blu")]);
    assert_eq!(Vault::open_with_identity(&path, &Identity::generate()).err(), Some(OpenError::WrongPassword));
    assert!(matches!(Vault::open(&path, &pw("three")), Err(OpenError::Io(_))));
    drop(v);

    let kf = dir.path().join("me.nvkey");
    id.to_file_with(&kf, &pw("keypw"), fast).unwrap();
    assert!(matches!(id.to_file_with(&kf, &pw("keypw"), fast), Err(Error::Exists)));
    assert_eq!(Identity::from_file(&kf, &pw("nope")).err(), Some(OpenError::WrongPassword));
    let id2 = Identity::from_file(&kf, &pw("keypw")).unwrap();
    assert_eq!(id2.recipient().fingerprint(), r.fingerprint());

    let v = Vault::open_with_identity(&path, &id2).unwrap();
    v.remove_slot(1).unwrap();
    assert_eq!(v.slots().iter().map(|s| s.index).collect::<Vec<_>>(), [0, 2]);
    assert_eq!(v.add_password(&pw("four"), fast, None).unwrap(), 1, "hole is reused");
    assert_eq!(v.slots()[1].label, None, "a reused hole does not inherit the old label");
    v.remove_slot(1).unwrap();
    assert_eq!(v.unlocked_slot(), Some(2));
    v.remove_slot(2).unwrap();
    assert_eq!(v.unlocked_slot(), None, "the unlocking slot is gone");
    assert!(v.slots().iter().all(|s| !s.in_use));
    assert!(matches!(v.remove_slot(0), Err(SlotError::LastSlot)), "last slot must stay");
    assert!(matches!(v.remove_slot(5), Err(SlotError::NotFound)));
    for k in 1..8 {
        assert_eq!(v.add_password(&pw("fill"), fast, None).unwrap(), k);
    }
    assert!(matches!(v.add_password(&pw("fill"), fast, None), Err(SlotError::Full)));
    for k in 1..8 {
        v.remove_slot(k).unwrap();
    }
    v.close().unwrap();
    assert_eq!(Vault::open(&path, &pw("two")).err(), Some(OpenError::WrongPassword));
    assert_eq!(Vault::open(&path, &pw("four")).err(), Some(OpenError::WrongPassword));
    assert_eq!(Vault::open_with_identity(&path, &id).err(), Some(OpenError::WrongPassword));
    Vault::open(&path, &pw("three")).unwrap().close().unwrap();
}

#[test]
fn no_plaintext_on_disk() {
    let _g = heavy();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.nvault");
    let name = "SECRETNAME_q8z1";
    let content = "SECRETCONTENT_w3x9";
    let label = "SECRETLABEL_k2p7";
    let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
    v.add_password(&pw("pw2"), KdfPreset::Fast, Some(label)).unwrap();
    let d = v.create(ROOT, name, Kind::Dir).unwrap();
    let f = v.create(d.id, &format!("{name}.txt"), Kind::File).unwrap();
    v.write(f.id, 0, content.repeat(50_000).as_bytes()).unwrap();
    v.flush().unwrap();
    let scan = |label: &str| {
        for p in [path.clone(), side(&path, "-wal"), side(&path, "-shm"), side(&path, "-journal")] {
            let Ok(raw) = std::fs::read(&p) else { continue };
            for needle in [name, content, label] {
                assert!(!raw.windows(needle.len()).any(|w| w == needle.as_bytes()), "{label}: {needle} in {}", p.display());
            }
        }
    };
    scan("while open");
    v.close().unwrap();
    scan("after close");
}

#[test]
fn fast_create_is_fast() {
    let _g = heavy();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.nvault");
    let t = Instant::now();
    let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
    let el = t.elapsed();
    v.close().unwrap();
    eprintln!("Fast create: {el:?}");
    assert!(el.as_millis() < 200, "Fast create took {el:?}");
}

#[test]
#[ignore]
fn strong_unlock_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.nvault");
    let t = Instant::now();
    Vault::create(&path, &pw("pw"), KdfPreset::Strong).unwrap().close().unwrap();
    let c = t.elapsed();
    let t = Instant::now();
    Vault::open(&path, &pw("pw")).unwrap().close().unwrap();
    println!("Strong: create {c:?}, unlock {:?}", t.elapsed());
}

#[test]
#[ignore]
fn bench_sequential_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.nvault");
    let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
    let f = v.create(ROOT, "big", Kind::File).unwrap();
    let block = pat(5, 1 << 20);
    let total = 256u64 << 20;
    let t = Instant::now();
    let mut off = 0;
    while off < total {
        v.write(f.id, off, &block).unwrap();
        off += block.len() as u64;
    }
    v.flush().unwrap();
    let w = t.elapsed();
    let mut buf = vec![0u8; 1 << 20];
    let t = Instant::now();
    let mut off = 0;
    while off < total {
        off += v.read(f.id, off, &mut buf).unwrap() as u64;
    }
    let r = t.elapsed();
    let mb = (total >> 20) as f64;
    println!("write {:.0} MB/s, read {:.0} MB/s", mb / w.as_secs_f64(), mb / r.as_secs_f64());
    v.close().unwrap();
}

#[test]
fn meta_roundtrip_and_opacity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.nvault");
    let emblem = b"EMBLEMBYTES_r5t8".repeat(500);
    let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
    assert_eq!(v.meta_get("emblem").unwrap(), None);
    v.meta_set("emblem", Some(&emblem)).unwrap();
    assert_eq!(v.meta_get("emblem").unwrap().as_deref(), Some(&emblem[..]));
    assert!(matches!(v.meta_set("volume_id", Some(b"x")), Err(Error::InvalidName)));
    assert!(matches!(v.meta_set("big", Some(&vec![0u8; nv_core::MAX_META + 1])), Err(Error::InvalidName)));
    let scan = |when: &str| {
        for p in [path.clone(), side(&path, "-wal"), side(&path, "-shm"), side(&path, "-journal")] {
            let Ok(raw) = std::fs::read(&p) else { continue };
            assert!(!raw.windows(16).any(|w| w == b"EMBLEMBYTES_r5t8"), "{when}: emblem in {}", p.display());
        }
    };
    scan("while open");
    v.close().unwrap();
    scan("after close");
    let v = Vault::open(&path, &pw("pw")).unwrap();
    assert_eq!(v.meta_get("emblem").unwrap().as_deref(), Some(&emblem[..]));
    v.meta_set("emblem", None).unwrap();
    assert_eq!(v.meta_get("emblem").unwrap(), None);
    v.close().unwrap();
    let v = Vault::open(&path, &pw("pw")).unwrap();
    assert_eq!(v.meta_get("emblem").unwrap(), None);
    v.check_integrity().unwrap();
}

use nv_core::{Appearance, AppearanceError, LockedEmblem};

const COPY: usize = 65536;

fn look(accent: &str, emblem: LockedEmblem) -> Appearance {
    Appearance { accent: Some(accent.into()), emblem: Some(emblem) }
}

fn png(w: u32, h: u32, len: usize) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    v.extend_from_slice(&w.to_be_bytes());
    v.extend_from_slice(&h.to_be_bytes());
    v.resize(len.max(v.len()), 0x42);
    v
}

fn forge(path: &Path, from: &[u8], to: &[u8]) {
    let mut raw = std::fs::read(path).unwrap();
    for c in 0..2 {
        let copy = &mut raw[c * COPY..(c + 1) * COPY];
        let at = copy.windows(from.len()).position(|w| w == from).expect("bytes in header copy");
        copy[at..at + to.len()].copy_from_slice(to);
        let sum = blake3::hash(&copy[..COPY - 32]);
        copy[COPY - 32..].copy_from_slice(sum.as_bytes());
    }
    use std::io::{Seek, SeekFrom, Write};
    let mut f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.seek(SeekFrom::Start(0)).unwrap();
    f.write_all(&raw[..2 * COPY]).unwrap();
}

#[test]
fn appearance_peek_without_password() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.nvault");
    let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
    assert_eq!(v.appearance(), None);
    let p = nv_core::peek(&path).unwrap();
    assert_eq!(p.appearance, None, "a new volume has no section until the app writes one");
    assert_eq!(p.password_kdf.len(), 1);
    assert_eq!((p.password_kdf[0].m_kib, p.password_kdf[0].t), (8 * 1024, 1));
    assert!(p.unlock_ms.is_some_and(|ms| ms < 5_000), "{:?}", p.unlock_ms);

    let a = look("velluto", LockedEmblem::Icon("house".into()));
    v.set_appearance(Some(&a)).unwrap();
    assert_eq!(nv_core::peek_appearance(&path).unwrap(), Some(a.clone()), "readable while open");
    v.add_password(&pw("due"), KdfPreset::Fast, Some("Busta")).unwrap();
    v.close().unwrap();
    let p = nv_core::peek(&path).unwrap();
    assert_eq!(p.appearance, Some(a.clone()));
    assert_eq!(p.password_kdf.len(), 2);
    assert_eq!(p.recipient_slots, 0);
    let v = Vault::open(&path, &pw("due")).unwrap();
    assert_eq!(v.appearance(), Some(a));
    assert!(!v.appearance_tampered());
    v.close().unwrap();

    let junk = dir.path().join("junk.nvault");
    std::fs::write(&junk, pat(3, 300_000)).unwrap();
    assert_eq!(nv_core::peek(&junk).err(), Some(OpenError::NotAVault));
}

#[test]
fn appearance_tamper_is_detected_on_unlock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.nvault");
    let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
    let f = v.create(ROOT, "f", Kind::File).unwrap();
    v.write(f.id, 0, b"payload").unwrap();
    v.set_appearance(Some(&look("prussia", LockedEmblem::Icon("house".into())))).unwrap();
    v.close().unwrap();

    forge(&path, b"house", b"mouse");
    assert_eq!(nv_core::peek_appearance(&path).unwrap(), Some(look("prussia", LockedEmblem::Icon("mouse".into()))));
    let v = Vault::open(&path, &pw("pw")).expect("a forged appearance does not lock the user out");
    assert!(v.appearance_tampered());
    assert_eq!(v.appearance(), None, "the forged section is dropped");
    assert_eq!(read_all(&v, f.id), b"payload");
    v.close().unwrap();
    assert_eq!(nv_core::peek_appearance(&path).unwrap(), None, "and the header repaired without it");
    let v = Vault::open(&path, &pw("pw")).unwrap();
    assert!(!v.appearance_tampered());
    v.close().unwrap();

    let v = Vault::open(&path, &pw("pw")).unwrap();
    v.set_slot_label(0, Some("Principale")).unwrap();
    v.close().unwrap();
    let raw = std::fs::read(&path).unwrap();
    let mut fixed = raw[..2 * COPY].to_vec();
    for c in 0..2 {
        let copy = &mut fixed[c * COPY..(c + 1) * COPY];
        copy[60] ^= 1;
        let sum = blake3::hash(&copy[..COPY - 32]);
        copy[COPY - 32..].copy_from_slice(sum.as_bytes());
    }
    use std::io::{Seek, SeekFrom, Write};
    let mut f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    f.seek(SeekFrom::Start(0)).unwrap();
    f.write_all(&fixed).unwrap();
    drop(f);
    assert!(matches!(Vault::open(&path, &pw("pw")), Err(OpenError::Corrupt(_) | OpenError::WrongPassword)));
}

#[test]
fn appearance_caps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.nvault");
    let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
    let ok = look("ottanio", LockedEmblem::Image(png(128, 128, nv_core::MAX_LOCKED_IMAGE)));
    v.set_appearance(Some(&ok)).unwrap();
    for (bad, want) in [
        (look("ottanio", LockedEmblem::Image(png(128, 128, nv_core::MAX_LOCKED_IMAGE + 1))), "ImageTooLarge"),
        (look("ottanio", LockedEmblem::Image(png(129, 64, 500))), "ImageSide"),
        (look("ottanio", LockedEmblem::Image(b"GIF89a-not-allowed".to_vec())), "ImageFormat"),
        (look(&"a".repeat(nv_core::MAX_ACCENT_ID + 1), LockedEmblem::Initial("A".into())), "Accent"),
        (look("ottanio", LockedEmblem::Initial("AB".into())), "Initial"),
        (look("ottanio", LockedEmblem::Icon("../../x".into())), "Icon"),
    ] {
        let e = v.set_appearance(Some(&bad)).unwrap_err();
        assert!(format!("{e:?}").starts_with(want), "{e:?} vs {want}");
    }
    assert_eq!(v.appearance(), Some(ok), "a refused write leaves the header alone");
    for i in 1..8 {
        v.add_recipient(&Identity::generate().recipient(), Some(&"L".repeat(nv_core::MAX_LABEL))).unwrap();
        assert_eq!(v.slots().len(), i + 1);
    }
    v.close().unwrap();
    assert!(nv_core::peek(&path).unwrap().appearance.is_some());
    assert!(matches!(
        Vault::open(&path, &pw("pw")).unwrap().set_appearance(Some(&look("X", LockedEmblem::Initial(String::new())))),
        Err(AppearanceError::Accent)
    ));
}

#[test]
fn appearance_cleared_leaves_no_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("e.nvault");
    let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
    let mut img = png(96, 96, 4000);
    img.extend_from_slice(b"EMBLEMPIXELS_q9w2");
    v.set_appearance(Some(&Appearance { accent: Some("zz-accent-k4".into()), emblem: Some(LockedEmblem::Image(img)) })).unwrap();
    v.meta_set("nicevault.emblem", Some(b"{\"accent\":\"zz-accent-k4\",\"pix\":\"EMBLEMPIXELS_q9w2\"}")).unwrap();
    let raw = std::fs::read(&path).unwrap();
    assert!(raw.windows(12).any(|w| w == b"zz-accent-k4"), "visible: accent is plaintext");
    v.set_appearance(None).unwrap();
    v.flush().unwrap();
    for when in ["open", "closed"] {
        for p in [path.clone(), side(&path, "-wal")] {
            let Ok(raw) = std::fs::read(&p) else { continue };
            assert!(!raw.windows(12).any(|w| w == b"zz-accent-k4"), "{when}: accent bytes in {}", p.display());
            assert!(!raw.windows(17).any(|w| w == b"EMBLEMPIXELS_q9w2"), "{when}: emblem bytes in {}", p.display());
            assert!(!raw.windows(4).any(|w| w == b"\x89PNG"), "{when}: an image in {}", p.display());
        }
        if when == "open" {
            v.check_integrity().unwrap();
        }
    }
    v.close().unwrap();
    assert_eq!(nv_core::peek_appearance(&path).unwrap(), None);
}

#[test]
#[cfg(unix)]
fn appearance_two_copy_robustness() {
    use std::os::unix::fs::FileExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.nvault");
    let a = look("seppia", LockedEmblem::Initial("R".into()));
    let b = look("ardesia", LockedEmblem::Icon("leaf".into()));
    Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap().close().unwrap();
    for copy in [0usize, 1] {
        let v = Vault::open(&path, &pw("pw")).unwrap();
        v.set_appearance(Some(&a)).unwrap();
        v.close().unwrap();
        let before = std::fs::read(&path).unwrap()[..2 * COPY].to_vec();
        let v = Vault::open(&path, &pw("pw")).unwrap();
        v.set_appearance(Some(&b)).unwrap();
        v.close().unwrap();
        let after = std::fs::read(&path).unwrap()[..2 * COPY].to_vec();
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.write_all_at(&after, 0).unwrap();
        f.write_all_at(&before[copy * COPY..(copy + 1) * COPY], (copy * COPY) as u64).unwrap();
        drop(f);
        assert_eq!(nv_core::peek_appearance(&path).unwrap(), Some(b.clone()), "copy {copy}: newest seq wins");
        let v = Vault::open(&path, &pw("pw")).unwrap();
        assert_eq!(v.appearance(), Some(b.clone()));
        assert!(!v.appearance_tampered());
        v.close().unwrap();
        let now = std::fs::read(&path).unwrap();
        assert!(now[copy * COPY..(copy + 1) * COPY].windows(7).any(|w| w == b"ardesia"), "copy {copy} repaired");
        flip(&path, (copy * COPY + 30) as u64);
        assert_eq!(nv_core::peek_appearance(&path).unwrap(), Some(b.clone()), "torn copy {copy}");
        Vault::open(&path, &pw("pw")).unwrap().close().unwrap();
    }
}
