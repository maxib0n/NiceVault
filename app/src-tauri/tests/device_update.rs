use base64::Engine;
use nicevault_app::device::backend::{Fake, FakeFile};
use nicevault_app::device::client::Local;
use nicevault_app::device::job::{DeviceCtx, Log};
use nicevault_app::device::stick::{self, Change, Source};
use nicevault_app::device::{Disk, Marker, Os, Partition};
use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

#[derive(Clone, Default)]
struct Rec(Arc<Mutex<Vec<(String, Value)>>>);
impl Events for Rec {
    fn emit(&self, event: &str, payload: Value) {
        self.0.lock().unwrap().push((event.to_owned(), payload));
    }
}

struct Key {
    pk: minisign::PublicKey,
    sk: minisign::SecretKey,
}
impl Key {
    fn new() -> Key {
        let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        Key { pk: kp.pk, sk: kp.sk }
    }
    fn pubkey_b64(&self) -> String {
        B64.encode(self.pk.to_box().unwrap().to_string())
    }
    fn sign(&self, data: &[u8], version: &str, platforms: &str) -> String {
        let tc = format!("timestamp:1\tfile:x\tversion:{version}\tplatform:{platforms}");
        B64.encode(minisign::sign(Some(&self.pk), &self.sk, data, Some(&tc), Some("sig")).unwrap().to_string())
    }
}

fn app_zip() -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in [("NiceVault.app/Contents/Info.plist", &b"<plist/>"[..]), ("NiceVault.app/Contents/MacOS/nicevault", b"new mac binary")] {
        w.start_file(name, zip::write::SimpleFileOptions::default().unix_permissions(0o755)).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn serve(k: &Key, version: &str, bad: &[&str]) -> String {
    serve_with(k, version, bad, [b"channel linux".to_vec(), b"new windows exe".to_vec(), app_zip()])
}

fn serve_with(k: &Key, version: &str, bad: &[&str], [lin, win, mac]: [Vec<u8>; 3]) -> String {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}", server.server_addr().to_ip().unwrap());
    let builds = [("linux-x86_64", "NiceVault-linux-x86_64", lin), ("windows-x86_64", "NiceVault-windows-x86_64.exe", win), ("darwin-aarch64", "NiceVault-macos-universal.zip", mac)];
    let mut files: Vec<(String, Vec<u8>)> = vec![];
    let mut platforms = serde_json::Map::new();
    for (key, name, bytes) in builds {
        let signed: &[u8] = if bad.contains(&key) { b"something else" } else { &bytes };
        let plat = if key == "darwin-aarch64" { "darwin-aarch64,darwin-x86_64" } else { key };
        platforms.insert(key.into(), serde_json::json!({ "signature": k.sign(signed, version, plat), "url": format!("{url}/v/{name}") }));
        files.push((format!("/v/{name}"), bytes));
    }
    files.push(("/latest.json".into(), serde_json::to_vec(&serde_json::json!({ "version": version, "platforms": platforms })).unwrap()));
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let hit = files.iter().find(|(p, _)| p == req.url()).map(|(_, b)| b.clone());
            let _ = match hit {
                Some(b) => req.respond(tiny_http::Response::from_data(b)),
                None => req.respond(tiny_http::Response::empty(404)),
            };
        }
    });
    format!("{url}/latest.json")
}

struct World {
    t: tempfile::TempDir,
    file: PathBuf,
    client: PathBuf,
    data: PathBuf,
    rec: Rec,
    st: Arc<AppState>,
    ctx: Arc<DeviceCtx>,
}

