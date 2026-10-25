//! The drives of this computer, live: what "Nuovo volume" offers as places for a new volume and
//! what tells Benvenuto to re-scan when a drive is plugged in or pulled out.
//!
//! Enumeration is read-only everywhere and never asks for a password:
//!  - Linux: `/proc/self/mountinfo` (block-device mounts, one entry per device), labels from
//!    `/dev/disk/by-label`, removable/USB from sysfs, sizes from `statvfs`;
//!  - Windows: `GetLogicalDrives` + `GetDriveTypeW` (removable and fixed), `GetVolumeInformationW`
//!    (label, file system), `GetDiskFreeSpaceExW`;
//!  - macOS: `getmntinfo` (local `/dev/disk*` mounts), DiskArbitration descriptions (internal or
//!    not, volume name).
//!
//! Hot-plug: a watcher thread re-enumerates when the OS says something changed and, as a fallback,
//! every [`POLL`]; it emits `drives-changed { drives }` only when the set of drives changed (not
//! for free space alone):
//!  - Linux: `gdbus monitor --system --dest org.freedesktop.UDisks2` (a signal subscription:
//!    InterfacesAdded / InterfacesRemoved / MountPoints changes) and `poll(POLLPRI)` on
//!    `/proc/self/mountinfo`, which the kernel signals on every mount / unmount;
//!  - Windows: `WM_DEVICECHANGE` (`DBT_DEVICEARRIVAL` / `DBT_DEVICEREMOVECOMPLETE`) in the hidden
//!    window of watch.rs;
//!  - macOS: DiskArbitration appeared / disappeared / description-changed callbacks.
//!
//! Test hook: `NICEVAULT_FAKE_DRIVES=<file.json>` replaces the OS with a JSON array of drives
//! (see [`Drive`]; re-read at every poll), so the e2e suites can plug and pull drives.

use crate::state::{AppState, lock_or_recover};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Duration;

/// Fallback re-enumeration interval (the OS signals usually come first).
pub const POLL: Duration = Duration::from_secs(2);
/// With a fake source (tests): faster, so plugging a fake drive shows quickly.
const FAKE_POLL: Duration = Duration::from_millis(400);
/// Partition labels of a NiceVault device.
pub const DEVICE_LABELS: [&str; 2] = ["VaultData", "NiceVault"];

/// One mounted drive. `label` may be empty (the UI then names it from `mount_path`, or "this
/// computer" when `system`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Drive {
    /// Stable while mounted: Linux the device path, Windows "E:", macOS the BSD name.
    pub id: String,
    pub label: String,
    /// Root of the drive as mounted (`/run/media/u/STICK`, `E:\`, `/Volumes/STICK`).
    pub mount_path: String,
    /// Where a new volume goes when this drive is picked: the app's folder when the executable
    /// runs from this drive, the Documents folder on the drive holding the home, else the root.
    pub folder: String,
    pub fs_type: Option<String>,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// Removable or external (USB stick, SD card, external disk), as opposed to internal.
    pub removable: bool,
    /// The drive holding the OS / the user's home.
    pub system: bool,
    /// The executable runs from this drive (portable use).
    pub app_here: bool,
    /// A partition labelled VaultData / NiceVault (a NiceVault device).
    pub is_nicevault_device: bool,
}

impl Drive {
    /// What makes two lists "different" for `drives-changed` (free space alone does not).
    fn key(&self) -> (&str, &str, &str, &Option<String>, bool, bool) {
        (&self.id, &self.mount_path, &self.label, &self.fs_type, self.removable, self.is_nicevault_device)
    }
}

pub fn is_device_label(label: &str) -> bool {
    DEVICE_LABELS.iter().any(|l| l.eq_ignore_ascii_case(label.trim()))
}

fn same_set(a: &[Drive], b: &[Drive]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.key() == y.key())
}

/// A drive as enumerated, before the app-relative fields are filled in.
#[derive(Clone, Debug, Default)]
pub struct Raw {
    pub drive: Drive,
    /// Every mount point of this device (btrfs subvolumes, bind mounts): used to tell which drive
    /// holds a folder.
    pub mounts: Vec<PathBuf>,
}

