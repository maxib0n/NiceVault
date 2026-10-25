//! The disk operations behind an interface: the OS implementation (udisks2 / elevated helper /
//! diskutil), a fake one for tests, e2e and mock parity (`NICEVAULT_FAKE_DISKS=<file.json>`),
//! and a dry run (`NICEVAULT_DEVICE_DRY_RUN=1`) that logs every command it would run and runs none.

use super::commands::{self, DbusCall};
use super::job::Log;
use super::{CLIENT_LABEL, DATA_LABEL, DataFs, Disk, NvState, Partition, Plan};
use crate::error::{ApiError, ApiResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::Mutex;
use std::time::Duration;

pub trait DiskOps: Send + Sync {
    fn name(&self) -> &'static str;
    /// Every whole disk (read-only, never prompts).
    fn disks(&self) -> Vec<Disk>;
    /// The mkfs tool that is missing for `fs` (Linux; None = it can be created).
    fn missing_tool(&self, fs: DataFs) -> Option<String>;
    /// The privileged part: unmount, partition table, both partitions formatted. `step(name)`
    /// is called as each step starts.
    fn partition(&self, plan: &Plan, log: &Log, step: &mut dyn FnMut(&str)) -> ApiResult<()>;
    /// Mounts both partitions (found by label): (NiceVault root, VaultData root).
    fn mount(&self, plan: &Plan, log: &Log) -> ApiResult<(PathBuf, PathBuf)>;
    fn eject(&self, disk: &Disk, log: &Log) -> ApiResult<()>;
    /// Where `disk`'s NiceVault partition is mounted, if it is (read-only; the dry run: its
    /// temp folder, never the real partition).
    fn client_root(&self, disk: &Disk) -> Option<PathBuf> {
        disk.by_label(CLIENT_LABEL)?.mountpoints.iter().find(|m| !m.is_empty()).map(PathBuf::from)
    }
    /// Mounts only the NiceVault partition, without privileges ("Aggiorna i programmi").
    fn mount_client(&self, _disk: &Disk, _log: &Log) -> ApiResult<PathBuf> {
        Err(not_mounted())
    }
    /// Tests: an error injected at a step (the fake's `fail` / `fail_at`).
    fn fault(&self, _step: &str) -> Option<ApiError> {
        None
    }
    /// Room left on `disk`'s NiceVault partition mounted at `root` (None: unknown). The fake:
    /// its partition's size less what its folder holds (so a 128 MiB stick is really 128 MiB).
    fn client_free(&self, _disk: &Disk, root: &Path) -> Option<u64> {
        let f = super::job::free_space(root);
        (f > 0).then_some(f)
    }
}

/// From the environment: fake file, dry run (over the OS or the fake), else the OS.
pub fn detect() -> Box<dyn DiskOps> {
    let inner: Box<dyn DiskOps> = match std::env::var_os("NICEVAULT_FAKE_DISKS") {
        Some(p) if !p.is_empty() => Box::new(Fake::new(PathBuf::from(p))),
        _ => os(),
    };
    if std::env::var("NICEVAULT_DEVICE_TRACE").is_ok_and(|v| v == "1") {
        eprintln!("[device] disks from: {}", inner.name());
    }
    if std::env::var("NICEVAULT_DEVICE_DRY_RUN").is_ok_and(|v| v == "1") { Box::new(DryRun::new(inner)) } else { inner }
}

pub fn os() -> Box<dyn DiskOps> {
    #[cfg(target_os = "linux")]
    return Box::new(Linux::new(Box::new(run_real)));
    #[cfg(windows)]
    return Box::new(Windows);
    #[cfg(target_os = "macos")]
    return Box::new(Mac::new(Box::new(run_real)));
    #[allow(unreachable_code)]
    Box::new(DryRun::new(Box::new(Fake::new(PathBuf::new()))))
}

/// Runs a program (tests swap it for a recorder).
pub type Runner = Box<dyn Fn(&str, &[String]) -> std::io::Result<Output> + Send + Sync>;

pub fn run_real(prog: &str, args: &[String]) -> std::io::Result<Output> {
    std::process::Command::new(prog).args(args).stdin(std::process::Stdio::null()).output()
}

#[cfg(test)]
pub(crate) fn ok_output(stdout: &str) -> Output {
    #[cfg(unix)]
    let status = std::os::unix::process::ExitStatusExt::from_raw(0);
    #[cfg(windows)]
    let status = std::os::windows::process::ExitStatusExt::from_raw(0);
    Output { status, stdout: stdout.as_bytes().to_vec(), stderr: vec![] }
}

/// Runs and logs one command; Err((exit code, stderr)) when it fails.
fn logged(run: &Runner, log: &Log, prog: &str, args: &[String]) -> Result<String, (Option<i32>, String)> {
    let t0 = std::time::Instant::now();
    match run(prog, args) {
        Ok(o) => {
            let (out, err) = (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned());
            log.command(prog, args, o.status.code(), &out, &err, t0.elapsed());
            if o.status.success() { Ok(out) } else { Err((o.status.code(), err)) }
        }
        Err(e) => {
            log.command(prog, args, None, "", &e.to_string(), t0.elapsed());
            Err((None, e.to_string()))
        }
    }
}

/// Waits for `f` to give a value (udev / the OS mounting new partitions takes a moment).
fn wait_for<T>(secs: u64, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let t0 = std::time::Instant::now();
    loop {
        if let Some(v) = f() {
            return Some(v);
        }
        if t0.elapsed() > Duration::from_secs(secs) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn find_disk(ops: &dyn DiskOps, plan: &Plan) -> Option<Disk> {
    ops.disks().into_iter().find(|d| d.same_as(&plan.disk))
}

fn not_mounted() -> ApiError {
    ApiError::new("stick_not_mounted", "The NiceVault partition is not mounted.")
}

fn gone() -> ApiError {
    ApiError::new("device_gone", "The drive was disconnected.")
}

// ---------------------------------------------------------------------------------------------
// Linux: udisks2 over gdbus

pub struct Linux {
    run: Runner,
}

impl Linux {
    pub fn new(run: Runner) -> Linux {
        Linux { run }
    }

    fn gdbus(&self) -> &'static str {
        if Path::new(commands::GDBUS).exists() { commands::GDBUS } else { "gdbus" }
    }

    fn call(&self, log: &Log, c: &DbusCall) -> Result<String, ApiError> {
        logged(&self.run, log, self.gdbus(), &c.argv()).map_err(|(_, e)| udisks_error(c.step, &e))
    }
}

/// An udisks / polkit error text -> the spec's code.
pub fn udisks_error(step: &str, stderr: &str) -> ApiError {
    let e = stderr;
    if e.contains("NotAuthorizedDismissed") || e.contains("Error.Cancelled") {
        ApiError::new("elevation_denied", "Permission was not granted.")
    } else if e.contains("NotAuthorized") || e.contains("ServiceUnknown") || e.contains("No such file or directory") && e.contains("gdbus") {
        ApiError::new("elevation_unavailable", e.trim())
    } else if e.contains("busy") || e.contains("Busy") {
        ApiError::new("device_busy", e.trim())
    } else if e.contains("UnknownObject") || e.contains("No such device") || e.contains("No such object") {
        gone()
    } else if step == "unmount" {
        ApiError::new("device_busy", e.trim())
    } else if step == "partition" {
        ApiError::new("partition_failed", e.trim())
    } else {
        ApiError::new("format_failed", e.trim())
    }
}

/// Where udisks keeps the mkfs tools.
const SBIN: [&str; 4] = ["/usr/sbin", "/sbin", "/usr/bin", "/bin"];

impl DiskOps for Linux {
    fn name(&self) -> &'static str {
        "udisks2"
    }
    fn disks(&self) -> Vec<Disk> {
        if cfg!(target_os = "linux") { super::enumerate::os_disks() } else { vec![] }
    }
    fn missing_tool(&self, fs: DataFs) -> Option<String> {
        // what udisks' CanFormat checks, without asking it (no D-Bus probe can prompt)
        let (tool, _) = fs.linux_tool()?;
        (!SBIN.iter().any(|d| Path::new(d).join(tool).exists())).then(|| tool.to_owned())
    }
    fn partition(&self, plan: &Plan, log: &Log, step: &mut dyn FnMut(&str)) -> ApiResult<()> {
        let mut last = "";
        for c in commands::linux_calls(plan) {
            if c.step != last {
                step(c.step);
                last = c.step;
            }
            self.call(log, &c)?;
        }
        Ok(())
    }
    fn mount(&self, plan: &Plan, log: &Log) -> ApiResult<(PathBuf, PathBuf)> {
        let mut roots = vec![];
        for label in [CLIENT_LABEL, DATA_LABEL] {
            let part = wait_for(15, || find_disk(self, plan).and_then(|d| d.by_label(label).cloned()))
                .ok_or_else(|| ApiError::new("device_gone", format!("{label} not found on the drive")))?;
            if let Some(m) = part.mountpoints.first() {
                roots.push(PathBuf::from(m));
                continue;
            }
            let reply = self.call(log, &commands::linux_mount(&commands::block_object(&part.path)))?;
            let m = commands::parse_string_reply(&reply).ok_or_else(|| ApiError::new("io", format!("mount: {reply}")))?;
            roots.push(PathBuf::from(m));
        }
        Ok((roots.remove(0), roots.remove(0)))
    }
    fn mount_client(&self, disk: &Disk, log: &Log) -> ApiResult<PathBuf> {
        // udisks Filesystem.Mount: filesystem-mount is allow_active=yes (no password locally)
        let part = disk.by_label(CLIENT_LABEL).ok_or_else(not_mounted)?;
        if let Some(m) = part.mountpoints.first() {
            return Ok(PathBuf::from(m));
        }
        let reply = self.call(log, &commands::linux_mount(&commands::block_object(&part.path)))?;
        commands::parse_string_reply(&reply).map(PathBuf::from).ok_or_else(|| ApiError::new("io", format!("mount: {reply}")))
    }
    fn eject(&self, disk: &Disk, log: &Log) -> ApiResult<()> {
        for p in disk.partitions.iter().filter(|p| !p.mountpoints.is_empty()) {
            let c = DbusCall {
                step: "unmount",
                object: commands::block_object(&p.path),
                method: format!("{}.Filesystem.Unmount", commands::UDISKS),
                args: vec!["{'auth.no_user_interaction': <false>}".into()],
            };
            self.call(log, &c)?;
        }
        let prop = DbusCall {
            step: "eject",
            object: commands::block_object(&disk.os_path),
            method: "org.freedesktop.DBus.Properties.Get".into(),
            args: vec![commands::gv_str("org.freedesktop.UDisks2.Block"), commands::gv_str("Drive")],
        };
        let drive = commands::parse_object_path(&self.call(log, &prop)?).ok_or_else(|| ApiError::new("io", "no drive object"))?;
        let off = DbusCall {
            step: "eject",
            object: drive,
            method: format!("{}.Drive.PowerOff", commands::UDISKS),
            args: vec!["{'auth.no_user_interaction': <false>}".into()],
        };
        self.call(log, &off).map(|_| ())
    }
}

// ---------------------------------------------------------------------------------------------
// Windows: UAC-elevated helper (same exe, `--device-helper <plan.json>`)

#[cfg(windows)]
pub struct Windows;

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    #[repr(C)]
    pub struct ShellExecuteInfoW {
        pub cb_size: u32,
        pub f_mask: u32,
        pub hwnd: *mut c_void,
        pub lp_verb: *const u16,
        pub lp_file: *const u16,
        pub lp_parameters: *const u16,
        pub lp_directory: *const u16,
        pub n_show: i32,
        pub h_inst_app: *mut c_void,
        pub lp_id_list: *mut c_void,
        pub lp_class: *const u16,
        pub hkey_class: *mut c_void,
        pub dw_hot_key: u32,
        pub h_icon_or_monitor: *mut c_void,
        pub h_process: *mut c_void,
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        pub fn ShellExecuteExW(info: *mut ShellExecuteInfoW) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn WaitForSingleObject(h: *mut c_void, ms: u32) -> u32;
        pub fn GetExitCodeProcess(h: *mut c_void, code: *mut u32) -> i32;
        pub fn CloseHandle(h: *mut c_void) -> i32;
        pub fn GetLastError() -> u32;
    }
    pub const SEE_MASK_NOCLOSEPROCESS: u32 = 0x40;
    pub const SEE_MASK_NOASYNC: u32 = 0x100;
    pub const SW_HIDE: i32 = 0;
    pub const ERROR_CANCELLED: u32 = 1223;
    pub const WAIT_TIMEOUT: u32 = 0x102;
    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }
}