fn world(manifest: String, pubkey: String, client_mounted: bool, exe_in_client: bool) -> World {
    let t = tempfile::tempdir().unwrap();
    let file = t.path().join("disks.json");
    let root = t.path().join("parts");
    let client = root.join("_dev_fake1").join("NiceVault");
    let data = t.path().join("media/VaultData");
    std::fs::create_dir_all(&client).unwrap();
    std::fs::create_dir_all(data.join("Lavoro")).unwrap();
    std::fs::write(data.join("Dati.nvault"), b"an encrypted volume").unwrap();
    std::fs::write(data.join("Lavoro/Altro.nvault"), b"another one").unwrap();
    std::fs::write(client.join("NiceVault-linux-x86_64"), b"old linux").unwrap();
    std::fs::write(client.join("NiceVault-windows-x86_64.exe"), b"old windows").unwrap();
    std::fs::write(client.join("NiceVault-windows-x86_64.exe.sig"), b"old sig").unwrap();
    std::fs::write(client.join("README.txt"), b"old readme").unwrap();
    let clients: BTreeMap<String, Option<String>> = [("linux", Some("0.0.5")), ("windows", Some("0.0.5")), ("macos", None)].into_iter().map(|(k, v)| (k.to_owned(), v.map(str::to_owned))).collect();
    let marker = Marker { format: 1, created: 1, app_version: "0.0.5".into(), data_fs: "exfat".into(), device_id: "ab".repeat(16), table: None, clients, updated: None, mac_app: None };
    std::fs::write(client.join(".nicevault-device.json"), serde_json::to_vec_pretty(&marker).unwrap()).unwrap();
    let disk = Disk {
        id: "/dev/fake1".into(),
        os_path: "/dev/fake1".into(),
        vendor: "SanDisk".into(),
        model: "Ultra".into(),
        serial: "4C530001071103235".into(),
        bus: "usb".into(),
        size: 32 << 30,
        removable: true,
        partitions: vec![
            Partition { path: "/dev/fake1p1".into(), label: "NiceVault".into(), fs: "exfat".into(), size: 128 << 20, mountpoints: if client_mounted { vec![client.to_string_lossy().into_owned()] } else { vec![] } },
            Partition { path: "/dev/fake1p2".into(), label: "VaultData".into(), fs: "exfat".into(), size: 31 << 30, mountpoints: vec![data.to_string_lossy().into_owned()] },
        ],
        ..Default::default()
    };
    let other = Disk { id: "/dev/fake2".into(), partitions: vec![Partition { path: "/dev/fake2p1".into(), label: "STICK".into(), fs: "vfat".into(), size: 1 << 30, mountpoints: vec![t.path().join("media/STICK").to_string_lossy().into_owned()] }], ..Default::default() };
    let f = FakeFile { disks: vec![disk, other], root: root.to_string_lossy().into_owned(), step_ms: Some(10), ..Default::default() };
    std::fs::write(&file, serde_json::to_vec(&f).unwrap()).unwrap();
    let exe = if exe_in_client {
        client.join("NiceVault-linux-x86_64")
    } else {
        let e = t.path().join("bin/nicevault");
        std::fs::create_dir_all(e.parent().unwrap()).unwrap();
        std::fs::write(&e, b"running linux 0.1.0").unwrap();
        e
    };
    let rec = Rec::default();
    let st = AppState::new(Store::new(Locations { exe_dir: None, removable: Some(vec![]), config_dir: None }), Box::new(rec.clone()));
    let ctx = Arc::new(DeviceCtx {
        ops: Box::new(Fake::new(file.clone())),
        plans: Default::default(),
        outcomes: Default::default(),
        logs: Default::default(),
        log_dir: t.path().join("logs"),
        manifest_url: manifest,
        local: Local { exe, os: Os::Linux, version: "0.1.0".into(), pubkey },
    });
    World { t, file, client, data, rec, st, ctx }
}

fn run(w: &World) -> Result<stick::UpdateOutcome, nicevault_app::error::ApiError> {
    stick::execute(&w.ctx, &w.st, "job1", &AtomicBool::new(false), "/dev/fake1", &Log::null())
}

fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut out = vec![];
    for e in walk(dir) {
        let m = std::fs::metadata(&e).unwrap();
        out.push((e.clone(), std::fs::read(&e).unwrap(), m.modified().unwrap()));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut v = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.file_type().unwrap().is_dir() {
            v.extend(walk(&e.path()));
        } else {
            v.push(e.path());
        }
    }
    v
}
fn leftovers(dir: &Path) -> Vec<PathBuf> {
    walk(dir).into_iter().chain(std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path())).filter(|p| p.to_string_lossy().ends_with(".old") || p.to_string_lossy().ends_with(".part")).collect()
}
fn marker(w: &World) -> Marker {
    serde_json::from_slice(&std::fs::read(w.client.join(".nicevault-device.json")).unwrap()).unwrap()
}
fn prog(s: &stick::Stick, os: Os) -> &stick::StickOs {
    s.programs.iter().find(|p| p.os == os).unwrap()
}

