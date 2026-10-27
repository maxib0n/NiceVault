use super::*;
use crate::header::STORE_AT;
use crate::single::{self, Map, crash};
use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering::SeqCst;

fn pw() -> SecretString {
    SecretString::from("pw".to_owned())
}

fn pat(seed: u8, len: usize) -> Vec<u8> {
    let mut x = (seed as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

fn read_file(v: &Vault, id: NodeId) -> Vec<u8> {
    let size = v.getattr(id).unwrap().size as usize;
    let mut out = vec![0u8; size];
    let mut off = 0;
    while off < size {
        let n = v.read(id, off as u64, &mut out[off..]).unwrap();
        assert!(n > 0);
        off += n;
    }
    out
}

fn snapshot(v: &Vault) -> BTreeMap<String, Vec<u8>> {
    v.readdir(ROOT).unwrap().into_iter().filter(|n| n.kind == Kind::File).map(|n| (n.name.clone(), read_file(v, n.id))).collect()
}

fn others(dir: &Path, keep: &str) -> Vec<String> {
    std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != keep).collect()
}

fn create_single(path: &Path, region: u64) -> Vault {
    Vault::create_full(path, &pw(), KdfPreset::Fast, None, None, JOURNAL, Some(region)).unwrap()
}

fn header_of(path: &Path) -> Header {
    read_headers(&File::open(path).unwrap()).unwrap().0
}

fn put(v: &Vault, name: &str, data: &[u8]) -> NodeId {
    let f = match v.lookup(ROOT, name) {
        Ok(n) => n,
        Err(_) => v.create(ROOT, name, Kind::File).unwrap(),
    };
    v.set_size(f.id, 0).unwrap();
    for (k, c) in data.chunks(1 << 20).enumerate() {
        v.write(f.id, (k << 20) as u64, c).unwrap();
    }
    f.id
}

fn small_cache(v: &Vault) {
    v.with(|i| i.conn.execute_batch("PRAGMA cache_size = 4").map_err(dberr)).unwrap();
}

#[test]
fn single_file_volume_is_one_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one.nvault");
    let v = create_single(&path, single::DEFAULT_REGION);
    let h = header_of(&path);
    assert_eq!((h.body.version, h.layout, h.fixed, h.journal), (header::VERSION_LAYOUT, Some(Layout::LEGACY), None, Some(JournalRegion::fresh(single::DEFAULT_REGION))));
    assert!(v.is_single_file() && !v.can_upgrade_format());
    let a = pat(1, 3 << 20);
    let fa = put(&v, "a", &a);
    put(&v, "b", &pat(2, 100_000));
    v.flush().unwrap();
    assert_eq!(others(dir.path(), "one.nvault"), Vec::<String>::new(), "while open");
    put(&v, "b", &pat(3, 2_000_000));
    v.remove(fa).unwrap();
    v.flush().unwrap();
    assert_eq!(others(dir.path(), "one.nvault"), Vec::<String>::new());
    v.close().unwrap();
    assert_eq!(others(dir.path(), "one.nvault"), Vec::<String>::new(), "after close");
    let v = Vault::open(&path, &pw()).unwrap();
    assert_eq!(snapshot(&v), BTreeMap::from([("b".to_owned(), pat(3, 2_000_000))]));
    v.check_integrity().unwrap();
    v.close().unwrap();
    assert!(nv_core_peek_ok(&path));
}

fn nv_core_peek_ok(path: &Path) -> bool {
    crate::peek(path).is_ok()
}

#[test]
fn big_transactions_use_the_tail_and_give_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.nvault");
    let v = create_single(&path, JournalRegion::MIN);
    let map = Map::new(header_of(&path).journal.unwrap(), None);
    put(&v, "old", &pat(1, 3 << 20));
    v.flush().unwrap();
    let c0 = io_counters();
    let moves0 = crash::MOVES.load(SeqCst);
    small_cache(&v);
    put(&v, "old", &pat(2, 3 << 20));
    put(&v, "new", &pat(3, 2 << 20));
    v.flush().unwrap();
    let c = io_counters().since(&c0);
    assert!(c.write_bytes[FileKind::Journal as usize] > 2 << 20, "{c:?}");
    assert!(crash::MOVES.load(SeqCst) > moves0, "the tail never moved");
    let rec = single::read_record(&File::open(&path).unwrap()).unwrap();
    assert_eq!(rec.tail, 0, "tail given back after the commit: {rec:?}");
    assert!(rec.jsize <= map.jbase());
    assert_eq!(std::fs::metadata(&path).unwrap().len(), map.db_end(rec.logical), "file cut back to the database");
    assert_eq!(others(dir.path(), "big.nvault"), Vec::<String>::new());
    v.close().unwrap();
    let v = Vault::open(&path, &pw()).unwrap();
    assert_eq!(snapshot(&v), BTreeMap::from([("new".to_owned(), pat(3, 2 << 20)), ("old".to_owned(), pat(2, 3 << 20))]));
    v.check_integrity().unwrap();
    v.remove(v.lookup(ROOT, "old").unwrap().id).unwrap();
    v.flush().unwrap();
    v.close().unwrap();
    let v = Vault::open(&path, &pw()).unwrap();
    assert_eq!(snapshot(&v).len(), 1);
    v.check_integrity().unwrap();
}