/// Fills `folder`, `system`, `app_here`, `is_nicevault_device` and sorts: the app's drive first,
/// then removable ones, then the system drive, then by label.
pub fn finish(raws: Vec<Raw>, exe_dir: Option<&Path>, home: Option<&Path>, documents: Option<&Path>) -> Vec<Drive> {
    // the drive holding `p` = the one with the longest mount point that is a prefix of it
    let owner = |p: &Path| -> Option<usize> {
        raws.iter()
            .enumerate()
            .flat_map(|(i, r)| r.mounts.iter().map(move |m| (i, m)))
            .filter(|(_, m)| p.starts_with(m))
            .max_by_key(|(_, m)| m.as_os_str().len())
            .map(|(i, _)| i)
    };
    let app = exe_dir.and_then(owner);
    let sys = home.and_then(owner);
    let mut out: Vec<Drive> = raws
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut d = r.drive.clone();
            d.app_here = app == Some(i);
            d.system |= sys == Some(i);
            d.is_nicevault_device |= is_device_label(&d.label);
            if d.folder.is_empty() {
                d.folder = match (d.app_here, d.system) {
                    (true, _) if exe_dir.is_some_and(crate::store::dir_writable) => exe_dir.unwrap().to_string_lossy().into_owned(),
                    (_, true) => documents.filter(|p| p.is_dir()).or(home).map_or(d.mount_path.clone(), |p| p.to_string_lossy().into_owned()),
                    _ => d.mount_path.clone(),
                };
            }
            d
        })
        .collect();
    out.sort_by_key(|d| (!d.app_here, !d.removable, !d.system, d.label.to_lowercase(), d.mount_path.clone()));
    out
}

// ---------------------------------------------------------------------------
// sources

/// Where the list comes from.
pub enum Source {
    Os,
    /// `NICEVAULT_FAKE_DRIVES`: a JSON array of [`Drive`] (missing file = no drives).
    FakeFile(PathBuf),
    /// Tests.
    Custom(Box<dyn Fn() -> Vec<Drive> + Send + Sync>),
}

impl Source {
    pub fn detect() -> Source {
        match std::env::var_os("NICEVAULT_FAKE_DRIVES") {
            Some(p) if !p.is_empty() => Source::FakeFile(PathBuf::from(p)),
            _ => Source::Os,
        }
    }

    pub fn list(&self) -> Vec<Drive> {
        match self {
            Source::Os => os_list(),
            Source::FakeFile(p) => fake_list(p),
            Source::Custom(f) => f(),
        }
    }
}

fn exe_dir_for_drives() -> Option<PathBuf> {
    std::env::var_os("NICEVAULT_EXE_DIR").map(PathBuf::from).or_else(crate::store::exe_dir)
}

fn fake_list(p: &Path) -> Vec<Drive> {
    let drives: Vec<Drive> = std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let raws = drives
        .into_iter()
        .map(|d| {
            let mounts = vec![PathBuf::from(&d.mount_path)];
            Raw { drive: d, mounts }
        })
        .collect();
    finish(raws, exe_dir_for_drives().as_deref(), None, None)
}

/// The mounted drives of this machine.
pub fn os_list() -> Vec<Drive> {
    let raws = os::enumerate();
    let home = dirs::home_dir();
    let docs = dirs::document_dir();
    finish(raws, exe_dir_for_drives().as_deref(), home.as_deref(), docs.as_deref())
}

/// Roots of the removable drives Benvenuto scans (store.rs), from the same source as the picker.
pub fn removable_roots() -> Vec<PathBuf> {
    Source::detect().list().into_iter().filter(|d| d.removable).map(|d| PathBuf::from(d.mount_path)).collect()
}

// ---------------------------------------------------------------------------
// watcher

static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

/// Something may have changed (an OS notification): re-enumerate now.
pub fn wake() {
    *lock_or_recover(&WAKE.0) = true;
    WAKE.1.notify_all();
}

/// The live list (Tauri managed state; `drives_list` reads it).
pub struct Drives {
    source: Source,
    last: Mutex<Vec<Drive>>,
}

impl Drives {
    pub fn new(source: Source) -> Arc<Drives> {
        let last = source.list();
        Arc::new(Drives { source, last: Mutex::new(last) })
    }

    /// Fresh enumeration (free space up to date); also updates what the watcher compares with.
    pub fn list(&self) -> Vec<Drive> {
        let now = self.source.list();
        *lock_or_recover(&self.last) = now.clone();
        now
    }

    /// Re-enumerates; `Some(list)` when the set of drives changed since the last look.
    pub fn rescan(&self) -> Option<Vec<Drive>> {
        let now = self.source.list();
        let mut last = lock_or_recover(&self.last);
        let changed = !same_set(&last, &now);
        *last = now.clone();
        changed.then_some(now)
    }

