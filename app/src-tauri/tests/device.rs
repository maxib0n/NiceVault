//! "Crea dispositivo" end to end with the fake disk backend (partitions are folders) and a
//! local update server with signed builds: download + verify, copy + verify, marker last,
//! refusals, confirmation, an unplug midway and "Completa", a fixed-size first volume.
//! Nothing here touches a real disk or asks for any privilege.

use base64::Engine;
use nicevault_app::device::backend::{Fake, FakeFile};
use nicevault_app::device::client::Local;
use nicevault_app::device::job::DeviceCtx;
use nicevault_app::device::{self, DataFs, Disk, NvState, Os, Partition, PlanOpts, Table, cmd};
use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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
impl Rec {
    fn done(&self, job: &str) -> Value {
        let t0 = Instant::now();
        loop {
            if let Some((_, v)) = self.0.lock().unwrap().iter().find(|(e, v)| e == "job-done" && v["job"] == job) {
                return v.clone();
            }
            assert!(t0.elapsed() < Duration::from_secs(30), "no job-done for {job}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn steps(&self, job: &str) -> Vec<String> {
        let mut v: Vec<String> = vec![];
        for (e, p) in self.0.lock().unwrap().iter() {
            if e == "job-progress" && p["job"] == job {
                let s = p["step"].as_str().unwrap().to_owned();
                if v.last() != Some(&s) {
                    v.push(s);
                }
            }
        }
        v
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
    for (name, data) in [("NiceVault.app/Contents/Info.plist", &b"<plist/>"[..]), ("NiceVault.app/Contents/MacOS/nicevault", b"mac binary")] {
        w.start_file(name, zip::write::SimpleFileOptions::default().unix_permissions(0o755)).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// A local update channel at `version`, published like the real one (plain files:
/// latest.json, signatures over the plaintext); `bad` platforms get a signature of other bytes.
fn serve(k: &Key, version: &str, bad: &[&str]) -> String {
    serve_switch(k, version, bad, Arc::new(AtomicBool::new(true)))
}

/// The same; while `online` is false every request gets a 503 (offline).
fn serve_switch(k: &Key, version: &str, bad: &[&str], online: Arc<AtomicBool>) -> String {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}", server.server_addr().to_ip().unwrap());
    let builds = [("linux-x86_64", "NiceVault-linux-x86_64.AppImage", b"linux binary".to_vec()), ("windows-x86_64", "NiceVault-windows-x86_64.exe", b"windows exe".to_vec()), ("darwin-aarch64", "NiceVault-macos-universal.zip", app_zip())];
    let mut files: Vec<(String, Vec<u8>)> = vec![];
    let mut platforms = serde_json::Map::new();
    for (key, name, bytes) in builds {
        let signed: &[u8] = if bad.contains(&key) { b"something else" } else { &bytes };
        // Linux is published as the AppImage under Tauri's `-appimage` key
        let mkey = if key == "linux-x86_64" { "linux-x86_64-appimage" } else { key };
        let plat = if key == "darwin-aarch64" { "darwin-aarch64,darwin-x86_64" } else { mkey };
        platforms.insert(mkey.into(), serde_json::json!({ "signature": k.sign(signed, version, plat), "url": format!("{url}/v/{name}") }));
        files.push((format!("/v/{name}"), bytes));
    }
    files.push(("/latest.json".into(), serde_json::to_vec(&serde_json::json!({ "version": version, "platforms": platforms })).unwrap()));
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let hit = files.iter().find(|(p, _)| p == req.url()).map(|(_, b)| b.clone());
            let _ = match hit {
                _ if !online.load(Ordering::SeqCst) => req.respond(tiny_http::Response::empty(503)),
                Some(b) => req.respond(tiny_http::Response::from_data(b)),
                None => req.respond(tiny_http::Response::empty(404)),
            };
        }
    });
    format!("{url}/latest.json")
}

fn stick(mount: &Path) -> Disk {
    Disk {
        id: "/dev/fake1".into(),
        os_path: "/dev/fake1".into(),
        vendor: "SanDisk".into(),
        model: "Ultra".into(),
        serial: "4C530001071103235".into(),
        bus: "usb".into(),
        size: 32 << 30,
        removable: true,
        partitions: vec![Partition { path: "/dev/fake1p1".into(), label: "STICK".into(), fs: "vfat".into(), size: 32 << 30, mountpoints: vec![mount.to_string_lossy().into_owned()] }],
        ..Default::default()
    }
}

struct World {
    _t: tempfile::TempDir,
    file: PathBuf,
    rec: Rec,
    st: Arc<AppState>,
    ctx: Arc<DeviceCtx>,
}

fn world(manifest: String, pubkey: String, fake: impl FnOnce(&mut FakeFile)) -> World {
    let t = tempfile::tempdir().unwrap();
    let file = t.path().join("disks.json");
    let mount = t.path().join("media/STICK");
    std::fs::create_dir_all(&mount).unwrap();
    std::fs::write(mount.join("old-file.txt"), b"will be erased").unwrap();
    let mut f = FakeFile { disks: vec![stick(&mount)], root: t.path().join("parts").to_string_lossy().into_owned(), step_ms: Some(10), ..Default::default() };
    fake(&mut f);
    std::fs::write(&file, serde_json::to_vec(&f).unwrap()).unwrap();
    // the running "app": a Linux binary without a signature next to it
    let exe = t.path().join("bin/nicevault");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"local linux build").unwrap();
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
    World { _t: t, file, rec, st, ctx }
}