#[test]
fn upgrade_in_place_small_and_large() {
    for (data, region) in [(300_000usize, single::DEFAULT_REGION), (3 << 20, JournalRegion::MIN), (6 << 20, single::DEFAULT_REGION)] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.nvault");
        let v = Vault::create_with(&path, &pw(), KdfPreset::Fast, None, None, JOURNAL).unwrap();
        put(&v, "f", &pat(4, data));
        v.flush().unwrap();
        v.add_password(&SecretString::from("second".to_owned()), KdfPreset::Fast, Some("due")).unwrap();
        assert!(!v.is_single_file() && v.can_upgrade_format());
        assert_eq!(header_of(&path).body.version, header::VERSION);
        let before = std::fs::metadata(&path).unwrap().len();
        let t = Instant::now();
        assert!(v.upgrade_format_with(region).unwrap());
        let took = t.elapsed();
        assert!(v.is_single_file());
        assert_eq!(snapshot(&v)["f"], pat(4, data));
        put(&v, "g", &pat(5, 500_000));
        v.flush().unwrap();
        assert!(!v.upgrade_format_with(region).unwrap(), "already single-file");
        v.close().unwrap();
        assert_eq!(others(dir.path(), "old.nvault"), Vec::<String>::new());
        let h = header_of(&path);
        let j = h.journal.unwrap();
        assert_eq!((h.body.version, h.layout), (header::VERSION_LAYOUT, Some(Layout::LEGACY)));
        assert_eq!(j.region, region);
        assert_eq!(j.split > j.region, before - STORE_AT > region, "{j:?}");
        let len = std::fs::metadata(&path).unwrap().len();
        assert!(len <= before + 2 * region + 600_000, "{before} -> {len}");
        eprintln!("upgrade {data} B: {took:?}, {before} -> {len} bytes, {j:?}");
        for p in ["pw", "second"] {
            let v = Vault::open(&path, &SecretString::from(p.to_owned())).unwrap();
            assert_eq!(snapshot(&v), BTreeMap::from([("f".to_owned(), pat(4, data)), ("g".to_owned(), pat(5, 500_000))]));
            v.check_integrity().unwrap();
            assert!(v.slots().iter().any(|s| s.label.as_deref() == Some("due")));
            v.close().unwrap();
        }
    }
}

#[test]
fn rotate_master_keeps_a_single_file_volume_single() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rot.nvault");
    let v = Vault::create_with(&path, &pw(), KdfPreset::Fast, None, None, JOURNAL).unwrap();
    put(&v, "f", &pat(6, 5 << 20));
    v.flush().unwrap();
    v.upgrade_format().unwrap();
    v.rotate_master(0, &pw()).unwrap();
    assert!(v.is_single_file());
    assert_eq!(snapshot(&v)["f"], pat(6, 5 << 20));
    v.close().unwrap();
    assert_eq!(others(dir.path(), "rot.nvault"), Vec::<String>::new());
    assert_eq!(header_of(&path).journal, Some(JournalRegion::fresh(single::DEFAULT_REGION)));
    let v = Vault::open(&path, &pw()).unwrap();
    assert_eq!(snapshot(&v)["f"], pat(6, 5 << 20));
    v.check_integrity().unwrap();
}

#[test]
fn new_volumes_are_single_file() {
    const { assert!(NEW_VOLUMES_SINGLE_FILE) };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("n.nvault");
    let v = Vault::create(&path, &pw(), KdfPreset::Fast).unwrap();
    assert!(v.is_single_file() && !v.can_upgrade_format());
    put(&v, "f", &pat(1, 1 << 20));
    v.flush().unwrap();
    assert_eq!(others(dir.path(), "n.nvault"), Vec::<String>::new());
    v.close().unwrap();
    let h = header_of(&path);
    assert_eq!((h.body.version, h.layout, h.journal), (header::VERSION_LAYOUT, Some(Layout::LEGACY), Some(JournalRegion::fresh(single::DEFAULT_REGION))));
    let fixed = dir.path().join("f.nvault");
    let v = Vault::create_fixed(&fixed, &pw(), KdfPreset::Fast, 8 << 20).unwrap();
    assert!(v.is_single_file() && !v.can_upgrade_format());
    put(&v, "f", &pat(2, 1 << 20));
    v.flush().unwrap();
    assert_eq!(others(dir.path(), "n.nvault").into_iter().filter(|n| n.starts_with("f.")).collect::<Vec<_>>(), vec!["f.nvault".to_owned()]);
    v.close().unwrap();
    let h = header_of(&fixed);
    assert_eq!((h.fixed, h.journal), (Some(Fixed { capacity: 8 << 20 }), Some(JournalRegion::fresh_fixed(1 << 20))));
    let old = dir.path().join("o.nvault");
    let v = Vault::create_with(&old, &pw(), KdfPreset::Fast, None, None, JOURNAL).unwrap();
    assert!(!v.is_single_file() && v.can_upgrade_format());
    v.close().unwrap();
    assert_eq!(header_of(&old).body.version, header::VERSION);
}

fn create_fixed_side(path: &Path, cap: u64) -> Vault {
    Vault::create_fixed_as(path, &pw(), KdfPreset::Fast, cap, false).unwrap()
}