/// Exit code of the helper -> error (None = success).
pub fn helper_exit_error(code: u32, last_step: &str) -> Option<ApiError> {
    match code {
        0 => None,
        c if c == commands::EXIT_CHANGED as u32 => Some(ApiError::new("device_changed", "The drive changed since it was chosen.")),
        c if c == commands::EXIT_REFUSED as u32 => Some(ApiError::new("device_refused", "system")),
        _ if matches!(last_step, "check" | "unmount" | "partition" | "") => Some(ApiError::new("partition_failed", format!("exit code {code}"))),
        _ => Some(ApiError::new("format_failed", format!("exit code {code}"))),
    }
}

#[cfg(windows)]
impl DiskOps for Windows {
    fn name(&self) -> &'static str {
        "windows-storage"
    }
    fn disks(&self) -> Vec<Disk> {
        super::enumerate::os_disks()
    }
    fn missing_tool(&self, _fs: DataFs) -> Option<String> {
        None
    }
    fn partition(&self, plan: &Plan, log: &Log, step: &mut dyn FnMut(&str)) -> ApiResult<()> {
        use win::*;
        let dir = std::env::temp_dir().join(format!("nv-device-{}", crate::state::random_id()));
        std::fs::create_dir_all(&dir).map_err(ApiError::io)?;
        let plan_file = dir.join("plan.json");
        let progress = dir.join("progress.jsonl");
        let req = super::helper::Request { plan: plan.clone(), progress: progress.to_string_lossy().into_owned() };
        std::fs::write(&plan_file, serde_json::to_vec(&req).unwrap()).map_err(ApiError::io)?;
        let exe = std::env::current_exe().map_err(ApiError::io)?;
        let params = format!("--device-helper \"{}\"", plan_file.display());
        log.event("elevate", serde_json::json!({ "exe": exe.to_string_lossy(), "params": params }));
        let (verb, file, par) = (wide("runas"), wide(&exe.to_string_lossy()), wide(&params));
        let mut info: ShellExecuteInfoW = unsafe { std::mem::zeroed() };
        info.cb_size = std::mem::size_of::<ShellExecuteInfoW>() as u32;
        info.f_mask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lp_verb = verb.as_ptr();
        info.lp_file = file.as_ptr();
        info.lp_parameters = par.as_ptr();
        info.n_show = SW_HIDE;
        // SAFETY: the struct and the wide strings outlive the call; the handle is closed below.
        let ok = unsafe { ShellExecuteExW(&mut info) };
        if ok == 0 {
            let e = unsafe { GetLastError() };
            let _ = std::fs::remove_dir_all(&dir);
            return Err(if e == ERROR_CANCELLED {
                ApiError::new("elevation_denied", "Permission was not granted.")
            } else {
                ApiError::new("elevation_unavailable", format!("ShellExecuteEx error {e}"))
            });
        }
        let mut seen = 0usize;
        let mut last = String::new();
        let mut tail = |step: &mut dyn FnMut(&str), last: &mut String| {
            let text = std::fs::read_to_string(&progress).unwrap_or_default();
            for line in text.lines().skip(seen) {
                seen += 1;
                log.event("helper", serde_json::from_str(line).unwrap_or(serde_json::Value::String(line.to_owned())));
                if let Some(s) = serde_json::from_str::<serde_json::Value>(line).ok().and_then(|v| v["step"].as_str().map(str::to_owned))
                    && s != "done"
                {
                    step(&s);
                    *last = s;
                }
            }
        };
        let code = unsafe {
            while WaitForSingleObject(info.h_process, 200) == WAIT_TIMEOUT {
                tail(step, &mut last);
            }
            let mut code = 1u32;
            GetExitCodeProcess(info.h_process, &mut code);
            CloseHandle(info.h_process);
            code
        };
        tail(step, &mut last);
        let _ = std::fs::remove_dir_all(&dir);
        helper_exit_error(code, &last).map_or(Ok(()), Err)
    }
    fn mount(&self, plan: &Plan, _log: &Log) -> ApiResult<(PathBuf, PathBuf)> {
        let roots = wait_for(30, || {
            let d = find_disk(self, plan)?;
            let c = d.by_label(CLIENT_LABEL)?.mountpoints.first()?.clone();
            let v = d.by_label(DATA_LABEL)?.mountpoints.first()?.clone();
            Some((PathBuf::from(c), PathBuf::from(v)))
        });
        roots.ok_or_else(|| ApiError::new("device_gone", "The new partitions did not get a drive letter."))
    }
    fn eject(&self, disk: &Disk, log: &Log) -> ApiResult<()> {
        let letters: Vec<String> = disk.mountpoints().map(|m| m.trim_end_matches('\\').to_owned()).collect();
        let script = letters
            .iter()
            .map(|l| format!("(New-Object -ComObject Shell.Application).Namespace(17).ParseName({}).InvokeVerb('Eject')", commands::ps_str(l)))
            .collect::<Vec<_>>()
            .join("; ");
        log.event("eject", serde_json::json!({ "script": script }));
        super::enumerate::powershell(&script).map(|_| ()).map_err(ApiError::io)
    }
}