#[test]
fn detection_is_read_only_and_finds_the_device_of_a_volume() {
    let w = world("http://127.0.0.1:9/latest.json".into(), Key::new().pubkey_b64(), true, false);
    let before = snapshot(w.t.path());
    let all = stick::sticks(w.ctx.ops.as_ref(), &w.ctx.local, None);
    assert_eq!(all.len(), 1, "only the drive with both partitions");
    let s = &stick::sticks(w.ctx.ops.as_ref(), &w.ctx.local, Some(&w.data.join("Lavoro/Altro.nvault")))[0];
    assert_eq!((s.disk_id.as_str(), s.name.as_str(), s.mounted, s.complete, s.outdated), ("/dev/fake1", "SanDisk Ultra", true, true, true));
    assert_eq!(s.running_version, "0.1.0");
    let l = prog(s, Os::Linux);
    assert_eq!((l.present, l.version.as_deref(), l.target.as_deref(), l.source, l.current), (true, Some("0.0.5"), None, Source::Download, false));
    let m = prog(s, Os::Macos);
    assert_eq!((m.present, m.version.as_deref(), m.target.as_deref(), m.source, m.current), (false, None, None, Source::Download, false));
    assert!(stick::sticks(w.ctx.ops.as_ref(), &w.ctx.local, Some(&w.t.path().join("media/STICK/x.nvault"))).is_empty());
    assert!(stick::sticks(w.ctx.ops.as_ref(), &w.ctx.local, Some(Path::new("/home/u/x.nvault"))).is_empty());
    assert_eq!(snapshot(w.t.path()), before, "detection writes nothing");
}

#[test]
fn online_update_replaces_every_program_and_never_touches_the_data_partition() {
    let k = Key::new();
    let w = world(serve(&k, "0.2.0", &[]), k.pubkey_b64(), true, false);
    let data_before = snapshot(&w.data);
    let o = run(&w).unwrap();
    let by = |os: Os| o.results.iter().find(|r| r.os == os).unwrap().clone();
    assert_eq!((by(Os::Linux).result, by(Os::Linux).from.as_deref(), by(Os::Linux).to.as_deref()), (Change::Updated, Some("0.0.5"), Some("0.2.0")));
    assert_eq!((by(Os::Windows).result, by(Os::Windows).to.as_deref()), (Change::Updated, Some("0.2.0")));
    assert_eq!((by(Os::Macos).result, by(Os::Macos).from.as_deref(), by(Os::Macos).to.as_deref()), (Change::Updated, None, Some("0.2.0")));
    assert_eq!(std::fs::read(w.client.join("NiceVault-linux-x86_64")).unwrap(), b"channel linux");
    assert!(w.client.join("NiceVault-linux-x86_64.sig").exists());
    assert_eq!(std::fs::read(w.client.join("NiceVault-windows-x86_64.exe")).unwrap(), b"new windows exe");
    assert_ne!(std::fs::read(w.client.join("NiceVault-windows-x86_64.exe.sig")).unwrap(), b"old sig");
    assert_eq!(std::fs::read(w.client.join("NiceVault.app/Contents/MacOS/nicevault")).unwrap(), b"new mac binary");
    assert!(!w.client.join("NiceVault-macos-universal.zip").exists() && !w.client.join("NiceVault-macos-universal.zip.sig").exists(), "macOS stored once");
    assert!(!std::fs::read_to_string(w.client.join("README.txt")).unwrap().contains("missing"));
    assert!(leftovers(&w.client).is_empty(), "{:?}", leftovers(&w.client));
    let m = marker(&w);
    assert_eq!(m.clients.get("linux").cloned().flatten().as_deref(), Some("0.2.0"));
    assert_eq!(m.clients.get("macos").cloned().flatten().as_deref(), Some("0.2.0"));
    assert_eq!(m.mac_app.as_ref().map(|r| r.tree_sha256.clone()), nicevault_app::device::client::tree_digest(&w.client.join("NiceVault.app")));
    assert_eq!((m.device_id.as_str(), m.app_version.as_str()), ("ab".repeat(16).as_str(), "0.0.5"), "identity kept");
    assert!(m.updated.is_some());
    assert!(!o.stick.outdated && o.stick.programs.iter().all(|p| p.current));
    assert_eq!(snapshot(&w.data), data_before, "VaultData untouched");

    let mtime = std::fs::metadata(w.client.join("NiceVault-windows-x86_64.exe")).unwrap().modified().unwrap();
    let o = run(&w).unwrap();
    assert!(o.results.iter().all(|r| r.result == Change::Unchanged), "{:?}", o.results);
    assert_eq!(std::fs::metadata(w.client.join("NiceVault-windows-x86_64.exe")).unwrap().modified().unwrap(), mtime);
}

