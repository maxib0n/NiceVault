//! The device job: one worker thread through the plan's steps, `job-progress {job, step,
//! done_bytes, total_bytes}` and one `job-done {job, error, outcome}`. Resumable: a run that
//! stopped after partitioning ("Completa") mounts, copies and writes the marker again.
//!
//! The log (`device-<ts>.log`, JSON lines, in the OS cache dir; portable: the temp dir, never
//! the stick) records the plan, every command with its arguments, exit code, output and time.
//! Never a password: none passes through here.

use super::backend::DiskOps;
use super::client::{self, Builds, Local};
use super::{DataFs, Disk, HostCtx, Os, Plan, VolumeOpts};
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, Job, lock_or_recover, random_id};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Log {
    path: PathBuf,
    file: Mutex<Option<std::fs::File>>,
}

impl Log {
    pub fn open(dir: &Path) -> Log {
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join(format!("device-{}.log", crate::updater::now_ms()));
        let file = std::fs::File::create(&path).ok();
        Log { path, file: Mutex::new(file) }
    }
    /// Discards everything (tests, probes).
    pub fn null() -> Log {
        Log { path: PathBuf::new(), file: Mutex::new(None) }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn event(&self, kind: &str, data: Value) {
        if let Some(f) = lock_or_recover(&self.file).as_mut() {
            let line = json!({ "t": crate::updater::now_ms(), "kind": kind, "data": data });
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }
    pub fn command(&self, prog: &str, args: &[String], code: Option<i32>, out: &str, err: &str, took: Duration) {
        // an -EncodedCommand argument is the script itself: logged decoded for auditing
        self.event("command", json!({ "prog": prog, "args": args, "exit": code, "stdout": out, "stderr": err, "ms": took.as_millis() as u64 }));
    }
}

/// What a finished (or already complete) device gives the UI.
#[derive(Clone, Debug, Serialize, Default)]
pub struct Outcome {
    pub disk_id: String,
    pub client_mount: String,
    pub data_mount: String,
    pub data_fs: Option<DataFs>,
    pub data_free: u64,
    /// Size of a "Tutta la partizione" volume.
    pub whole_volume_size: u64,
    pub missing: Vec<Os>,
    pub first_volume: Option<VolumeOpts>,
    pub log: String,
}

/// Tauri-managed state of the feature.
pub struct DeviceCtx {
    pub ops: Box<dyn DiskOps>,
    pub plans: Mutex<HashMap<String, Plan>>,
    pub outcomes: Mutex<HashMap<String, Outcome>>,
    pub logs: Mutex<HashMap<String, PathBuf>>,
    pub log_dir: PathBuf,
    pub manifest_url: String,
    pub local: Local,
}

impl DeviceCtx {
    pub fn new(ops: Box<dyn DiskOps>, log_dir: PathBuf) -> Arc<DeviceCtx> {
        if std::env::var("NICEVAULT_DEVICE_TRACE").is_ok_and(|v| v == "1") {
            eprintln!("[device] backend: {}", ops.name());
        }
        Arc::new(DeviceCtx {
            ops,
            plans: Mutex::default(),
            outcomes: Mutex::default(),
            logs: Mutex::default(),
            log_dir,
            manifest_url: crate::updater::manifest_url(),
            local: Local::this_app(),
        })
    }

    pub fn host_ctx(&self, st: &AppState) -> HostCtx {
        HostCtx::detect(st.vault_list_open().into_iter().map(|v| PathBuf::from(v.path)).collect())
    }

    pub fn find(&self, id: &str) -> ApiResult<Disk> {
        self.ops.disks().into_iter().find(|d| d.id == id).ok_or_else(|| ApiError::new("device_not_found", "The drive is no longer connected."))
    }
}

pub(crate) fn free_space(p: &Path) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else { return 0 };
        // SAFETY: statvfs fills a zeroed struct from a NUL-terminated path.
        unsafe {
            let mut s: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c.as_ptr(), &mut s) != 0 {
                return 0;
            }
            s.f_bavail as u64 * s.f_frsize as u64
        }
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetDiskFreeSpaceExW(root: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
        }
        let w: Vec<u16> = p.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
        let (mut a, mut t, mut f) = (0u64, 0u64, 0u64);
        // SAFETY: NUL-terminated path, out-pointers to locals.
        unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut a, &mut t, &mut f) };
        a
    }
}

pub fn outcome(plan: &Plan, client: &Path, data: &Path, missing: Vec<Os>, log: &Path) -> Outcome {
    let free = free_space(data);
    Outcome {
        disk_id: plan.disk.id.clone(),
        client_mount: client.to_string_lossy().into_owned(),
        data_mount: data.to_string_lossy().into_owned(),
        data_fs: Some(plan.data_fs),
        data_free: free,
        whole_volume_size: super::whole_volume_size(free),
        missing,
        first_volume: plan.first_volume.clone(),
        log: log.to_string_lossy().into_owned(),
    }
}

struct Run<'a> {
    st: &'a AppState,
    job: &'a str,
    cancel: &'a AtomicBool,
    last: Instant,
}

impl Run<'_> {
    fn step(&mut self, step: &str, done: u64, total: u64, force: bool) {
        if force || self.last.elapsed() >= Duration::from_millis(100) {
            self.last = Instant::now();
            self.st.emit("job-progress", json!({ "job": self.job, "step": step, "done_bytes": done, "total_bytes": total, "current": step }));
        }
    }
    fn cancelled(&self) -> ApiResult<()> {
        if self.cancel.load(Ordering::Relaxed) { Err(ApiError::cancelled()) } else { Ok(()) }
    }
}