fn opts() -> PlanOpts {
    PlanOpts { id: "/dev/fake1".into(), table: Table::Mbr, data_fs: DataFs::Exfat, first_volume: None, download: true }
}

fn disk_now(w: &World) -> Disk {
    let f: FakeFile = serde_json::from_slice(&std::fs::read(&w.file).unwrap()).unwrap();
    let mut d = f.disks.into_iter().next().unwrap();
    d.nicevault = device::state_of(&d);
    d
}

#[test]
fn creates_a_device_with_the_latest_signed_builds() {
    let k = Key::new();
    let w = world(serve(&k, "0.2.0", &[]), k.pubkey_b64(), |_| {});
    let c = cmd::candidates(&w.ctx, &w.st);
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].name.as_str(), c[0].serial_tail.as_str()), ("SanDisk Ultra", "1103235"));
    let o = cmd::options(&w.ctx, &w.st, "/dev/fake1").unwrap();
    assert!(o.fs.iter().any(|f| f.id == DataFs::Exfat && f.creatable));
    assert_eq!(o.update_host, w.ctx.manifest_url.split('/').nth(2).unwrap());

    let p = cmd::make_plan(&w.ctx, &w.st, &opts()).unwrap();
    assert_eq!(cmd::create(&w.ctx, &w.st, &p.id, "SanDisk").unwrap_err().code, "confirmation_mismatch");
    let job = cmd::create(&w.ctx, &w.st, &p.id, " sandisk ultra ").unwrap();
    let done = w.rec.done(&job);
    assert!(done["error"].is_null(), "{done}");
    assert_eq!(w.rec.steps(&job), ["check", "download", "unmount", "partition", "format_client", "format_data", "mount", "copy", "verify", "marker"]);
    let out = &done["outcome"];
    let client = PathBuf::from(out["client_mount"].as_str().unwrap());
    assert_eq!(std::fs::read(client.join("NiceVault-linux-x86_64")).unwrap(), b"linux binary", "the download wins over the local copy");
    assert_eq!(std::fs::read(client.join("NiceVault-windows-x86_64.exe")).unwrap(), b"windows exe");
    assert!(client.join("NiceVault-windows-x86_64.exe.sig").is_file());
    assert_eq!(std::fs::read(client.join("NiceVault.app/Contents/MacOS/nicevault")).unwrap(), b"mac binary");
    // macOS once: the app, checked against the signed zip (not kept) and recorded in the marker
    assert!(!client.join("NiceVault-macos-universal.zip").exists() && !client.join("NiceVault-macos-universal.zip.sig").exists());
    let rec = serde_json::from_slice::<device::Marker>(&std::fs::read(client.join(device::MARKER)).unwrap()).unwrap().mac_app.expect("mac record");
    assert_eq!(Some(rec.tree_sha256), device::client::tree_digest(&client.join("NiceVault.app")));
    assert!(rec.zip_sha256.is_some());
    assert!(client.join("LEGGIMI.txt").is_file() && client.join("README.txt").is_file());
    assert!(!client.join("autorun.inf").exists(), "no autorun.inf on the client partition");
    let m: device::Marker = serde_json::from_slice(&std::fs::read(client.join(device::MARKER)).unwrap()).unwrap();
    assert_eq!((m.format, m.data_fs.as_str(), m.device_id.len()), (1, "exfat", 32));
    assert_eq!(m.clients.get("windows").cloned().flatten().as_deref(), Some("0.2.0"));
    assert_eq!(out["missing"], serde_json::json!([]));
    assert_eq!(disk_now(&w).nicevault, NvState::Complete);
    assert!(!PathBuf::from(out["data_mount"].as_str().unwrap()).join("old-file.txt").exists());
    // the log names every step, never a password
    let log = cmd::log_text(&w.ctx, &job).unwrap();
    assert!(log.contains("\"kind\":\"plan\"") && log.contains("fake_command") && log.contains("\"kind\":\"done\""));

    // a fixed-size first volume in VaultData ("Un pezzo", preallocated)
    let data = PathBuf::from(out["data_mount"].as_str().unwrap());
    let vol = data.join("Dati.nvault");
    let info = w.st.vault_create_sized(vol.to_str().unwrap(), "pw".to_owned().into(), Some("fast"), Some(32 << 20)).unwrap();
    assert_eq!(std::fs::metadata(&vol).unwrap().len(), 32 << 20);
    assert_eq!(w.st.vault_stats(&info.id).unwrap().used_bytes, 0);
    w.st.vault_lock(&info.id, "user").unwrap();
    // "Completa" on a complete device: nothing rewritten, the outcome again (offer the volume)
    let j2 = cmd::resume(&w.ctx, &w.st, "/dev/fake1").unwrap();
    assert!(w.rec.done(&j2)["error"].is_null());
    // eject
    cmd::eject(&w.ctx, "/dev/fake1").unwrap();
    assert!(cmd::candidates(&w.ctx, &w.st).is_empty());
}