#[test]
fn offline_updates_only_this_os_and_leaves_the_others() {
    let w = world("http://127.0.0.1:9/latest.json".into(), Key::new().pubkey_b64(), true, false);
    let o = run(&w).unwrap();
    let by = |os: Os| o.results.iter().find(|r| r.os == os).unwrap().clone();
    assert_eq!(by(Os::Linux).result, Change::Updated);
    assert_eq!(by(Os::Windows).result, Change::Offline);
    assert_eq!(by(Os::Macos).result, Change::Offline);
    assert_eq!(std::fs::read(w.client.join("NiceVault-windows-x86_64.exe")).unwrap(), b"old windows");
    assert_eq!(std::fs::read(w.client.join("NiceVault-windows-x86_64.exe.sig")).unwrap(), b"old sig");
    let m = marker(&w);
    assert_eq!(m.clients.get("windows").cloned().flatten().as_deref(), Some("0.0.5"));
    assert!(std::fs::read_to_string(w.client.join("README.txt")).unwrap().contains("programs for macOS are missing"));
    assert!(o.stick.outdated, "Windows is still older, macOS still missing");
}

#[test]
fn a_bad_signature_keeps_the_old_program() {
    let k = Key::new();
    let w = world(serve(&k, "0.2.0", &["windows-x86_64"]), k.pubkey_b64(), true, false);
    let o = run(&w).unwrap();
    let win = o.results.iter().find(|r| r.os == Os::Windows).unwrap();
    assert_eq!(win.result, Change::Failed);
    assert_eq!(win.error.as_ref().unwrap().code, "device_download");
    assert!(win.error.as_ref().unwrap().message.contains("windows-x86_64"), "{:?}", win.error);
    assert_eq!(std::fs::read(w.client.join("NiceVault-windows-x86_64.exe")).unwrap(), b"old windows");
    assert_eq!(o.results.iter().find(|r| r.os == Os::Macos).unwrap().result, Change::Updated);
}

#[test]
fn not_mounted_is_said_then_mounted_without_privileges() {
    let k = Key::new();
    let w = world(serve(&k, "0.2.0", &[]), k.pubkey_b64(), false, false);
    let s = &stick::sticks(w.ctx.ops.as_ref(), &w.ctx.local, Some(&w.data.join("Dati.nvault")))[0];
    assert!(!s.mounted && !s.complete && !s.outdated && s.client_mount.is_none());
    assert_eq!(stick::start_update(&w.ctx, &w.st, "/dev/fake1").unwrap_err().code, "stick_not_mounted");
    let s = stick::mount(&w.ctx, "/dev/fake1").unwrap();
    assert!(s.mounted && s.complete && s.outdated);
    assert_eq!(PathBuf::from(s.client_mount.unwrap()), w.client);
    let job = stick::start_update(&w.ctx, &w.st, "/dev/fake1").unwrap();
    let t0 = Instant::now();
    let done = loop {
        if let Some((_, v)) = w.rec.0.lock().unwrap().iter().find(|(e, v)| e == "job-done" && v["job"] == job) {
            break v.clone();
        }
        assert!(t0.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(done["error"].is_null(), "{done}");
    assert_eq!(done["outcome"]["stick"]["outdated"], false);
    let steps: Vec<String> = w.rec.0.lock().unwrap().iter().filter(|(e, v)| e == "job-progress" && v["job"] == job).map(|(_, v)| v["step"].as_str().unwrap().to_owned()).collect();
    assert!(steps.first().map(String::as_str) == Some("check") && steps.last().map(String::as_str) == Some("marker"), "{steps:?}");
    assert!(w.rec.0.lock().unwrap().iter().any(|(e, v)| e == "job-progress" && v["step"] == "copy" && v["os"] == "windows"));
    assert!(std::fs::read_to_string(&w.file).unwrap().contains("NiceVault\""));
}

#[test]
fn running_from_the_stick_keeps_its_own_program() {
    let w = world("http://127.0.0.1:9/latest.json".into(), Key::new().pubkey_b64(), true, true);
    let s = &stick::sticks(w.ctx.ops.as_ref(), &w.ctx.local, None)[0];
    assert!(s.runs_from_stick && prog(s, Os::Linux).current);
    let o = run(&w).unwrap();
    assert_eq!(o.results.iter().find(|r| r.os == Os::Linux).unwrap().result, Change::InUse);
    assert_eq!(std::fs::read(w.client.join("NiceVault-linux-x86_64")).unwrap(), b"old linux");
}

#[test]
fn an_incomplete_device_is_refused() {
    let w = world("http://127.0.0.1:9/latest.json".into(), Key::new().pubkey_b64(), true, false);
    std::fs::remove_file(w.client.join(".nicevault-device.json")).unwrap();
    assert_eq!(run(&w).unwrap_err().code, "stick_not_device");
    assert_eq!(std::fs::read(w.client.join("NiceVault-linux-x86_64")).unwrap(), b"old linux");
}

const MB: usize = 1_000_000;

fn blob(n: usize, seed: u64) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            b"ABCDEFGHIJKLMNOP"[(x & 15) as usize]
        })
        .collect()
}