// ---------------------------------------------------------------------------------------------
// macOS: diskutil, admin only if needed

pub struct Mac {
    run: Runner,
}

impl Mac {
    pub fn new(run: Runner) -> Mac {
        Mac { run }
    }
}

impl DiskOps for Mac {
    fn name(&self) -> &'static str {
        "diskutil"
    }
    fn disks(&self) -> Vec<Disk> {
        if cfg!(target_os = "macos") { super::enumerate::os_disks() } else { vec![] }
    }
    fn missing_tool(&self, _fs: DataFs) -> Option<String> {
        None
    }
    fn partition(&self, plan: &Plan, log: &Log, step: &mut dyn FnMut(&str)) -> ApiResult<()> {
        step("check");
        // BSD numbers can be reused after a replug: same identifier, size, still external
        match find_disk(self, plan) {
            Some(d) if super::is_external(&d) || d.is_virtual => {}
            Some(_) => return Err(ApiError::new("device_changed", "The drive changed since it was chosen.")),
            None => return Err(gone()),
        }
        step("unmount");
        logged(&self.run, log, commands::DISKUTIL, &commands::mac_unmount_args(plan)).map_err(|(_, e)| ApiError::new("device_busy", e.trim()))?;
        step("partition");
        let args = commands::mac_partition_args(plan);
        match logged(&self.run, log, commands::DISKUTIL, &args) {
            Ok(_) => Ok(()),
            Err((_, e)) if commands::mac_needs_admin(&e) => {
                let script = commands::mac_osascript(&args, &commands::mac_prompt(&plan.disk, true));
                logged(&self.run, log, commands::OSASCRIPT, &["-e".into(), script]).map(|_| ()).map_err(|(_, e)| {
                    if e.contains("-128") || e.to_lowercase().contains("user canceled") {
                        ApiError::new("elevation_denied", "Permission was not granted.")
                    } else {
                        ApiError::new("partition_failed", e.trim())
                    }
                })
            }
            Err((_, e)) => Err(ApiError::new("partition_failed", e.trim())),
        }
    }
    fn mount(&self, plan: &Plan, log: &Log) -> ApiResult<(PathBuf, PathBuf)> {
        let mut roots = vec![];
        for label in [CLIENT_LABEL, DATA_LABEL] {
            let part = wait_for(20, || find_disk(self, plan).and_then(|d| d.by_label(label).cloned()))
                .ok_or_else(|| ApiError::new("device_gone", format!("{label} not found on the drive")))?;
            if part.mountpoints.is_empty() {
                let _ = logged(&self.run, log, commands::DISKUTIL, &["mount".into(), part.path.clone()]);
            }
            let m = wait_for(15, || find_disk(self, plan).and_then(|d| d.by_label(label)?.mountpoints.first().cloned()))
                .ok_or_else(|| ApiError::new("io", format!("{label} was not mounted")))?;
            roots.push(PathBuf::from(m));
        }
        Ok((roots.remove(0), roots.remove(0)))
    }
    fn mount_client(&self, disk: &Disk, log: &Log) -> ApiResult<PathBuf> {
        let part = disk.by_label(CLIENT_LABEL).ok_or_else(not_mounted)?;
        if let Some(m) = part.mountpoints.first() {
            return Ok(PathBuf::from(m));
        }
        let _ = logged(&self.run, log, commands::DISKUTIL, &["mount".into(), part.path.clone()]);
        let id = disk.id.clone();
        wait_for(15, || self.disks().into_iter().find(|d| d.id == id).and_then(|d| d.by_label(CLIENT_LABEL)?.mountpoints.first().cloned()))
            .map(PathBuf::from)
            .ok_or_else(not_mounted)
    }
    fn eject(&self, disk: &Disk, log: &Log) -> ApiResult<()> {
        logged(&self.run, log, commands::DISKUTIL, &["eject".into(), commands::mac_device(disk)]).map(|_| ()).map_err(|(_, e)| ApiError::new("device_busy", e.trim()))
    }
}