#[test]
fn bad_signature_and_old_channel_fall_back_to_local_copies() {
    let k = Key::new();
    // Windows' signature is for other bytes: not copied; Linux comes from the channel
    let w = world(serve(&k, "0.2.0", &["windows-x86_64"]), k.pubkey_b64(), |_| {});
    let p = cmd::make_plan(&w.ctx, &w.st, &opts()).unwrap();
    let job = cmd::create(&w.ctx, &w.st, &p.id, "SanDisk Ultra").unwrap();
    let done = w.rec.done(&job);
    assert!(done["error"].is_null(), "{done}");
    let client = PathBuf::from(done["outcome"]["client_mount"].as_str().unwrap());
    assert!(!client.join("NiceVault-windows-x86_64.exe").exists());
    assert_eq!(done["outcome"]["missing"], serde_json::json!(["windows"]));
    assert!(std::fs::read_to_string(client.join("README.txt")).unwrap().contains("programs for Windows are missing"));

    // a channel older than this app (0.0.9 < 0.1.0) is never used: local copies only
    let w = world(serve(&k, "0.0.9", &[]), k.pubkey_b64(), |_| {});
    let p = cmd::make_plan(&w.ctx, &w.st, &opts()).unwrap();
    let job = cmd::create(&w.ctx, &w.st, &p.id, "SanDisk Ultra").unwrap();
    let done = w.rec.done(&job);
    let client = PathBuf::from(done["outcome"]["client_mount"].as_str().unwrap());
    assert_eq!(std::fs::read(client.join("NiceVault-linux-x86_64")).unwrap(), b"local linux build");
    assert!(!client.join("NiceVault-linux-x86_64.sig").exists(), "the unsigned running build gets no .sig");
    assert_eq!(done["outcome"]["missing"], serde_json::json!(["windows", "macos"]));

    // offline (nothing listens): same
    let w = world("http://127.0.0.1:9/latest.json".into(), k.pubkey_b64(), |_| {});
    let p = cmd::make_plan(&w.ctx, &w.st, &opts()).unwrap();
    let done = w.rec.done(&cmd::create(&w.ctx, &w.st, &p.id, "SanDisk Ultra").unwrap());
    assert!(done["error"].is_null());
    assert_eq!(done["outcome"]["missing"], serde_json::json!(["windows", "macos"]));
}