fn big_zip(n: usize, seed: u64) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default().unix_permissions(0o755).compression_level(Some(1));
    for (name, data) in [("NiceVault.app/Contents/Info.plist", b"<plist/>".to_vec()), ("NiceVault.app/Contents/MacOS/nicevault", blob(n, seed))] {
        w.start_file(name, o).unwrap();
        w.write_all(&data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn extract(zip_bytes: &[u8], dest: &Path) {
    nicevault_app::updater::extract_bundle(zip_bytes, dest).unwrap();
}

fn used(dir: &Path) -> u64 {
    walk(dir).iter().map(|p| std::fs::metadata(p).unwrap().len()).sum()
}

fn tight_world(k: &Key, bad: &[&str], exe_in_client: bool) -> World {
    let new_mac = big_zip(42 * MB, 7);
    let url = serve_with(k, "0.2.0", bad, [blob(28 * MB + 300_000, 3), blob(24 * MB + 500_000, 4), new_mac]);
    let w = world(url, k.pubkey_b64(), true, exe_in_client);
    std::fs::write(w.client.join("NiceVault-linux-x86_64"), blob(28 * MB, 1)).unwrap();
    std::fs::write(w.client.join("NiceVault-windows-x86_64.exe"), blob(24 * MB, 2)).unwrap();
    let old_mac = big_zip(42 * MB, 5);
    std::fs::write(w.client.join("NiceVault-macos-universal.zip"), &old_mac).unwrap();
    std::fs::write(w.client.join("NiceVault-macos-universal.zip.sig"), b"old mac sig").unwrap();
    extract(&old_mac, &w.client.join("NiceVault.app"));
    let mut m = marker(&w);
    m.clients.insert("macos".into(), Some("0.0.5".into()));
    std::fs::write(w.client.join(".nicevault-device.json"), serde_json::to_vec_pretty(&m).unwrap()).unwrap();
    if !exe_in_client {
        std::fs::write(&w.ctx.local.exe, blob(28 * MB + 200_000, 9)).unwrap();
    }
    let free = (128u64 << 20) - used(&w.client);
    assert!(free < 28 * MB as u64, "too full for two copies of any program: {free}");
    w
}

fn set_fault(w: &World, fail: Option<&str>, at: Option<&str>) {
    let mut f: FakeFile = serde_json::from_slice(&std::fs::read(&w.file).unwrap()).unwrap();
    f.fail = fail.map(str::to_owned);
    f.fail_at = at.map(str::to_owned);
    std::fs::write(&w.file, serde_json::to_vec(&f).unwrap()).unwrap();
}

fn by(o: &stick::UpdateOutcome, os: Os) -> stick::OsResult {
    o.results.iter().find(|r| r.os == os).unwrap().clone()
}

#[test]
fn a_128_mib_stick_is_updated_one_program_at_a_time() {
    let k = Key::new();
    let w = tight_world(&k, &[], false);
    let data_before = snapshot(&w.data);
    let s = &stick::sticks(w.ctx.ops.as_ref(), &w.ctx.local, None)[0];
    let sp = s.space.as_ref().expect("space");
    assert_eq!(sp.size, Some(128 << 20));
    assert!(sp.free.unwrap() < 28 * MB as u64 && sp.reclaim > 20 * MB as u64, "{sp:?}");
    assert!(sp.tight && sp.fits, "{sp:?}");

    let o = run(&w).unwrap();
    for os in Os::ALL {
        assert_eq!(by(&o, os).result, Change::Updated, "{:?}", o.results);
    }
    let sp = o.space.as_ref().unwrap();
    assert!(sp.tight, "{sp:?}");
    let order: Vec<Os> = sp.programs.iter().map(|p| p.os).collect();
    assert_eq!(order, [Os::Windows, Os::Linux, Os::Macos], "smallest first");
    assert_eq!(by(&o, Os::Macos).mode, Some(stick::Mode::Tight));
    assert_eq!(by(&o, Os::Windows).mode, Some(stick::Mode::Safe));
    assert_eq!(std::fs::read(w.client.join("NiceVault-linux-x86_64")).unwrap(), blob(28 * MB + 300_000, 3));
    assert_eq!(std::fs::read(w.client.join("NiceVault-windows-x86_64.exe")).unwrap(), blob(24 * MB + 500_000, 4));
    assert_eq!(std::fs::read(w.client.join("NiceVault.app/Contents/MacOS/nicevault")).unwrap(), blob(42 * MB, 7));
    assert!(!w.client.join("NiceVault-macos-universal.zip").exists() && !w.client.join("NiceVault-macos-universal.zip.sig").exists());
    assert!(leftovers(&w.client).is_empty(), "{:?}", leftovers(&w.client));
    assert!(used(&w.client) < 128 << 20);
    let m = marker(&w);
    assert_eq!(m.clients.get("macos").cloned().flatten().as_deref(), Some("0.2.0"));
    assert_eq!(m.clients.get("windows").cloned().flatten().as_deref(), Some("0.2.0"));
    assert_eq!(m.mac_app.unwrap().tree_sha256, nicevault_app::device::client::tree_digest(&w.client.join("NiceVault.app")).unwrap());
    assert!(!o.stick.outdated);
    assert_eq!(snapshot(&w.data), data_before, "VaultData untouched");
}

#[test]
fn a_failed_tight_write_is_rewritten_by_the_next_update() {
    let k = Key::new();
    let w = tight_world(&k, &[], false);
    let data_before = snapshot(&w.data);
    set_fault(&w, Some("copy_failed"), Some("write_macos"));
    let o = run(&w).unwrap();
    let mac = by(&o, Os::Macos);
    assert_eq!((mac.result, mac.mode, mac.to.as_deref()), (Change::Rewrite, Some(stick::Mode::Tight), None), "{mac:?}");
    assert_eq!(by(&o, Os::Windows).result, Change::Updated);
    assert_eq!(by(&o, Os::Linux).result, Change::Updated);
    assert!(!w.client.join("NiceVault.app").exists() && leftovers(&w.client).is_empty());
    assert_eq!(marker(&w).clients.get("macos").cloned().flatten(), None);
    assert!(std::fs::read_to_string(w.client.join("README.txt")).unwrap().contains("programs for macOS are missing"));
    assert!(o.stick.complete && o.stick.outdated && !prog(&o.stick, Os::Macos).current);
    assert_eq!(snapshot(&w.data), data_before);

    set_fault(&w, None, None);
    let o = run(&w).unwrap();
    assert_eq!(by(&o, Os::Macos).result, Change::Updated, "{:?}", o.results);
    assert_eq!(by(&o, Os::Linux).result, Change::Unchanged);
    assert_eq!(std::fs::read(w.client.join("NiceVault.app/Contents/MacOS/nicevault")).unwrap(), blob(42 * MB, 7));
    assert!(!o.stick.outdated);
    assert_eq!(snapshot(&w.data), data_before);
}

#[test]
fn running_from_a_tight_stick_never_removes_its_own_program() {
    let k = Key::new();
    let w = tight_world(&k, &[], true);
    let mine = std::fs::read(w.client.join("NiceVault-linux-x86_64")).unwrap();
    let o = run(&w).unwrap();
    assert_eq!(by(&o, Os::Linux).result, Change::InUse);
    assert_eq!(by(&o, Os::Windows).result, Change::Updated);
    assert_eq!(by(&o, Os::Macos).result, Change::Updated);
    assert_eq!(std::fs::read(w.client.join("NiceVault-linux-x86_64")).unwrap(), mine);
    assert!(leftovers(&w.client).is_empty());
}

#[test]
fn a_program_that_does_not_fit_even_in_place_is_left_as_it_was() {
    let k = Key::new();
    let w = tight_world(&k, &[], false);
    for f in ["NiceVault-macos-universal.zip", "NiceVault-macos-universal.zip.sig"] {
        std::fs::remove_file(w.client.join(f)).unwrap();
    }
    let mut f: FakeFile = serde_json::from_slice(&std::fs::read(&w.file).unwrap()).unwrap();
    f.disks[0].partitions[0].size = used(&w.client);
    std::fs::write(&w.file, serde_json::to_vec(&f).unwrap()).unwrap();
    let before = snapshot(&w.client).into_iter().filter(|e| !e.0.to_string_lossy().ends_with(".json")).map(|e| (e.0, e.1)).collect::<Vec<_>>();
    let o = run(&w).unwrap_or_else(|e| panic!("{e:?}"));
    for os in Os::ALL {
        let r = by(&o, os);
        assert_eq!((r.result, r.error.as_ref().map(|e| e.code.as_str())), (Change::Failed, Some("stick_full")), "{:?}", o.results);
    }
    assert!(!o.space.as_ref().unwrap().fits);
    for (p, bytes) in &before {
        if p.to_string_lossy().contains("NiceVault") && !p.to_string_lossy().ends_with(".txt") {
            assert_eq!(&std::fs::read(p).unwrap(), bytes, "{p:?} kept");
        }
    }
    assert_eq!(marker(&w).clients.get("macos").cloned().flatten().as_deref(), Some("0.0.5"));
}

#[test]
fn scarica_ora_on_a_128_mib_stick_clears_leftovers_first() {
    let k = Key::new();
    let w = tight_world(&k, &[], false);
    for f in ["NiceVault.app", "NiceVault-macos-universal.zip", "NiceVault-macos-universal.zip.sig"] {
        nicevault_app::device::client::remove_any(&w.client.join(f));
    }
    std::fs::write(w.client.join("NiceVault-linux-x86_64.part"), blob(28 * MB, 11)).unwrap();
    std::fs::write(w.client.join("NiceVault-windows-x86_64.exe.old"), blob(24 * MB, 12)).unwrap();
    let mut m = marker(&w);
    m.clients.insert("macos".into(), None);
    std::fs::write(w.client.join(".nicevault-device.json"), serde_json::to_vec_pretty(&m).unwrap()).unwrap();
    assert!((128u64 << 20) - used(&w.client) < 43 * MB as u64);
    let o = nicevault_app::device::job::Outcome {
        disk_id: "/dev/fake1".into(),
        client_mount: w.client.to_string_lossy().into_owned(),
        data_mount: w.data.to_string_lossy().into_owned(),
        data_fs: None,
        data_free: 0,
        whole_volume_size: 0,
        missing: vec![Os::Macos],
        first_volume: None,
        log: String::new(),
    };
    w.ctx.outcomes.lock().unwrap().insert("j".into(), o);
    let data_before = snapshot(&w.data);
    let o = nicevault_app::device::cmd::fetch_missing(&w.ctx, "j").unwrap();
    assert!(o.missing.is_empty());
    assert_eq!(std::fs::read(w.client.join("NiceVault.app/Contents/MacOS/nicevault")).unwrap(), blob(42 * MB, 7));
    assert!(leftovers(&w.client).is_empty(), "{:?}", leftovers(&w.client));
    assert!(marker(&w).mac_app.is_some());
    assert_eq!(snapshot(&w.data), data_before);
}