#[test]
fn fixed_volumes_upgrade_in_place() {
    const MIB: u64 = 1 << 20;
    for (cap, data, region) in [(24 * MIB, 300_000usize, 2 * MIB), (24 * MIB, 6 << 20, 2 * MIB), (64 * MIB, 9 << 20, single::DEFAULT_REGION), (6 * MIB, 1 << 20, 512 << 10)] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fx.nvault");
        let v = create_fixed_side(&path, cap);
        put(&v, "f", &pat(4, data));
        v.flush().unwrap();
        v.add_password(&SecretString::from("second".to_owned()), KdfPreset::Fast, Some("due")).unwrap();
        assert!(!v.is_single_file() && v.can_upgrade_format(), "{cap} {data}");
        let free0 = v.stats().unwrap().free_bytes;
        assert!(v.upgrade_format().unwrap());
        assert!(v.is_single_file() && !v.can_upgrade_format());
        assert_eq!(v.capacity(), Some(cap));
        assert_eq!(snapshot(&v)["f"], pat(4, data));
        let free1 = v.stats().unwrap().free_bytes;
        assert!(free1 + region >= free0 && free1 < free0, "{free0} -> {free1}");
        put(&v, "g", &pat(5, 500_000));
        v.flush().unwrap();
        v.close().unwrap();
        assert_eq!(others(dir.path(), "fx.nvault"), Vec::<String>::new());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), cap);
        let h = header_of(&path);
        let j = h.journal.unwrap();
        assert_eq!((h.fixed, j.region), (Some(Fixed { capacity: cap }), region), "{cap} {data}");
        assert_eq!(j.split > j.displaced(true), data as u64 > region, "{j:?}");
        for p in ["pw", "second"] {
            let v = Vault::open(&path, &SecretString::from(p.to_owned())).unwrap();
            assert_eq!(snapshot(&v), BTreeMap::from([("f".to_owned(), pat(4, data)), ("g".to_owned(), pat(5, 500_000))]));
            v.check_integrity().unwrap();
            v.close().unwrap();
        }
    }
}

#[test]
fn full_fixed_volume_upgrades_after_extending() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("full.nvault");
    let v = create_fixed_side(&path, 8 << 20);
    put(&v, "f", &pat(6, 7_000_000));
    v.flush().unwrap();
    assert!(!v.can_upgrade_format());
    let before = std::fs::read(&path).unwrap();
    assert!(matches!(v.upgrade_format(), Err(Error::Io(m)) if m == UPGRADE_NO_ROOM));
    assert!(std::fs::read(&path).unwrap() == before, "nothing changed");
    assert!(!v.is_single_file());
    v.close().unwrap();
    let v = Vault::open(&path, &pw()).unwrap();
    v.resize(16 << 20, &|_| {}).unwrap();
    assert!(v.can_upgrade_format());
    assert!(v.upgrade_format().unwrap());
    assert_eq!(snapshot(&v)["f"], pat(6, 7_000_000));
    v.close().unwrap();
}

#[test]
fn fixed_volume_big_delete_when_nearly_full() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nf.nvault");
    let cap = 24u64 << 20;
    let v = Vault::create_fixed(&path, &pw(), KdfPreset::Fast, cap).unwrap();
    let big = put(&v, "big", &pat(7, 16 << 20));
    put(&v, "small", &pat(8, 2 << 20));
    v.flush().unwrap();
    assert!(v.stats().unwrap().free_bytes < 4 << 20);
    v.remove(big).unwrap();
    v.flush().unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), cap, "cut back after the commit");
    assert!(v.stats().unwrap().free_bytes > 16 << 20);
    put(&v, "again", &pat(9, 12 << 20));
    v.close().unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), cap);
    assert_eq!(others(dir.path(), "nf.nvault"), Vec::<String>::new());
    let v = Vault::open(&path, &pw()).unwrap();
    assert_eq!(snapshot(&v), BTreeMap::from([("again".to_owned(), pat(9, 12 << 20)), ("small".to_owned(), pat(8, 2 << 20))]));
    v.check_integrity().unwrap();
}

#[test]
fn rotate_master_keeps_a_fixed_single_file_volume() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rf.nvault");
    let v = create_fixed_side(&path, 32 << 20);
    put(&v, "f", &pat(6, 5 << 20));
    v.flush().unwrap();
    v.upgrade_format().unwrap();
    v.rotate_master(0, &pw()).unwrap();
    assert!(v.is_single_file());
    assert_eq!(snapshot(&v)["f"], pat(6, 5 << 20));
    v.close().unwrap();
    assert_eq!(others(dir.path(), "rf.nvault"), Vec::<String>::new());
    let h = header_of(&path);
    assert_eq!((h.fixed, h.journal), (Some(Fixed { capacity: 32 << 20 }), Some(JournalRegion::fresh_fixed(single::DEFAULT_REGION))));
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 32 << 20);
    let v = Vault::open(&path, &pw()).unwrap();
    assert_eq!(snapshot(&v)["f"], pat(6, 5 << 20));
    v.check_integrity().unwrap();
}

fn states() -> [BTreeMap<String, Vec<u8>>; 3] {
    let a = BTreeMap::from([("a".to_owned(), pat(10, 300_000)), ("b".to_owned(), pat(11, 200_000))]);
    let b = BTreeMap::from([("a".to_owned(), pat(12, 300_000)), ("c".to_owned(), pat(13, 600_000))]);
    let mut a_c = pat(12, 300_000);
    a_c.truncate(100_000);
    let c = BTreeMap::from([("a".to_owned(), a_c), ("d".to_owned(), pat(14, 500_000)), ("e".to_owned(), pat(13, 600_000))]);
    [a, b, c]
}