/// The offline case: made offline, so Windows and macOS are missing; "Scarica ora" once online
/// adds just them (opened from the release packs, verified), then the READMEs and the marker.
#[test]
fn download_now_adds_the_missing_programs() {
    let k = Key::new();
    let online = Arc::new(AtomicBool::new(false));
    let w = world(serve_switch(&k, "0.2.0", &[], online.clone()), k.pubkey_b64(), |_| {});
    let p = cmd::make_plan(&w.ctx, &w.st, &opts()).unwrap();
    let job = cmd::create(&w.ctx, &w.st, &p.id, "SanDisk Ultra").unwrap();
    let done = w.rec.done(&job);
    assert!(done["error"].is_null(), "{done}");
    assert_eq!(done["outcome"]["missing"], serde_json::json!(["windows", "macos"]));
    let client = PathBuf::from(done["outcome"]["client_mount"].as_str().unwrap());
    assert!(std::fs::read_to_string(client.join("LEGGIMI.txt")).unwrap().contains("Scarica ora"));

    // still offline: an error, nothing changed
    let e = cmd::fetch_missing(&w.ctx, &job).unwrap_err();
    assert_eq!(e.code, "device_download");
    assert!(!client.join("NiceVault-windows-x86_64.exe").exists());

    online.store(true, Ordering::SeqCst);
    let o = cmd::fetch_missing(&w.ctx, &job).unwrap();
    assert!(o.missing.is_empty(), "{:?}", o.missing);
    assert_eq!(std::fs::read(client.join("NiceVault-windows-x86_64.exe")).unwrap(), b"windows exe", "plain on the stick");
    assert!(client.join("NiceVault-windows-x86_64.exe.sig").is_file());
    assert_eq!(std::fs::read(client.join("NiceVault.app/Contents/MacOS/nicevault")).unwrap(), b"mac binary");
    assert_eq!(std::fs::read(client.join("NiceVault-linux-x86_64")).unwrap(), b"local linux build", "what was there stays");
    assert!(!std::fs::read_to_string(client.join("README.txt")).unwrap().contains("missing"));
    let m: device::Marker = serde_json::from_slice(&std::fs::read(client.join(device::MARKER)).unwrap()).unwrap();
    assert_eq!(m.clients.get("windows").cloned().flatten().as_deref(), Some("0.2.0"));
    assert_eq!(m.clients.get("macos").cloned().flatten().as_deref(), Some("0.2.0"));
    assert_eq!(cmd::outcome(&w.ctx, &job).unwrap().missing, vec![]);
    // nothing missing any more: a no-op
    assert!(cmd::fetch_missing(&w.ctx, &job).unwrap().missing.is_empty());
    assert_eq!(cmd::fetch_missing(&w.ctx, "nope").unwrap_err().code, "not_found");
}

#[test]
fn refused_elevation_changes_nothing() {
    let k = Key::new();
    let w = world(serve(&k, "0.2.0", &[]), k.pubkey_b64(), |f| f.fail = Some("elevation_denied".into()));
    let p = cmd::make_plan(&w.ctx, &w.st, &opts()).unwrap();
    let done = w.rec.done(&cmd::create(&w.ctx, &w.st, &p.id, "SanDisk Ultra").unwrap());
    assert_eq!(done["error"]["code"], "elevation_denied");
    assert_eq!(disk_now(&w).partitions[0].label, "STICK", "not formatted");
    // the plan is used up; a new one is needed
    assert_eq!(cmd::create(&w.ctx, &w.st, &p.id, "SanDisk Ultra").unwrap_err().code, "not_found");
}

#[test]
fn unplugged_midway_then_completa() {
    let k = Key::new();
    let w = world(serve(&k, "0.2.0", &[]), k.pubkey_b64(), |f| {
        f.fail = Some("device_gone".into());
        f.fail_at = Some("copy".into());
    });
    let p = cmd::make_plan(&w.ctx, &w.st, &opts()).unwrap();
    let job = cmd::create(&w.ctx, &w.st, &p.id, "SanDisk Ultra").unwrap();
    assert_eq!(w.rec.done(&job)["error"]["code"], "device_gone");
    let d = disk_now(&w);
    assert_eq!(device::resume_action(&d, Some(false)), device::Resume::Complete);
    assert_eq!(cmd::candidates(&w.ctx, &w.st)[0].disk.nicevault, NvState::Incomplete);
    // replugged: the fault is gone
    let mut f: FakeFile = serde_json::from_slice(&std::fs::read(&w.file).unwrap()).unwrap();
    f.fail = None;
    std::fs::write(&w.file, serde_json::to_vec(&f).unwrap()).unwrap();
    let j2 = cmd::resume(&w.ctx, &w.st, "/dev/fake1").unwrap();
    let done = w.rec.done(&j2);
    assert!(done["error"].is_null(), "{done}");
    assert_eq!(w.rec.steps(&j2), ["check", "download", "mount", "copy", "verify", "marker"], "no reformat");
    assert_eq!(disk_now(&w).nicevault, NvState::Complete);
}