    /// One pass of the watcher: on a change, emits `drives-changed { drives }`. Returns whether it did.
    pub fn tick(&self, state: &AppState) -> bool {
        match self.rescan() {
            Some(drives) => {
                state.emit("drives-changed", serde_json::json!({ "drives": drives }));
                true
            }
            None => false,
        }
    }

    /// Starts the OS listeners and the watcher thread (ends when the app state is gone).
    pub fn spawn(self: &Arc<Self>, state: Weak<AppState>) {
        let fake = !matches!(self.source, Source::Os);
        if !fake {
            os::listen();
        }
        let me = self.clone();
        let every = if fake { FAKE_POLL } else { POLL };
        let _ = std::thread::Builder::new().name("nv-drives".into()).spawn(move || {
            loop {
                {
                    let g = lock_or_recover(&WAKE.0);
                    let (mut g, _) = WAKE.1.wait_timeout_while(g, every, |woken| !*woken).unwrap_or_else(|e| e.into_inner());
                    *g = false;
                }
                // a burst of notifications (a disk with several partitions) settles first
                std::thread::sleep(Duration::from_millis(150));
                *lock_or_recover(&WAKE.0) = false;
                let Some(s) = state.upgrade() else { return };
                me.tick(&s);
            }
        });
    }
}

pub mod cmd {
    use super::{Drive, Drives};
    use std::sync::Arc;

    #[tauri::command(rename_all = "snake_case")]
    pub async fn drives_list(drives: tauri::State<'_, Arc<Drives>>) -> crate::error::ApiResult<Vec<Drive>> {
        let d = drives.inner().clone();
        tauri::async_runtime::spawn_blocking(move || d.list())
            .await
            .map_err(|e| crate::error::ApiError::new("error", format!("Internal error: {e}")))
    }
}

// ---------------------------------------------------------------------------
// Linux

/// One line of /proc/self/mountinfo, the fields we use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountLine {
    pub devno: String,
    pub root: String,
    pub mount_point: String,
    pub read_only: bool,
    pub fs_type: String,
    pub source: String,
}

/// Parses one mountinfo line: `id parent major:minor root mountpoint opts [optional...] - fstype source superopts`.
pub fn parse_mountinfo_line(line: &str) -> Option<MountLine> {
    let (left, right) = line.split_once(" - ")?;
    let l: Vec<&str> = left.split(' ').collect();
    let r: Vec<&str> = right.split(' ').collect();
    if l.len() < 6 || r.len() < 2 {
        return None;
    }
    Some(MountLine {
        devno: l[2].to_owned(),
        root: unescape_mount(l[3]),
        mount_point: unescape_mount(l[4]),
        read_only: l[5].split(',').any(|o| o == "ro"),
        fs_type: r[0].to_owned(),
        source: unescape_mount(r[1]),
    })
}

/// mountinfo escapes space, tab, newline and backslash as \ooo (octal).
pub fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
            out.push(u8::from_str_radix(&s[i + 1..i + 4], 8).unwrap_or(b'?'));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `/dev/disk/by-label` names escape bytes as `\xHH` (udev).