/// The steps themselves (also run synchronously by the tests).
pub fn execute(ctx: &DeviceCtx, st: &AppState, job: &str, cancel: &AtomicBool, plan: &Plan, resume: bool, log: &Log) -> ApiResult<Outcome> {
    let mut r = Run { st, job, cancel, last: Instant::now() };
    let ops = ctx.ops.as_ref();
    let fault = |s: &str| ops.fault(s).map_or(Ok(()), Err);
    log.event("plan", serde_json::to_value(plan).unwrap_or_default());
    log.event("backend", json!({ "name": ops.name(), "resume": resume }));

    // check: the same disk, still allowed
    r.step("check", 0, 0, true);
    fault("check")?;
    let now = ops.disks().into_iter().find(|d| d.id == plan.disk.id).ok_or_else(|| ApiError::new("device_gone", "The drive was disconnected."))?;
    if !now.same_as(&plan.disk) {
        return Err(ApiError::new("device_changed", "The drive changed since it was chosen."));
    }
    if let Some(why) = super::refusal(&now, &ctx.host_ctx(st)) {
        return Err(why.error());
    }
    r.cancelled()?;

    // download (before anything is erased: a failure here changes nothing)
    let builds: Builds = if plan.download {
        r.step("download", 0, 0, true);
        fault("download")?;
        let b = client::gather(&ctx.local, &ctx.manifest_url, true, &mut |d, t| r.step("download", d, t, false));
        log.event("builds", json!({ "notes": b.notes, "missing": b.missing(), "files": b.files.iter().map(|f| json!({"name": f.name, "origin": f.origin, "version": f.version, "bytes": f.bytes.len(), "signed": f.sig.is_some()})).collect::<Vec<_>>() }));
        b
    } else {
        client::gather(&ctx.local, &ctx.manifest_url, false, &mut |_, _| {})
    };
    r.cancelled()?;

    let gone_or = |e: ApiError| -> ApiError {
        if e.code != "device_gone" && !ops.disks().iter().any(|d| d.same_as(&plan.disk)) { ApiError::new("device_gone", "The drive was disconnected.") } else { e }
    };

    if !resume {
        let mut cur = String::new();
        ops.partition(plan, log, &mut |s| {
            cur = s.to_owned();
            r.step(s, 0, 0, true);
        })?;
    }

    r.step("mount", 0, 0, true);
    fault("mount")?;
    let (client_dir, data_dir) = ops.mount(plan, log).map_err(gone_or)?;
    if plan.data_fs.unix_perms() {
        // any user of any Linux machine can write the data folder
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o777));
        }
    }

    r.step("copy", 0, builds.total_bytes(), true);
    fault("copy").map_err(gone_or)?;
    let free = || ops.client_free(&plan.disk, &client_dir);
    let room = client::Room { free: &free, running: &ctx.local.exe };
    let mac_app = client::write_client(&client_dir, &builds, plan.data_fs, &room, &mut |d, t| r.step("copy", d, t, false)).map_err(gone_or)?;

    r.step("verify", 0, 0, true);
    fault("verify").map_err(gone_or)?;
    // (macOS' bundle was checked against its zip as it was written; the zip is not kept)
    for f in builds.files.iter().filter(|f| f.os != super::Os::Macos) {
        let back = std::fs::read(client_dir.join(&f.name)).map_err(|e| gone_or(ApiError::new("copy_failed", e.to_string())))?;
        if back != f.bytes {
            return Err(ApiError::new("copy_failed", format!("{}: the copy differs", f.name)));
        }
    }

    r.step("marker", 0, 0, true);
    fault("marker").map_err(gone_or)?;
    let mut m = client::marker(plan, &builds);
    m.mac_app = mac_app;
    client::write_marker(&client_dir, &m).map_err(gone_or)?;
    log.event("done", json!({ "marker": m }));
    Ok(outcome(plan, &client_dir, &data_dir, builds.missing(), log.path()))
}

/// Starts the job thread. Returns the job id.
pub fn start(ctx: Arc<DeviceCtx>, st: Arc<AppState>, plan: Plan, resume: bool) -> String {
    let job = random_id();
    let cancel = Arc::new(AtomicBool::new(false));
    lock_or_recover(&st.jobs).insert(job.clone(), Job { vault: String::new(), cancel: cancel.clone() });
    let id = job.clone();
    let _ = std::thread::Builder::new().name("nv-device".into()).spawn(move || {
        let log = Log::open(&ctx.log_dir);
        lock_or_recover(&ctx.logs).insert(id.clone(), log.path().to_path_buf());
        let r = execute(&ctx, &st, &id, &cancel, &plan, resume, &log);
        if let Err(e) = &r {
            log.event("error", json!({ "code": e.code, "message": e.message }));
        }
        lock_or_recover(&st.jobs).remove(&id);
        let outcome = r.as_ref().ok().cloned();
        if let Some(o) = &outcome {
            lock_or_recover(&ctx.outcomes).insert(id.clone(), o.clone());
        }
        st.emit("job-done", json!({ "job": id, "error": r.err(), "outcome": outcome, "log": log.path().to_string_lossy() }));
    });
    job
}