#[test]
fn system_and_internal_disks_are_never_offered() {
    let k = Key::new();
    let w = world(serve(&k, "0.2.0", &[]), k.pubkey_b64(), |f| {
        let mut sys = f.disks[0].clone();
        (sys.id, sys.is_system) = ("/dev/fake0".into(), true);
        let mut internal = f.disks[0].clone();
        (internal.id, internal.removable, internal.bus) = ("/dev/fake2".into(), false, "sata".into());
        let mut small = f.disks[0].clone();
        (small.id, small.size) = ("/dev/fake3".into(), 512 << 20);
        f.disks.extend([sys, internal, small]);
    });
    let ids: Vec<String> = cmd::candidates(&w.ctx, &w.st).into_iter().map(|c| c.disk.id).collect();
    assert_eq!(ids, ["/dev/fake1"]);
    for (id, why) in [("/dev/fake0", "system"), ("/dev/fake2", "internal"), ("/dev/fake3", "too_small")] {
        let e = cmd::make_plan(&w.ctx, &w.st, &PlanOpts { id: id.into(), ..opts() }).unwrap_err();
        assert_eq!((e.code.as_str(), e.message.as_str()), ("device_refused", why));
    }
}

#[test]
fn the_plan_lists_the_nicevault_volumes_the_erase_destroys() {
    let k = Key::new();
    let w = world(serve(&k, "0.2.0", &[]), k.pubkey_b64(), |_| {});
    let mount = PathBuf::from(&disk_now(&w).partitions[0].mountpoints[0]);
    std::fs::write(mount.join("SH2RE.nvault"), b"v").unwrap();
    std::fs::write(mount.join("SH2RE.nvault-journal"), b"j").unwrap();
    std::fs::create_dir_all(mount.join("Lavoro/deeper")).unwrap();
    std::fs::write(mount.join("Lavoro/Archivio.NVAULT"), b"v").unwrap();
    std::fs::write(mount.join("Lavoro/deeper/Too-deep.nvault"), b"v").unwrap();
    std::fs::create_dir_all(mount.join("not-a-volume.nvault")).unwrap();
    let p = cmd::make_plan(&w.ctx, &w.st, &opts()).unwrap();
    let names: Vec<&str> = p.volumes.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["Archivio.NVAULT", "SH2RE.nvault"], "top level and one folder down, files only");
    assert!(p.volumes.iter().all(|v| !v.open));
    assert_eq!(serde_json::to_value(&p).unwrap()["volumes"][1]["path"], mount.join("SH2RE.nvault").to_string_lossy().as_ref());
    // nothing was touched by the scan
    assert_eq!(std::fs::read(mount.join("SH2RE.nvault")).unwrap(), b"v");
    // an empty stick: no warning
    for f in ["SH2RE.nvault", "Lavoro/Archivio.NVAULT"] {
        std::fs::remove_file(mount.join(f)).unwrap();
    }
    assert!(cmd::make_plan(&w.ctx, &w.st, &opts()).unwrap().volumes.is_empty());
}

#[test]
fn volume_scan_marks_open_volumes_and_is_capped() {
    let t = tempfile::tempdir().unwrap();
    let mut d = stick(t.path());
    let deep = t.path().join("a/b/c/Aperto.nvault");
    let open = vec![deep.clone()];
    let v = device::volumes_on(&d, &open);
    assert_eq!(v.len(), 1, "a volume open in the app is listed wherever it is on the drive");
    assert!(v[0].open && v[0].name == "Aperto.nvault");
    // many entries: the walk stops at the cap (no stall), whatever is found before it is kept
    for i in 0..(device::SCAN_CAP + 50) {
        std::fs::write(t.path().join(format!("f{i:05}.txt")), b"").unwrap();
    }
    let t0 = Instant::now();
    let _ = device::volumes_on(&d, &[]);
    assert!(t0.elapsed() < Duration::from_secs(5));
    d.partitions[0].mountpoints.clear();
    assert!(device::volumes_on(&d, &[]).is_empty(), "unmounted partitions are not read");
}