pub fn unescape_udev(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && b.get(i + 1) == Some(&b'x')
            && i + 4 <= b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 2..i + 4], 16)
        {
            out.push(v);
            i += 4;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Mounts that are places for files: block devices, writable, not system plumbing.
pub fn is_user_mount(m: &MountLine) -> bool {
    const SKIP_FS: [&str; 4] = ["squashfs", "iso9660", "udf", "erofs"];
    const SKIP_UNDER: [&str; 8] = ["/boot", "/efi", "/snap", "/var/lib/snapd", "/var/lib/docker", "/var/lib/containers", "/sysroot", "/proc"];
    m.source.starts_with("/dev/")
        && !m.source.starts_with("/dev/loop")
        && !m.source.starts_with("/dev/zram")
        && !m.read_only
        && !SKIP_FS.contains(&m.fs_type.as_str())
        && !SKIP_UNDER.iter().any(|p| Path::new(&m.mount_point).starts_with(p))
}

/// Groups user mounts by device: one drive per source, represented by its shortest mount point
/// (the root of a btrfs device rather than a subvolume). `label_of(source)` and `info(source,
/// mount)` supply what is not in mountinfo.
pub fn group_mounts(
    lines: &[MountLine],
    label_of: impl Fn(&str) -> Option<String>,
    removable_of: impl Fn(&str) -> bool,
    space_of: impl Fn(&Path) -> (u64, u64),
) -> Vec<Raw> {
    let mut out: Vec<Raw> = vec![];
    for m in lines.iter().filter(|m| is_user_mount(m)) {
        let mp = PathBuf::from(&m.mount_point);
        if let Some(r) = out.iter_mut().find(|r| r.drive.id == m.source) {
            if m.mount_point.len() < r.drive.mount_path.len() {
                r.drive.mount_path = m.mount_point.clone();
            }
            r.mounts.push(mp);
            continue;
        }
        out.push(Raw {
            drive: Drive {
                id: m.source.clone(),
                label: String::new(),
                mount_path: m.mount_point.clone(),
                fs_type: Some(m.fs_type.clone()),
                removable: removable_of(&m.source),
                ..Default::default()
            },
            mounts: vec![mp],
        });
    }
    for r in &mut out {
        let mp = PathBuf::from(&r.drive.mount_path);
        r.drive.label = label_of(&r.drive.id).unwrap_or_else(|| {
            // udisks mounts at /run/media/$USER/<label>: the folder name is the label
            if r.drive.mount_path == "/" { String::new() } else { mp.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() }
        });
        (r.drive.total_bytes, r.drive.free_bytes) = space_of(&mp);
    }
    out
}

/// Whether a `gdbus monitor` line of UDisks2 means the drives may have changed.
pub fn udisks_line_changes_drives(line: &str) -> bool {
    line.contains("org.freedesktop.DBus.ObjectManager.InterfacesAdded")
        || line.contains("org.freedesktop.DBus.ObjectManager.InterfacesRemoved")
        || (line.contains("org.freedesktop.DBus.Properties.PropertiesChanged") && (line.contains("MountPoints") || line.contains("IdLabel")))
}

#[cfg(target_os = "linux")]
mod os {
    use super::*;
    use std::io::BufRead;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    fn labels() -> Vec<(PathBuf, String)> {
        let Ok(rd) = std::fs::read_dir("/dev/disk/by-label") else { return vec![] };
        rd.flatten().filter_map(|e| Some((std::fs::canonicalize(e.path()).ok()?, unescape_udev(&e.file_name().to_string_lossy())))).collect()
    }

    /// sysfs entry of a block device, following device-mapper (LUKS, LVM) down to its disk.
    fn removable(source: &str) -> bool {
        let Ok(dev) = std::fs::canonicalize(source) else { return false };
        let Some(name) = dev.file_name().map(|n| n.to_string_lossy().into_owned()) else { return false };
        let mut sys = match std::fs::canonicalize(format!("/sys/class/block/{name}")) {
            Ok(p) => p,
            Err(_) => return false,
        };
        for _ in 0..4 {
            // dm-N: look at the (first) device below it
            let below = std::fs::read_dir(sys.join("slaves")).ok().and_then(|mut rd| rd.next()).and_then(|e| e.ok()).map(|e| e.path());
            match below.and_then(|p| std::fs::canonicalize(p).ok()) {
                Some(p) => sys = p,
                None => break,
            }
        }
        let disk = if sys.join("partition").exists() { sys.parent().map(Path::to_path_buf).unwrap_or(sys) } else { sys };
        std::fs::read_to_string(disk.join("removable")).is_ok_and(|v| v.trim() == "1") || disk.to_string_lossy().contains("/usb")
    }

    pub(super) fn space(p: &Path) -> (u64, u64) {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else { return (0, 0) };
        // SAFETY: statvfs fills a zeroed struct from a NUL-terminated path.
        unsafe {
            let mut s: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c.as_ptr(), &mut s) != 0 {
                return (0, 0);
            }
            let f = s.f_frsize as u64;
            (s.f_blocks as u64 * f, s.f_bavail as u64 * f)
        }
    }

    pub(super) fn enumerate() -> Vec<Raw> {
        let info = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
        let lines: Vec<MountLine> = info.lines().filter_map(parse_mountinfo_line).collect();
        let labels = labels();
        let label_of = |src: &str| {
            let dev = std::fs::canonicalize(src).ok()?;
            labels.iter().find(|(p, _)| *p == dev).map(|(_, l)| l.clone())
        };
        group_mounts(&lines, label_of, removable, space)
    }

    pub(super) fn listen() {
        // the kernel flags /proc/self/mountinfo (POLLPRI) at every mount table change
        let _ = std::thread::Builder::new().name("nv-mounts".into()).spawn(|| {
            use std::os::fd::AsRawFd;
            let Ok(f) = std::fs::File::open("/proc/self/mountinfo") else { return };
            loop {
                let mut p = libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLPRI | libc::POLLERR, revents: 0 };
                // SAFETY: one valid pollfd for the duration of the call.
                let n = unsafe { libc::poll(&mut p, 1, 60_000) };
                if n < 0 {
                    return;
                }
                if n > 0 {
                    // re-arm: the event stays pending until this fd reads the table again
                    use std::io::{Read, Seek};
                    let _ = (&f).seek(std::io::SeekFrom::Start(0));
                    let _ = (&f).read_to_end(&mut Vec::new());
                    wake();
                }
            }
        });
        // UDisks2 signals (plug / unplug before the mount, label changes); a subscription only
        let mut cmd = Command::new("gdbus");
        cmd.args(["monitor", "--system", "--dest", "org.freedesktop.UDisks2"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        // SAFETY: prctl is async-signal-safe; the child dies with us.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
        let Ok(mut child) = cmd.spawn() else { return };
        let Some(out) = child.stdout.take() else { return };
        lock_or_recover(&MONITOR).replace(child);
        let _ = std::thread::Builder::new().name("nv-udisks".into()).spawn(move || {
            for line in std::io::BufReader::new(out).lines() {
                let Ok(line) = line else { return };
                if udisks_line_changes_drives(&line) {
                    wake();
                }
            }
        });
    }

    pub(super) static MONITOR: Mutex<Option<std::process::Child>> = Mutex::new(None);

    pub(super) fn shutdown() {
        if let Some(mut c) = lock_or_recover(&MONITOR).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

// ---------------------------------------------------------------------------
// Windows

#[cfg(windows)]
mod os {
    use super::*;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLogicalDrives() -> u32;
        fn GetDriveTypeW(root: *const u16) -> u32;
        fn GetVolumeInformationW(
            root: *const u16,
            name: *mut u16,
            name_len: u32,
            serial: *mut u32,
            max_comp: *mut u32,
            flags: *mut u32,
            fs: *mut u16,
            fs_len: u32,
        ) -> i32;
        fn GetDiskFreeSpaceExW(root: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
        fn SetThreadErrorMode(new: u32, old: *mut u32) -> i32;
    }
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    const SEM_FAILCRITICALERRORS: u32 = 1;
    const FILE_READ_ONLY_VOLUME: u32 = 0x0008_0000;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }
    fn from_wide(b: &[u16]) -> String {
        String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())])
    }

    pub(super) fn enumerate() -> Vec<Raw> {
        let mut out = vec![];
        let mut old = 0;
        // SAFETY: plain Win32 calls with NUL-terminated buffers that outlive them. The error mode
        // keeps an empty card reader from showing a "no disk" dialog.
        unsafe {
            SetThreadErrorMode(SEM_FAILCRITICALERRORS, &mut old);
            let mask = GetLogicalDrives();
            for i in 0..26u32 {
                if mask & (1 << i) == 0 {
                    continue;
                }
                let letter = (b'A' + i as u8) as char;
                let root_s = format!("{letter}:\\");
                let root = wide(&root_s);
                let kind = GetDriveTypeW(root.as_ptr());
                if kind != DRIVE_REMOVABLE && kind != DRIVE_FIXED {
                    continue;
                }
                let mut name = [0u16; 261];
                let mut fs = [0u16; 261];
                let (mut serial, mut maxc, mut flags) = (0, 0, 0);
                if GetVolumeInformationW(root.as_ptr(), name.as_mut_ptr(), 261, &mut serial, &mut maxc, &mut flags, fs.as_mut_ptr(), 261) == 0 {
                    continue; // no medium
                }
                if flags & FILE_READ_ONLY_VOLUME != 0 {
                    continue;
                }
                let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
                GetDiskFreeSpaceExW(root.as_ptr(), &mut avail, &mut total, &mut free);
                let sys = std::env::var("SystemDrive").is_ok_and(|s| s.eq_ignore_ascii_case(&format!("{letter}:")));
                out.push(Raw {
                    drive: Drive {
                        id: format!("{letter}:"),
                        label: from_wide(&name),
                        mount_path: root_s.clone(),
                        fs_type: Some(from_wide(&fs)).filter(|s| !s.is_empty()),
                        total_bytes: total,
                        free_bytes: avail,
                        removable: kind == DRIVE_REMOVABLE,
                        system: sys,
                        ..Default::default()
                    },
                    mounts: vec![PathBuf::from(&root_s)],
                });
            }
            SetThreadErrorMode(old, std::ptr::null_mut());
        }
        out
    }

    /// WM_DEVICECHANGE arrives in watch.rs's hidden window, which calls [`wake`].
    pub(super) fn listen() {}
    pub(super) fn shutdown() {}
}