fn workload(v: &Vault) {
    small_cache(v);
    put(v, "a", &pat(12, 300_000));
    put(v, "c", &pat(13, 600_000));
    v.remove(v.lookup(ROOT, "b").unwrap().id).unwrap();
    v.flush().unwrap();
    let a = v.lookup(ROOT, "a").unwrap();
    v.set_size(a.id, 100_000).unwrap();
    put(v, "d", &pat(14, 500_000));
    let c = v.lookup(ROOT, "c").unwrap();
    v.rename(c.id, ROOT, "e").unwrap();
    v.flush().unwrap();
}

fn base_single(dir: &Path) -> PathBuf {
    let p = dir.join("base.nvault");
    let v = create_single(&p, JournalRegion::MIN);
    for (k, d) in &states()[0] {
        put(&v, k, d);
    }
    v.close().unwrap();
    p
}

#[test]
fn crash_child() {
    let Ok(path) = std::env::var("NV_CRASH_VOL") else { return };
    if std::env::var("NV_CRASH_JOB").as_deref() == Ok("create-fixed") {
        crash::start();
        let v = Vault::create_fixed(Path::new(&path), &pw(), KdfPreset::Fast, 12 << 20).unwrap();
        put(&v, "a", &pat(21, 300_000));
        v.close().unwrap();
        println!("TICKS={}", crash::TICKS.load(SeqCst));
        return;
    }
    let v = Vault::open(Path::new(&path), &pw()).unwrap();
    crash::start();
    match std::env::var("NV_CRASH_JOB").as_deref() {
        Ok("migrate") => {
            v.upgrade_format_with(JournalRegion::MIN).unwrap();
            put(&v, "after", &pat(20, 50_000));
            v.flush().unwrap();
        }
        Ok("resize") => {
            let cap = v.capacity().unwrap();
            v.resize(cap + (8 << 20), &|_| {}).unwrap();
            let info = v.resize_info().unwrap();
            v.resize(info.min_in_place.max(cap + (4 << 20)), &|_| {}).unwrap();
            v.remove(v.lookup(ROOT, "pad").unwrap().id).unwrap();
            v.flush().unwrap();
            let info = v.resize_info().unwrap();
            assert!(info.min_capacity < info.min_in_place, "{info:?}");
            v.resize(info.min_capacity, &|_| {}).unwrap();
        }
        _ => workload(&v),
    }
    v.close().unwrap();
    println!("TICKS={} OUTSIDE={}", crash::TICKS.load(SeqCst), crash::OUTSIDE.load(SeqCst));
}

fn run_child(vol: &Path, job: &str, at: Option<u64>, lose: Option<u64>) -> Option<u64> {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["single_tests::crash_child", "--exact", "--nocapture", "--test-threads=1"])
        .env("NV_CRASH_VOL", vol)
        .env("NV_CRASH_JOB", job)
        .stderr(if at.is_none() { Stdio::piped() } else { Stdio::null() });
    if let Some(n) = at {
        c.env("NV_CRASH_AT", n.to_string());
    }
    if let Some(s) = lose {
        c.env("NV_CRASH_LOSE", s.to_string());
    }
    let out = c.output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    match at {
        None => {
            let err = String::from_utf8_lossy(&out.stderr);
            let err: Vec<&str> = err.lines().filter(|l| !l.contains("ERROR CORE")).collect();
            assert!(out.status.success(), "full run failed: {text}\n{}", err.join("\n"));
            text.split("TICKS=").nth(1).map(|t| t.split_whitespace().next().unwrap().parse().unwrap())
        }
        Some(_) => {
            if out.status.success() {
                None
            } else {
                Some(0)
            }
        }
    }
}

fn check_state(vol: &Path, allowed: &[BTreeMap<String, Vec<u8>>], what: &str) -> usize {
    let v = Vault::open(vol, &pw()).unwrap_or_else(|e| panic!("{what}: does not open: {e}"));
    v.check_integrity().unwrap_or_else(|e| panic!("{what}: {e}"));
    let got = snapshot(&v);
    let i = allowed.iter().position(|s| *s == got).unwrap_or_else(|| {
        panic!("{what}: state is none of the committed ones: {:?}", got.iter().map(|(k, d)| (k, d.len())).collect::<Vec<_>>())
    });
    put(&v, "z", b"after the crash");
    v.flush().unwrap();
    v.close().unwrap();
    i
}

fn crash_matrix(job: &str, base: &Path, allowed: &[BTreeMap<String, Vec<u8>>], one_file_after: bool) {
    let dir = base.parent().unwrap();
    let probe = dir.join("probe.nvault");
    std::fs::copy(base, &probe).unwrap();
    let total = run_child(&probe, job, None, None).unwrap();
    assert!(total > 10, "{total}");
    let step = (total / 50).max(1);
    let mut seen = vec![0usize; allowed.len()];
    let mut n = 0;
    while n <= total + 1 {
        for lose in [None, Some(n * 7 + 1)] {
            let vol = dir.join(format!("c{n}-{}.nvault", lose.is_some()));
            std::fs::copy(base, &vol).unwrap();
            let what = format!("{job} crash at {n}/{total} lose={lose:?}");
            if run_child(&vol, job, Some(n), lose).is_none() {
                assert!(n + 1 + total / 10 >= total, "{what}: the child finished early");
            }
            let name = vol.file_name().unwrap().to_string_lossy().into_owned();
            seen[check_state(&vol, allowed, &what)] += 1;
            if one_file_after || header_of(&vol).journal.is_some() {
                let left: Vec<String> = others(dir, "").into_iter().filter(|f| f.starts_with(&name) && *f != name).collect();
                assert!(left.is_empty(), "{what}: side files {left:?}");
            }
            std::fs::remove_file(&vol).unwrap();
        }
        n += step;
    }
    eprintln!("{job}: {total} crash points (step {step}), committed states seen: {seen:?}");
    assert!(seen.iter().filter(|&&s| s > 0).count() >= 2, "{seen:?}");
}