/// The DryRun wrapper (`NICEVAULT_DEVICE_DRY_RUN=1`) over a backend that records every call:
/// a whole create, "Completa" and eject reach the inner backend only for read-only
/// enumeration; copy, verify and the marker land in the dry run's temp folders.
#[test]
fn dry_run_never_reaches_the_real_backend() {
    use nicevault_app::device::backend::{DiskOps, DryRun};
    use nicevault_app::device::job::Log;
    use nicevault_app::error::{ApiError, ApiResult};
    #[derive(Clone, Default)]
    struct Spy {
        calls: Arc<Mutex<Vec<String>>>,
        disk: Arc<Mutex<Option<Disk>>>,
    }
    impl DiskOps for Spy {
        fn name(&self) -> &'static str {
            "spy"
        }
        fn disks(&self) -> Vec<Disk> {
            self.disk.lock().unwrap().clone().into_iter().collect()
        }
        fn missing_tool(&self, _fs: DataFs) -> Option<String> {
            None
        }
        fn partition(&self, _: &device::Plan, _: &Log, _: &mut dyn FnMut(&str)) -> ApiResult<()> {
            self.calls.lock().unwrap().push("partition".into());
            Err(ApiError::new("leak", "partition reached the real backend"))
        }
        fn mount(&self, _: &device::Plan, _: &Log) -> ApiResult<(PathBuf, PathBuf)> {
            self.calls.lock().unwrap().push("mount".into());
            Err(ApiError::new("leak", "mount reached the real backend"))
        }
        fn eject(&self, _: &Disk, _: &Log) -> ApiResult<()> {
            self.calls.lock().unwrap().push("eject".into());
            Err(ApiError::new("leak", "eject reached the real backend"))
        }
    }
    let k = Key::new();
    let base = world(serve(&k, "0.2.0", &[]), k.pubkey_b64(), |_| {});
    let mount = PathBuf::from(&disk_now(&base).partitions[0].mountpoints[0]);
    std::fs::write(mount.join("SH2RE.nvault"), b"a user volume").unwrap();
    let before: Vec<_> = std::fs::read_dir(&mount).unwrap().flatten().map(|e| e.file_name()).collect();
    let spy = Spy::default();
    *spy.disk.lock().unwrap() = Some(disk_now(&base));
    let ctx = Arc::new(DeviceCtx {
        ops: Box::new(DryRun::new(Box::new(spy.clone()))),
        plans: Default::default(),
        outcomes: Default::default(),
        logs: Default::default(),
        log_dir: base._t.path().join("logs"),
        manifest_url: base.ctx.manifest_url.clone(),
        local: Local { exe: base._t.path().join("bin/nicevault"), os: Os::Linux, version: "0.1.0".into(), pubkey: k.pubkey_b64() },
    });
    let p = cmd::make_plan(&ctx, &base.st, &opts()).unwrap();
    assert_eq!(p.volumes.len(), 1);
    let job = cmd::create(&ctx, &base.st, &p.id, "SanDisk Ultra").unwrap();
    let done = base.rec.done(&job);
    assert!(done["error"].is_null(), "{done}");
    let client = PathBuf::from(done["outcome"]["client_mount"].as_str().unwrap());
    let data = PathBuf::from(done["outcome"]["data_mount"].as_str().unwrap());
    let tmp = std::env::temp_dir().join("nicevault-dry-run");
    assert!(client.starts_with(&tmp) && data.starts_with(&tmp), "{client:?} {data:?}");
    assert!(client.join(device::MARKER).is_file() && client.join("NiceVault-linux-x86_64").is_file());
    // the drive: same files, same bytes, nothing new
    let after: Vec<_> = std::fs::read_dir(&mount).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(before, after);
    assert_eq!(std::fs::read(mount.join("SH2RE.nvault")).unwrap(), b"a user volume");
    cmd::eject(&ctx, "/dev/fake1").unwrap();
    assert!(spy.calls.lock().unwrap().is_empty(), "mutating calls leaked: {:?}", spy.calls.lock().unwrap());
    let log = cmd::log_text(&ctx, &job).unwrap();
    assert!(log.contains("would_run") && log.contains("mount_instead"));
}