// ---------------------------------------------------------------------------
// macOS

#[cfg(target_os = "macos")]
mod os {
    use super::*;
    use std::ffi::{CStr, c_char, c_void};

    type CfRef = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDictionaryGetValue(d: CfRef, k: CfRef) -> CfRef;
        fn CFBooleanGetValue(b: CfRef) -> u8;
        fn CFStringGetCString(s: CfRef, buf: *mut c_char, len: isize, enc: u32) -> u8;
        fn CFRelease(r: CfRef);
        fn CFRunLoopGetCurrent() -> CfRef;
        fn CFRunLoopRun();
        static kCFRunLoopDefaultMode: CfRef;
        static kCFAllocatorDefault: CfRef;
    }
    #[link(name = "DiskArbitration", kind = "framework")]
    unsafe extern "C" {
        fn DASessionCreate(alloc: CfRef) -> CfRef;
        fn DADiskCreateFromBSDName(alloc: CfRef, session: CfRef, name: *const c_char) -> CfRef;
        fn DADiskCopyDescription(disk: CfRef) -> CfRef;
        fn DASessionScheduleWithRunLoop(session: CfRef, rl: CfRef, mode: CfRef);
        fn DARegisterDiskAppearedCallback(session: CfRef, matching: CfRef, cb: extern "C" fn(CfRef, *mut c_void), ctx: *mut c_void);
        fn DARegisterDiskDisappearedCallback(session: CfRef, matching: CfRef, cb: extern "C" fn(CfRef, *mut c_void), ctx: *mut c_void);
        fn DARegisterDiskDescriptionChangedCallback(
            session: CfRef,
            matching: CfRef,
            watch: CfRef,
            cb: extern "C" fn(CfRef, CfRef, *mut c_void),
            ctx: *mut c_void,
        );
        static kDADiskDescriptionDeviceInternalKey: CfRef;
        static kDADiskDescriptionVolumeNameKey: CfRef;
    }
    const UTF8: u32 = 0x0800_0100;

    fn cstr(b: &[c_char]) -> String {
        // SAFETY: statfs strings are NUL-terminated within their fixed-size arrays.
        unsafe { CStr::from_ptr(b.as_ptr()) }.to_string_lossy().into_owned()
    }

    /// (internal, volume name) from DiskArbitration.
    fn describe(session: CfRef, bsd: &str) -> (Option<bool>, Option<String>) {
        let Ok(name) = std::ffi::CString::new(bsd) else { return (None, None) };
        // SAFETY: CF/DA calls with valid arguments; every created object is released.
        unsafe {
            let disk = DADiskCreateFromBSDName(kCFAllocatorDefault, session, name.as_ptr());
            if disk.is_null() {
                return (None, None);
            }
            let d = DADiskCopyDescription(disk);
            let mut out = (None, None);
            if !d.is_null() {
                let b = CFDictionaryGetValue(d, kDADiskDescriptionDeviceInternalKey);
                if !b.is_null() {
                    out.0 = Some(CFBooleanGetValue(b) != 0);
                }
                let s = CFDictionaryGetValue(d, kDADiskDescriptionVolumeNameKey);
                let mut buf = [0 as c_char; 512];
                if !s.is_null() && CFStringGetCString(s, buf.as_mut_ptr(), 512, UTF8) != 0 {
                    out.1 = Some(cstr(&buf)).filter(|s| !s.is_empty());
                }
                CFRelease(d);
            }
            CFRelease(disk);
            out
        }
    }

    pub(super) fn enumerate() -> Vec<Raw> {
        let mut out = vec![];
        // SAFETY: getmntinfo returns a process-owned array of `n` statfs records.
        unsafe {
            let session = DASessionCreate(kCFAllocatorDefault);
            let mut p: *mut libc::statfs = std::ptr::null_mut();
            let n = libc::getmntinfo(&mut p, libc::MNT_NOWAIT);
            for i in 0..n.max(0) as usize {
                let s = &*p.add(i);
                let from = cstr(&s.f_mntfromname);
                let on = cstr(&s.f_mntonname);
                let flags = s.f_flags as u64;
                if !from.starts_with("/dev/disk") || flags & libc::MNT_LOCAL as u64 == 0 {
                    continue;
                }
                let root = on == "/";
                if !root && (on.starts_with("/System/Volumes/") || flags & libc::MNT_RDONLY as u64 != 0 || flags & libc::MNT_DONTBROWSE as u64 != 0) {
                    continue;
                }
                let bsd = from.trim_start_matches("/dev/");
                let (internal, name) = if session.is_null() { (None, None) } else { describe(session, bsd) };
                let label = name.unwrap_or_else(|| if root { String::new() } else { Path::new(&on).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() });
                let bs = s.f_bsize as u64;
                out.push(Raw {
                    drive: Drive {
                        id: bsd.to_owned(),
                        label,
                        mount_path: on.clone(),
                        fs_type: Some(cstr(&s.f_fstypename)),
                        total_bytes: s.f_blocks as u64 * bs,
                        free_bytes: s.f_bavail as u64 * bs,
                        removable: !root && internal == Some(false) || (!root && internal.is_none() && on.starts_with("/Volumes/")),
                        system: root,
                        ..Default::default()
                    },
                    // the home lives on the Data volume, firmlinked under "/"
                    mounts: if root { vec![PathBuf::from("/")] } else { vec![PathBuf::from(&on)] },
                });
            }
            if !session.is_null() {
                CFRelease(session);
            }
        }
        out
    }

    extern "C" fn on_disk(_: CfRef, _: *mut c_void) {
        wake();
    }
    extern "C" fn on_change(_: CfRef, _: CfRef, _: *mut c_void) {
        wake();
    }

    pub(super) fn listen() {
        let _ = std::thread::Builder::new().name("nv-diskarb".into()).spawn(|| {
            // SAFETY: DA callbacks on this thread's run loop; the session lives for the process.
            unsafe {
                let session = DASessionCreate(kCFAllocatorDefault);
                if session.is_null() {
                    return;
                }
                let null = std::ptr::null();
                DARegisterDiskAppearedCallback(session, null, on_disk, std::ptr::null_mut());
                DARegisterDiskDisappearedCallback(session, null, on_disk, std::ptr::null_mut());
                DARegisterDiskDescriptionChangedCallback(session, null, null, on_change, std::ptr::null_mut());
                DASessionScheduleWithRunLoop(session, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
                CFRunLoopRun();
            }
        });
    }
    pub(super) fn shutdown() {}
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
mod os {
    use super::*;
    pub(super) fn enumerate() -> Vec<Raw> {
        vec![]
    }
    pub(super) fn listen() {}
    pub(super) fn shutdown() {}
}

/// Stops helper processes (app exit).
pub fn shutdown() {
    os::shutdown();
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &str = r"45 1 0:36 /root / rw,relatime shared:1 - btrfs /dev/mapper/luks-test rw,seclabel,compress=zstd:1,subvol=/root
40 45 0:7 / /dev rw,nosuid shared:2 - devtmpfs devtmpfs rw,seclabel
63 45 0:36 /home /home rw,relatime shared:123 - btrfs /dev/mapper/luks-test rw,seclabel,subvol=/home
67 45 259:10 / /boot rw,relatime shared:127 - ext4 /dev/nvme1n1p2 rw,seclabel
71 45 0:52 / /mnt/data rw,relatime shared:131 - btrfs /dev/nvme0n1p2 rw,seclabel
75 45 7:2 / /var/lib/snapd/snap/core20/2866 ro,nodev,relatime shared:135 - squashfs /dev/loop2 ro
90 54 8:49 / /run/media/user/MY\040STICK rw,nosuid,nodev,relatime shared:200 - vfat /dev/sdd1 rw,uid=1000
91 54 8:65 / /run/media/user/CDROM ro,nosuid shared:201 - iso9660 /dev/sr0 ro
92 54 8:81 / /run/media/user/VaultData rw,nosuid shared:202 - exfat /dev/sde2 rw";

    fn lines() -> Vec<MountLine> {
        MOUNTINFO.lines().filter_map(parse_mountinfo_line).collect()
    }

    #[test]
    fn mountinfo_lines() {
        let l = lines();
        assert_eq!(l.len(), 9);
        assert_eq!(l[0], MountLine { devno: "0:36".into(), root: "/root".into(), mount_point: "/".into(), read_only: false, fs_type: "btrfs".into(), source: "/dev/mapper/luks-test".into() });
        assert_eq!(l[6].mount_point, "/run/media/user/MY STICK");
        assert!(l[5].read_only);
        assert_eq!(unescape_mount(r"/run/media/u/MY\040STICK"), "/run/media/u/MY STICK");
        assert_eq!(unescape_mount("/plain"), "/plain");
        assert_eq!(unescape_mount(r"/trailing\04"), r"/trailing\04");
        assert_eq!(unescape_udev(r"Sata\x20ssd"), "Sata ssd");
        assert!(parse_mountinfo_line("garbage").is_none());
    }

    #[test]
    fn classification_and_grouping() {
        let l = lines();
        let raws = group_mounts(
            &l,
            |src| (src == "/dev/sdd1").then(|| "MY STICK".to_owned()),
            |src| src.starts_with("/dev/sd"),
            |_| (100, 40),
        );
        let ids: Vec<&str> = raws.iter().map(|r| r.drive.id.as_str()).collect();
        // no devtmpfs, no /boot, no snap loop, no read-only CD
        assert_eq!(ids, ["/dev/mapper/luks-test", "/dev/nvme0n1p2", "/dev/sdd1", "/dev/sde2"]);
        let root = &raws[0];
        assert_eq!(root.drive.mount_path, "/", "the btrfs device is shown by its shortest mount");
        assert_eq!(root.mounts.len(), 2);
        assert_eq!(root.drive.label, "");
        assert_eq!(raws[1].drive.label, "data", "no by-label entry: the folder name");
        assert_eq!(raws[2].drive.label, "MY STICK");
        assert!(raws[2].drive.removable && !raws[0].drive.removable);
        assert_eq!((raws[2].drive.total_bytes, raws[2].drive.free_bytes), (100, 40));

        let exe = Path::new("/run/media/user/MY STICK/NiceVault");
        let drives = finish(raws, Some(exe), Some(Path::new("/home/user")), Some(Path::new("/nonexistent/Documents")));
        let by = |id: &str| drives.iter().find(|d| d.id == id).unwrap();
        assert_eq!(drives[0].id, "/dev/sdd1", "the app's drive comes first");
        assert!(by("/dev/sdd1").app_here);
        assert!(by("/dev/mapper/luks-test").system, "the home is on a subvolume of that device");
        assert_eq!(by("/dev/mapper/luks-test").folder, "/home/user", "documents missing: the home");
        assert_eq!(by("/dev/nvme0n1p2").folder, "/mnt/data");
        assert!(by("/dev/sde2").is_nicevault_device);
        assert!(!by("/dev/sdd1").is_nicevault_device);
        assert_eq!(drives[1].id, "/dev/sde2", "then removable");
    }

    #[test]
    fn udisks_signals() {
        assert!(udisks_line_changes_drives(
            "/org/freedesktop/UDisks2: org.freedesktop.DBus.ObjectManager.InterfacesAdded (objectpath '/org/freedesktop/UDisks2/block_devices/sdd1', {...})"
        ));
        assert!(udisks_line_changes_drives(
            "/org/freedesktop/UDisks2: org.freedesktop.DBus.ObjectManager.InterfacesRemoved (objectpath '/org/freedesktop/UDisks2/drives/X', ['org.freedesktop.UDisks2.Drive'])"
        ));
        assert!(udisks_line_changes_drives(
            "/org/freedesktop/UDisks2/block_devices/sdd1: org.freedesktop.DBus.Properties.PropertiesChanged ('org.freedesktop.UDisks2.Filesystem', {'MountPoints': <[b'/run/media/user/STICK']>}, @as [])"
        ));
        assert!(!udisks_line_changes_drives(
            "/org/freedesktop/UDisks2/drives/X: org.freedesktop.DBus.Properties.PropertiesChanged ('org.freedesktop.UDisks2.Drive.Ata', {'SmartUpdated': <uint64 1>}, @as [])"
        ));
    }

    #[test]
    fn changes_ignore_free_space() {
        let d = Drive { id: "a".into(), mount_path: "/m".into(), free_bytes: 1, ..Default::default() };
        let mut e = d.clone();
        e.free_bytes = 99;
        assert!(same_set(std::slice::from_ref(&d), &[e]));
        let mut f = d.clone();
        f.label = "X".into();
        assert!(!same_set(std::slice::from_ref(&d), &[f]));
        assert!(!same_set(&[d], &[]));
    }

    #[test]
    fn fake_file_source() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("drives.json");
        assert!(fake_list(&f).is_empty());
        std::fs::write(&f, r#"[{"id":"fake1","label":"STICK","mount_path":"/tmp/stick","removable":true,"total_bytes":10,"free_bytes":5}]"#).unwrap();
        let l = fake_list(&f);
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].folder.as_str(), l[0].removable, l[0].fs_type.as_deref()), ("/tmp/stick", true, None));
    }
}