#[test]
fn crash_during_writes() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_single(dir.path());
    crash_matrix("write", &base, &states(), true);
}

#[test]
fn crash_during_migration() {
    for size in [200_000usize, 1_500_000] {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base.nvault");
        let v = Vault::create_with(&base, &pw(), KdfPreset::Fast, None, None, JOURNAL).unwrap();
        put(&v, "m", &pat(30, size));
        v.close().unwrap();
        let a = BTreeMap::from([("m".to_owned(), pat(30, size))]);
        let mut b = a.clone();
        b.insert("after".to_owned(), pat(20, 50_000));
        let probe = dir.path().join("probe.nvault");
        std::fs::copy(&base, &probe).unwrap();
        let total = run_child(&probe, "migrate", None, None).unwrap();
        let mut formats = [0, 0];
        for n in 0..=total {
            let vol = dir.path().join(format!("m{n}.nvault"));
            std::fs::copy(&base, &vol).unwrap();
            run_child(&vol, "migrate", Some(n), None);
            let what = format!("migration {size} crash at {n}/{total}");
            let raw = std::fs::read(&vol).unwrap();
            for c in 0..2 {
                if let Ok(h) = header::decode_copy(&raw[c * COPY_LEN..(c + 1) * COPY_LEN])
                    && h.journal.is_none()
                {
                    let only = dir.path().join(format!("m{n}-v1-{c}.nvault"));
                    let mut r = raw.clone();
                    r[(1 - c) * COPY_LEN..(2 - c) * COPY_LEN].fill(0);
                    std::fs::write(&only, r).unwrap();
                    check_state(&only, &[a.clone(), b.clone()], &format!("{what}, v1 copy {c}"));
                    std::fs::remove_file(&only).unwrap();
                }
            }
            check_state(&vol, &[a.clone(), b.clone()], &what);
            formats[header_of(&vol).journal.is_some() as usize] += 1;
            std::fs::remove_file(&vol).unwrap();
            let _ = std::fs::remove_file(dir.path().join(format!("m{n}.nvault-journal")));
        }
        eprintln!("migration {size}: {total} crash points, v1/v2 after: {formats:?}");
        assert!(formats[0] > 0 && formats[1] > 0);
    }
}

fn base_fixed(dir: &Path, cap: u64, tight: bool) -> PathBuf {
    let p = dir.join("base.nvault");
    let v = Vault::create_full(&p, &pw(), KdfPreset::Fast, Some(DEFAULT_LAYOUT), Some(Fixed { capacity: cap }), JOURNAL, Some(JournalRegion::MIN)).unwrap();
    for (k, d) in &states()[0] {
        put(&v, k, d);
    }
    if tight {
        let sub = v.create(ROOT, "pieno", Kind::Dir).unwrap();
        let room = v.stats().unwrap().free_bytes;
        let f1 = v.create(sub.id, "liberato", Kind::File).unwrap();
        v.write(f1.id, 0, &pat(50, 2_500_000)).unwrap();
        let f2 = v.create(sub.id, "riempitivo", Kind::File).unwrap();
        v.write(f2.id, 0, &pat(51, ((room - 2_500_000) * 15 / 16 - (400 << 10)) as usize)).unwrap();
        v.flush().unwrap();
        v.remove(f1.id).unwrap();
    }
    v.close().unwrap();
    assert_eq!(std::fs::metadata(&p).unwrap().len(), cap);
    p
}

#[test]
fn crash_during_writes_fixed() {
    for (cap, tight) in [(24u64 << 20, false), (6 << 20, true)] {
        let dir = tempfile::tempdir().unwrap();
        let base = base_fixed(dir.path(), cap, tight);
        let p = dir.path().join("outside.nvault");
        std::fs::copy(&base, &p).unwrap();
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["single_tests::crash_child", "--exact", "--nocapture", "--test-threads=1"])
            .env("NV_CRASH_VOL", &p)
            .env("NV_CRASH_JOB", "write")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let outside: u64 = text.split("OUTSIDE=").nth(1).and_then(|t| t.split_whitespace().next()?.parse().ok()).unwrap_or_else(|| panic!("{text}\n{}", String::from_utf8_lossy(&out.stderr)));
        assert_eq!(outside > 0, tight, "{cap}: {outside}");
        std::fs::remove_file(&p).unwrap();
        crash_matrix("write", &base, &states(), true);
        for e in std::fs::read_dir(dir.path()).unwrap().flatten() {
            assert!(e.file_name() == "base.nvault" || e.file_name() == "probe.nvault", "{:?}", e.file_name());
        }
        assert_eq!(std::fs::metadata(dir.path().join("probe.nvault")).unwrap().len(), cap, "cut back to its capacity");
    }
}