// ---------------------------------------------------------------------------------------------
// Fake: disks in a JSON file, partitions as folders

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FakeFile {
    pub disks: Vec<Disk>,
    /// Folder for the partitions' contents (`<root>/<disk>/NiceVault`, `.../VaultData`).
    pub root: String,
    /// An error code to fail with (elevation_denied, partition_failed, device_gone, ...) ...
    pub fail: Option<String>,
    /// ... at this step (default: partition).
    pub fail_at: Option<String>,
    /// mkfs tools to report missing.
    pub missing_tools: Vec<String>,
    /// Milliseconds per privileged step (so the UI shows them).
    pub step_ms: Option<u64>,
}

pub struct Fake {
    file: PathBuf,
    lock: Mutex<()>,
}

impl Fake {
    pub fn new(file: PathBuf) -> Fake {
        Fake { file, lock: Mutex::new(()) }
    }
    pub fn read(&self) -> FakeFile {
        std::fs::read(&self.file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    fn write(&self, f: &FakeFile) {
        let tmp = self.file.with_extension("tmp");
        if std::fs::write(&tmp, serde_json::to_vec_pretty(f).unwrap()).is_ok() {
            let _ = std::fs::rename(tmp, &self.file);
        }
    }
    fn dir(f: &FakeFile, d: &Disk) -> PathBuf {
        let safe: String = d.id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        PathBuf::from(&f.root).join(safe)
    }
    fn delay(f: &FakeFile) {
        std::thread::sleep(Duration::from_millis(f.step_ms.unwrap_or(150)));
    }
}

impl DiskOps for Fake {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn disks(&self) -> Vec<Disk> {
        let mut v = self.read().disks;
        for d in &mut v {
            d.nicevault = super::state_of(d);
        }
        v
    }
    fn missing_tool(&self, fs: DataFs) -> Option<String> {
        let (tool, _) = fs.linux_tool()?;
        self.read().missing_tools.iter().any(|t| t == tool).then(|| tool.to_owned())
    }
    fn fault(&self, step: &str) -> Option<ApiError> {
        let f = self.read();
        let code = f.fail?;
        (f.fail_at.as_deref().unwrap_or("partition") == step).then(|| ApiError::new(&code, format!("fake failure at {step}")))
    }
    fn client_free(&self, disk: &Disk, root: &Path) -> Option<u64> {
        let now = self.disks().into_iter().find(|d| d.same_as(disk));
        match now.as_ref().and_then(|d| d.by_label(CLIENT_LABEL)).map(|p| p.size).filter(|s| *s > 0) {
            Some(size) => Some(size.saturating_sub(super::client::dir_size(root))),
            None => {
                let f = super::job::free_space(root);
                (f > 0).then_some(f)
            }
        }
    }
    fn partition(&self, plan: &Plan, log: &Log, step: &mut dyn FnMut(&str)) -> ApiResult<()> {
        let _g = crate::state::lock_or_recover(&self.lock);
        let mut f = self.read();
        let Some(i) = f.disks.iter().position(|d| d.same_as(&plan.disk)) else { return Err(gone()) };
        for c in commands::preview(plan) {
            log.event("fake_command", serde_json::json!({ "command": c }));
        }
        for s in ["unmount", "partition", "format_client", "format_data"] {
            step(s);
            if let Some(e) = self.fault(s) {
                return Err(e);
            }
            Self::delay(&f);
        }
        let dir = Self::dir(&f, &f.disks[i]);
        let _ = std::fs::remove_dir_all(&dir);
        let (c, d) = (dir.join(CLIENT_LABEL), dir.join(DATA_LABEL));
        std::fs::create_dir_all(&c).and_then(|_| std::fs::create_dir_all(&d)).map_err(ApiError::io)?;
        let disk = &mut f.disks[i];
        disk.partitions = vec![
            Partition { path: format!("{}1", disk.id), label: CLIENT_LABEL.into(), fs: "exfat".into(), size: plan.client.size, mountpoints: vec![] },
            Partition { path: format!("{}2", disk.id), label: DATA_LABEL.into(), fs: plan.data.fs.clone(), size: plan.data_size, mountpoints: vec![] },
        ];
        disk.nicevault = NvState::Incomplete;
        self.write(&f);
        Ok(())
    }
    fn mount(&self, plan: &Plan, _log: &Log) -> ApiResult<(PathBuf, PathBuf)> {
        let _g = crate::state::lock_or_recover(&self.lock);
        let mut f = self.read();
        let root = f.root.clone();
        let Some(disk) = f.disks.iter_mut().find(|d| d.same_as(&plan.disk)) else { return Err(gone()) };
        if disk.by_label(CLIENT_LABEL).is_none() || disk.by_label(DATA_LABEL).is_none() {
            return Err(ApiError::new("device_changed", "The layout is not a NiceVault device."));
        }
        let ff = FakeFile { root, ..Default::default() };
        let dir = Self::dir(&ff, disk);
        let (c, d) = (dir.join(CLIENT_LABEL), dir.join(DATA_LABEL));
        std::fs::create_dir_all(&c).and_then(|_| std::fs::create_dir_all(&d)).map_err(ApiError::io)?;
        for p in &mut disk.partitions {
            let m = if p.label == CLIENT_LABEL { &c } else { &d };
            p.mountpoints = vec![m.to_string_lossy().into_owned()];
        }
        self.write(&f);
        Ok((c, d))
    }
    fn mount_client(&self, disk: &Disk, _log: &Log) -> ApiResult<PathBuf> {
        let _g = crate::state::lock_or_recover(&self.lock);
        let mut f = self.read();
        if let Some(e) = self.fault("mount_client") {
            return Err(e);
        }
        let dir = Self::dir(&f, disk).join(CLIENT_LABEL);
        let Some(d) = f.disks.iter_mut().find(|d| d.same_as(disk)) else { return Err(gone()) };
        let Some(p) = d.partitions.iter_mut().find(|p| p.label.eq_ignore_ascii_case(CLIENT_LABEL)) else { return Err(not_mounted()) };
        std::fs::create_dir_all(&dir).map_err(ApiError::io)?;
        p.mountpoints = vec![dir.to_string_lossy().into_owned()];
        self.write(&f);
        Ok(dir)
    }
    fn eject(&self, disk: &Disk, log: &Log) -> ApiResult<()> {
        let _g = crate::state::lock_or_recover(&self.lock);
        let mut f = self.read();
        f.disks.retain(|d| !d.same_as(disk));
        log.event("fake_eject", serde_json::json!({ "id": disk.id }));
        self.write(&f);
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Dry run: the full flow, every command logged, none executed

pub struct DryRun {
    inner: Box<dyn DiskOps>,
}

impl DryRun {
    pub fn new(inner: Box<dyn DiskOps>) -> DryRun {
        DryRun { inner }
    }
    fn dir(d: &Disk) -> PathBuf {
        let safe: String = d.id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        std::env::temp_dir().join("nicevault-dry-run").join(safe)
    }
}

impl DiskOps for DryRun {
    fn name(&self) -> &'static str {
        "dry-run"
    }
    fn disks(&self) -> Vec<Disk> {
        self.inner.disks()
    }
    fn missing_tool(&self, fs: DataFs) -> Option<String> {
        self.inner.missing_tool(fs)
    }
    fn fault(&self, step: &str) -> Option<ApiError> {
        self.inner.fault(step)
    }
    fn partition(&self, plan: &Plan, log: &Log, step: &mut dyn FnMut(&str)) -> ApiResult<()> {
        let pv = commands::preview(plan);
        for s in ["unmount", "partition", "format_client", "format_data"] {
            step(s);
            std::thread::sleep(Duration::from_millis(100));
        }
        for c in pv {
            log.event("dry_run", serde_json::json!({ "would_run": c }));
        }
        Ok(())
    }
    fn mount(&self, plan: &Plan, log: &Log) -> ApiResult<(PathBuf, PathBuf)> {
        let dir = Self::dir(&plan.disk);
        let (c, d) = (dir.join(CLIENT_LABEL), dir.join(DATA_LABEL));
        std::fs::create_dir_all(&c).and_then(|_| std::fs::create_dir_all(&d)).map_err(ApiError::io)?;
        log.event("dry_run", serde_json::json!({ "mount_instead": dir.to_string_lossy() }));
        Ok((c, d))
    }
    fn client_root(&self, disk: &Disk) -> Option<PathBuf> {
        // never the real partition: its stand-in under the temp folder, once "mounted"
        let c = Self::dir(disk).join(CLIENT_LABEL);
        c.is_dir().then_some(c)
    }
    fn mount_client(&self, disk: &Disk, log: &Log) -> ApiResult<PathBuf> {
        let c = Self::dir(disk).join(CLIENT_LABEL);
        std::fs::create_dir_all(&c).map_err(ApiError::io)?;
        log.event("dry_run", serde_json::json!({ "mount_client_instead": c.to_string_lossy() }));
        Ok(c)
    }
    fn eject(&self, disk: &Disk, log: &Log) -> ApiResult<()> {
        log.event("dry_run", serde_json::json!({ "would_eject": disk.id }));
        Ok(())
    }
}