#[test]
fn crash_during_create_fixed() {
    let dir = tempfile::tempdir().unwrap();
    let probe = dir.path().join("probe.nvault");
    let total = run_child(&probe, "create-fixed", None, None).unwrap();
    let (mut opened, mut refused) = (0, 0);
    for n in 0..=total {
        for lose in [None, Some(n * 5 + 3)] {
            let vol = dir.path().join(format!("k{n}-{}.nvault", lose.is_some()));
            run_child(&vol, "create-fixed", Some(n), lose);
            let what = format!("create crash at {n}/{total} lose={lose:?}");
            let name = vol.file_name().unwrap().to_string_lossy().into_owned();
            let left: Vec<String> = others(dir.path(), "").into_iter().filter(|f| f.starts_with(&name) && *f != name).collect();
            assert!(left.is_empty(), "{what}: side files {left:?}");
            if !vol.exists() {
                refused += 1;
                continue;
            }
            match Vault::open(&vol, &pw()) {
                Ok(v) => {
                    v.check_integrity().unwrap_or_else(|e| panic!("{what}: {e}"));
                    let snap = snapshot(&v);
                    assert!(snap.is_empty() || snap == BTreeMap::from([("a".to_owned(), pat(21, 300_000))]), "{what}");
                    put(&v, "z", b"after the crash");
                    v.close().unwrap();
                    opened += 1;
                }
                Err(e) => {
                    assert!(matches!(e, OpenError::Corrupt(_) | OpenError::NotAVault), "{what}: {e:?}");
                    refused += 1;
                }
            }
            std::fs::remove_file(&vol).unwrap();
        }
    }
    eprintln!("create fixed: {total} crash points, opened {opened}, refused {refused}");
    assert!(opened > 0 && refused > 0);
}

#[test]
fn crash_during_fixed_migration() {
    for size in [200_000usize, 1_500_000] {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base.nvault");
        let cap = 12u64 << 20;
        let v = Vault::create_fixed_as(&base, &pw(), KdfPreset::Fast, cap, false).unwrap();
        put(&v, "m", &pat(30, size));
        v.close().unwrap();
        let a = BTreeMap::from([("m".to_owned(), pat(30, size))]);
        let mut b = a.clone();
        b.insert("after".to_owned(), pat(20, 50_000));
        let probe = dir.path().join("probe.nvault");
        std::fs::copy(&base, &probe).unwrap();
        let total = run_child(&probe, "migrate", None, None).unwrap();
        assert!(header_of(&probe).journal.is_some());
        let mut formats = [0, 0];
        for n in 0..=total {
            for lose in [None, Some(n * 3 + 1)] {
                let vol = dir.path().join(format!("m{n}-{}.nvault", lose.is_some()));
                std::fs::copy(&base, &vol).unwrap();
                run_child(&vol, "migrate", Some(n), lose);
                let what = format!("fixed migration {size} crash at {n}/{total} lose={lose:?}");
                let raw = std::fs::read(&vol).unwrap();
                assert_eq!(raw.len() as u64, cap, "{what}");
                for c in 0..2 {
                    if let Ok(h) = header::decode_copy(&raw[c * COPY_LEN..(c + 1) * COPY_LEN])
                        && h.journal.is_none()
                    {
                        let only = dir.path().join(format!("m{n}-old-{c}.nvault"));
                        let mut r = raw.clone();
                        r[(1 - c) * COPY_LEN..(2 - c) * COPY_LEN].fill(0);
                        std::fs::write(&only, r).unwrap();
                        check_state(&only, &[a.clone(), b.clone()], &format!("{what}, old copy {c}"));
                        std::fs::remove_file(&only).unwrap();
                        let _ = std::fs::remove_file(dir.path().join(format!("m{n}-old-{c}.nvault-journal")));
                    }
                }
                check_state(&vol, &[a.clone(), b.clone()], &what);
                assert_eq!(std::fs::metadata(&vol).unwrap().len(), cap, "{what}");
                formats[header_of(&vol).journal.is_some() as usize] += 1;
                std::fs::remove_file(&vol).unwrap();
                let _ = std::fs::remove_file(dir.path().join(format!("m{n}-{}.nvault-journal", lose.is_some())));
            }
        }
        eprintln!("fixed migration {size}: {total} crash points, old/new after: {formats:?}");
        assert!(formats[0] > 0 && formats[1] > 0);
    }
}

#[test]
fn crash_during_fixed_resize() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.nvault");
    let cap = 16u64 << 20;
    let v = Vault::create_fixed(&base, &pw(), KdfPreset::Fast, cap).unwrap();
    for (k, d) in &states()[0] {
        put(&v, k, d);
    }
    put(&v, "pad", &pat(51, 10 << 20));
    v.close().unwrap();
    let mut keep = states()[0].clone();
    keep.insert("pad".to_owned(), pat(51, 10 << 20));
    let gone = states()[0].clone();
    let probe = dir.path().join("probe.nvault");
    std::fs::copy(&base, &probe).unwrap();
    let total = run_child(&probe, "resize", None, None).unwrap();
    let step = (total / 60).max(1);
    let mut caps = std::collections::BTreeSet::new();
    let points: std::collections::BTreeSet<u64> = (0..80).chain((80..total).step_by(step as usize)).chain(total.saturating_sub(12)..=total + 1).collect();
    for n in points {
        for lose in [None, Some(n * 11 + 5)] {
            let vol = dir.path().join(format!("r{n}-{}.nvault", lose.is_some()));
            std::fs::copy(&base, &vol).unwrap();
            run_child(&vol, "resize", Some(n), lose);
            let what = format!("resize crash at {n}/{total} lose={lose:?}");
            let v = Vault::open(&vol, &pw()).unwrap_or_else(|e| panic!("{what}: {e}"));
            v.check_integrity().unwrap_or_else(|e| panic!("{what}: {e}"));
            let c = v.capacity().unwrap();
            assert_eq!(std::fs::metadata(&vol).unwrap().len(), c, "{what}");
            let snap = snapshot(&v);
            assert!(snap == keep || snap == gone, "{what}: {:?}", snap.keys());
            caps.insert(c);
            put(&v, "z", b"after the crash");
            v.close().unwrap();
            let name = vol.file_name().unwrap().to_string_lossy().into_owned();
            let left: Vec<String> = others(dir.path(), "").into_iter().filter(|f| f.starts_with(&name) && *f != name).collect();
            assert!(left.is_empty(), "{what}: side files {left:?}");
            std::fs::remove_file(&vol).unwrap();
        }
    }
    eprintln!("fixed resize: {total} crash points (step {step}), capacities seen: {caps:?}");
    assert!(caps.len() >= 4, "grown, shrunk in place, compacted: {caps:?}");
}

pub(crate) fn interrupted_committed() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([("tenuto.bin".to_owned(), pat(40, 700_000)), ("piccolo.txt".to_owned(), b"resta com'era\n".to_vec())])
}

#[test]
fn interrupted_child() {
    let Ok(path) = std::env::var("NV_INTERRUPTED_CHILD") else { return };
    let v = Vault::create_full(Path::new(&path), &SecretString::from("interrotto-pw".to_owned()), KdfPreset::Fast, None, None, JOURNAL, Some(JournalRegion::MIN)).unwrap();
    for (k, d) in interrupted_committed() {
        put(&v, &k, &d);
    }
    v.flush().unwrap();
    small_cache(&v);
    put(&v, "tenuto.bin", &pat(41, 700_000));
    put(&v, "perso.bin", &pat(42, 400_000));
    v.with(|i| i.spill()).unwrap();
    let rec = single::read_record(&File::open(&path).unwrap()).unwrap();
    assert!(rec.tail != 0 && rec.jsize > Map::new(JournalRegion::fresh(JournalRegion::MIN), None).jbase(), "{rec:?}");
    println!("HOT-JOURNAL");
    std::process::exit(134);
}

#[test]
#[ignore = "fixture generator"]
fn gen_interrupted_fixture() {
    let out = PathBuf::from(std::env::var("NV_INTERRUPTED_OUT").expect("NV_INTERRUPTED_OUT"));
    assert!(!out.exists());
    let st = Command::new(std::env::current_exe().unwrap())
        .args(["single_tests::interrupted_child", "--exact", "--nocapture", "--test-threads=1"])
        .env("NV_INTERRUPTED_CHILD", &out)
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert!(!st.status.success() && String::from_utf8_lossy(&st.stdout).contains("HOT-JOURNAL"), "the child did not leave a hot journal in the tail");
    let tmp = tempfile::tempdir().unwrap();
    let c = tmp.path().join("c.nvault");
    std::fs::copy(&out, &c).unwrap();
    let v = Vault::open(&c, &SecretString::from("interrotto-pw".to_owned())).unwrap();
    assert_eq!(snapshot(&v), interrupted_committed());
    v.check_integrity().unwrap();
}

#[test]
fn hot_in_file_journal_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("i.nvault");
    let st = Command::new(std::env::current_exe().unwrap())
        .args(["single_tests::interrupted_child", "--exact", "--nocapture", "--test-threads=1"])
        .env("NV_INTERRUPTED_CHILD", &out)
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert!(!st.status.success() && String::from_utf8_lossy(&st.stdout).contains("HOT-JOURNAL"), "the child did not leave a hot journal in the tail");
    let v = Vault::open(&out, &SecretString::from("interrotto-pw".to_owned())).unwrap();
    assert_eq!(snapshot(&v), interrupted_committed());
    v.check_integrity().unwrap();
    v.close().unwrap();
    assert_eq!(others(dir.path(), "i.nvault"), Vec::<String>::new());
    let rec = single::read_record(&File::open(&out).unwrap()).unwrap();
    assert_eq!((rec.tail, rec.jsize), (0, 0), "{rec:?}");
}

pub(crate) fn interrupted_fixed_committed() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        ("tenuto.bin".to_owned(), pat(40, 700_000)),
        ("piccolo.txt".to_owned(), b"resta com'era\n".to_vec()),
        ("pieno/riempitivo.bin".to_owned(), pat(52, 3_400_000)),
    ])
}

fn snapshot_deep(v: &Vault) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    for n in v.readdir(ROOT).unwrap() {
        match n.kind {
            Kind::File => {
                out.insert(n.name.clone(), read_file(v, n.id));
            }
            Kind::Dir => {
                for c in v.readdir(n.id).unwrap() {
                    out.insert(format!("{}/{}", n.name, c.name), read_file(v, c.id));
                }
            }
        }
    }
    out
}

#[test]
fn interrupted_fixed_child() {
    let Ok(path) = std::env::var("NV_INTERRUPTED_FIXED_CHILD") else { return };
    let cap = 6u64 << 20;
    let v = Vault::create_full(
        Path::new(&path),
        &SecretString::from("fisso-interrotto-pw".to_owned()),
        KdfPreset::Fast,
        Some(DEFAULT_LAYOUT),
        Some(Fixed { capacity: cap }),
        JOURNAL,
        Some(JournalRegion::MIN),
    )
    .unwrap();
    put(&v, "tenuto.bin", &pat(40, 700_000));
    put(&v, "piccolo.txt", b"resta com'era\n");
    let sub = v.create(ROOT, "pieno", Kind::Dir).unwrap();
    let freed = v.create(sub.id, "liberato.bin", Kind::File).unwrap();
    v.write(freed.id, 0, &pat(53, 1_000_000)).unwrap();
    let f = v.create(sub.id, "riempitivo.bin", Kind::File).unwrap();
    v.write(f.id, 0, &pat(52, 3_400_000)).unwrap();
    v.flush().unwrap();
    v.remove(freed.id).unwrap();
    v.flush().unwrap();
    assert_eq!(snapshot_deep(&v), interrupted_fixed_committed());
    small_cache(&v);
    put(&v, "tenuto.bin", &pat(41, 700_000));
    put(&v, "perso.bin", &pat(42, 400_000));
    v.with(|i| i.spill()).unwrap();
    let rec = single::read_record(&File::open(&path).unwrap()).unwrap();
    let len = std::fs::metadata(&path).unwrap().len();
    assert!(rec.tail >= cap && len > cap && rec.cap == cap, "{rec:?} {len}");
    println!("HOT-JOURNAL-PAST-CAPACITY");
    std::process::exit(134);
}

fn make_interrupted_fixed(out: &Path) {
    let st = Command::new(std::env::current_exe().unwrap())
        .args(["single_tests::interrupted_fixed_child", "--exact", "--nocapture", "--test-threads=1"])
        .env("NV_INTERRUPTED_FIXED_CHILD", out)
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        !st.status.success() && String::from_utf8_lossy(&st.stdout).contains("HOT-JOURNAL-PAST-CAPACITY"),
        "the child did not leave a hot journal past the capacity: {}",
        String::from_utf8_lossy(&st.stderr)
    );
}

#[test]
#[ignore = "fixture generator"]
fn gen_interrupted_fixed_fixture() {
    let out = PathBuf::from(std::env::var("NV_INTERRUPTED_FIXED_OUT").expect("NV_INTERRUPTED_FIXED_OUT"));
    assert!(!out.exists());
    make_interrupted_fixed(&out);
    let tmp = tempfile::tempdir().unwrap();
    let c = tmp.path().join("c.nvault");
    std::fs::copy(&out, &c).unwrap();
    let v = Vault::open(&c, &SecretString::from("fisso-interrotto-pw".to_owned())).unwrap();
    assert_eq!(snapshot_deep(&v), interrupted_fixed_committed());
    v.check_integrity().unwrap();
}

#[test]
fn hot_tail_past_the_capacity_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("fi.nvault");
    make_interrupted_fixed(&out);
    let v = Vault::open(&out, &SecretString::from("fisso-interrotto-pw".to_owned())).unwrap();
    assert_eq!(snapshot_deep(&v), interrupted_fixed_committed());
    v.check_integrity().unwrap();
    assert_eq!(std::fs::metadata(&out).unwrap().len(), 6 << 20, "cut back to the capacity");
    put(&v, "dopo.bin", &pat(43, 300_000));
    v.close().unwrap();
    assert_eq!(others(dir.path(), "fi.nvault"), Vec::<String>::new());
    let rec = single::read_record(&File::open(&out).unwrap()).unwrap();
    assert_eq!((rec.tail, rec.cap), (0, 6 << 20), "{rec:?}");
    let v = Vault::open(&out, &SecretString::from("fisso-interrotto-pw".to_owned())).unwrap();
    assert_eq!(snapshot_deep(&v).len(), 4);
}

#[test]
fn freshness_records_survive_fixed_upgrade_and_resize() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fr.nvault");
    let v = create_fixed_side(&path, 24 << 20);
    v.set_appearance(Some(&Appearance { accent: Some("ottanio".into()), emblem: None })).unwrap();
    let gap = put(&v, "gap", &pat(70, 10 << 20));
    put(&v, "keep", &pat(71, 1 << 20));
    v.flush().unwrap();
    let mut last = v.commit_seq().unwrap();
    let step = |v: &Vault, last: &mut u64, what: &str| {
        let c = v.commit_seq().unwrap();
        assert!(c >= *last, "{what}: commit seq went back {last} -> {c}");
        *last = c;
        assert!(!v.appearance_tampered(), "{what}");
        assert_eq!(v.appearance().and_then(|a| a.accent).as_deref(), Some("ottanio"), "{what}");
    };
    assert!(v.upgrade_format().unwrap());
    step(&v, &mut last, "upgrade");
    v.resize(32 << 20, &|_| {}).unwrap();
    step(&v, &mut last, "grow");
    v.resize(28 << 20, &|_| {}).unwrap();
    step(&v, &mut last, "shrink in place");
    v.remove(gap).unwrap();
    v.flush().unwrap();
    let info = v.resize_info().unwrap();
    assert!(info.min_capacity < info.min_in_place);
    v.resize(info.min_capacity, &|_| {}).unwrap();
    step(&v, &mut last, "compact");
    v.close().unwrap();
    let v = Vault::open(&path, &pw()).unwrap();
    step(&v, &mut last, "reopen");
    assert!(v.is_single_file());
    assert_eq!(snapshot(&v)["keep"], pat(71, 1 << 20));
}
