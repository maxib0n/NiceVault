//! "Crea dispositivo NiceVault": turns an
//! external drive into a NiceVault device, a `NiceVault` exFAT partition (512 MiB at 1 MiB) with
//! the three builds and a README, and a `VaultData` partition (the rest) for volumes.
//!
//! Layers, from pure to effectful:
//!  - this file: the model (whole disks, partitions), the refusal rules, [`plan`], the typed
//!    confirmation, the resume decision table: all pure and unit-tested;
//!  - [`commands`]: the exact privileged commands per OS (udisks2 D-Bus calls run with `gdbus`,
//!    the PowerShell Storage script of the Windows helper, `diskutil` argv and its `osascript`
//!    fallback), generated from a plan, pure;
//!  - [`enumerate`]: read-only whole-disk enumeration (Linux sysfs + udev db + mountinfo, never
//!    D-Bus; Windows `Get-Disk` via PowerShell; macOS `diskutil -plist`), parsers pure;
//!  - [`backend`]: the [`backend::DiskOps`] interface with the OS implementation, a fake one
//!    (`NICEVAULT_FAKE_DISKS`, e2e and mock parity) and a dry run (`NICEVAULT_DEVICE_DRY_RUN=1`:
//!    logs every command, executes none, "mounts" temp folders);
//!  - [`client`]: what goes on the NiceVault partition (signed downloads through the updater,
//!    local copies as offline fallback, README, marker), written and verified unprivileged;
//!  - [`stick`]: "Aggiorna i programmi sulla chiavetta" (status of a device, update of its programs);
//!  - [`job`]: the resumable job with `job-progress {job, step, ...}` and the log;
//!  - [`helper`]: the Windows elevated helper (`--device-helper <plan.json>`, main.rs);
//!  - [`cmd`]: the Tauri commands.
//!
//! Only partitioning and formatting is privileged. The password of a first volume never goes
//! through any of this (the UI creates it afterwards with `vault_create`).

pub mod backend;
pub mod client;
pub mod cmd;
pub mod commands;
pub mod enumerate;
pub mod helper;
pub mod job;
pub mod stick;

use crate::error::ApiError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MIB: u64 = 1 << 20;
pub const GIB: u64 = 1 << 30;
/// The client partition: 512 MiB (room for the three builds, the macOS .app and its signed zip, plus an update's temporary copies; devices made earlier have 128 or 256 MiB and keep working) starting at 1 MiB.
pub const CLIENT_OFFSET: u64 = MIB;
pub const CLIENT_SIZE: u64 = 512 * MIB;
/// The data partition starts right after it (1 MiB aligned) and takes the rest.
pub const DATA_OFFSET: u64 = CLIENT_OFFSET + CLIENT_SIZE;
pub const MIN_DISK: u64 = GIB;
/// MBR cannot address more than 2 TiB (512-byte sectors).
pub const MBR_MAX: u64 = 2 << 40;
pub const CLIENT_LABEL: &str = "NiceVault";
pub const DATA_LABEL: &str = "VaultData";
pub const MARKER: &str = ".nicevault-device.json";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Linux,
    Windows,
    Macos,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        }
    }
    pub const ALL: [Os; 3] = [Os::Linux, Os::Windows, Os::Macos];
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Table {
    Mbr,
    Gpt,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DataFs {
    Exfat,
    Ntfs,
    Ext4,
    Btrfs,
    Apfs,
}

/// How another OS can use a file system: read-write, read-only, not at all.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Rw,
    Ro,
    No,
}

impl DataFs {
    pub const ALL: [DataFs; 5] = [DataFs::Exfat, DataFs::Ntfs, DataFs::Ext4, DataFs::Btrfs, DataFs::Apfs];

    /// The readability table of the spec (section 3): (Windows, macOS, Linux).
    pub fn readable_on(self) -> [(Os, Access); 3] {
        use Access::*;
        let (w, m, l) = match self {
            DataFs::Exfat => (Rw, Rw, Rw),
            DataFs::Ntfs => (Rw, Ro, Rw),
            DataFs::Ext4 | DataFs::Btrfs => (No, No, Rw),
            DataFs::Apfs => (No, Rw, Ro),
        };
        [(Os::Windows, w), (Os::Macos, m), (Os::Linux, l)]
    }

    /// Whether `os` can create it at all (Linux still needs the mkfs tool, see `missing_tool`).
    pub fn creatable_on(self, os: Os) -> bool {
        match os {
            Os::Linux => !matches!(self, DataFs::Apfs),
            Os::Windows => matches!(self, DataFs::Exfat | DataFs::Ntfs),
            Os::Macos => matches!(self, DataFs::Exfat | DataFs::Apfs),
        }
    }

    /// Linux: the mkfs utility udisks runs, and the package that has it.
    pub fn linux_tool(self) -> Option<(&'static str, &'static str)> {
        match self {
            DataFs::Exfat => Some(("mkfs.exfat", "exfatprogs")),
            DataFs::Ntfs => Some(("mkfs.ntfs", "ntfsprogs")),
            DataFs::Ext4 => Some(("mkfs.ext4", "e2fsprogs")),
            DataFs::Btrfs => Some(("mkfs.btrfs", "btrfs-progs")),
            DataFs::Apfs => None,
        }
    }

    /// udisks / mkfs name.
    pub fn id(self) -> &'static str {
        match self {
            DataFs::Exfat => "exfat",
            DataFs::Ntfs => "ntfs",
            DataFs::Ext4 => "ext4",
            DataFs::Btrfs => "btrfs",
            DataFs::Apfs => "apfs",
        }
    }

    /// A Linux-only file system: its root is made world-writable.
    pub fn unix_perms(self) -> bool {
        matches!(self, DataFs::Ext4 | DataFs::Btrfs)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NvState {
    #[default]
    None,
    /// A `NiceVault` partition without the marker (an interrupted run): "Completa".
    Incomplete,
    Complete,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Partition {
    /// OS path (`/dev/sdd1`, `\\?\Volume{..}` / partition number on Windows, `disk4s1`).
    pub path: String,
    pub label: String,
    pub fs: String,
    pub size: u64,
    /// Every place it is mounted (Windows: drive letter roots); empty = not mounted.
    pub mountpoints: Vec<String>,
}

/// One whole disk, as enumerated (read-only).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Disk {
    /// Linux `/dev/sdd`, Windows `PhysicalDrive3` (number in `number`), macOS `disk4`.
    pub id: String,
    pub os_path: String,
    /// Windows disk number.
    pub number: Option<u32>,
    pub vendor: String,
    pub model: String,
    pub serial: String,
    /// usb | sd | mmc | ieee1394 | sata | nvme | scsi | virtual | other
    pub bus: String,
    pub size: u64,
    pub removable: bool,
    /// The OS says it is a system / boot disk.
    pub is_system: bool,
    /// Loop device, VHD(X), disk image.
    pub is_virtual: bool,
    pub read_only: bool,
    pub partitions: Vec<Partition>,
    pub nicevault: NvState,
}

impl Disk {
    /// What the user types to confirm and what the cards show: `Vendor Model` (spaces collapsed),
    /// empty when the drive reports neither.
    pub fn name(&self) -> String {
        let mut s = String::new();
        for w in self.vendor.split_whitespace().chain(self.model.split_whitespace()) {
            // "SanDisk SanDisk Ultra": the vendor repeated at the start of the model
            if s.is_empty() || !s.split(' ').any(|x| x.eq_ignore_ascii_case(w)) {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(w);
            }
        }
        s
    }

    pub fn serial_tail(&self) -> String {
        let t = self.serial.trim();
        let n = t.chars().count();
        t.chars().skip(n.saturating_sub(7)).collect()
    }

    pub fn mountpoints(&self) -> impl Iterator<Item = &str> {
        self.partitions.iter().flat_map(|p| p.mountpoints.iter().map(String::as_str))
    }

    pub fn by_label(&self, label: &str) -> Option<&Partition> {
        self.partitions.iter().find(|p| p.label.trim().eq_ignore_ascii_case(label))
    }

    /// Same physical disk as the one a plan was made for (spec 5: serial, size, id).
    pub fn same_as(&self, other: &Disk) -> bool {
        self.id == other.id && self.size == other.size && self.serial.trim() == other.serial.trim()
    }
}

/// External / removable as the spec's rule 2 defines it.
pub fn is_external(d: &Disk) -> bool {
    d.removable || matches!(d.bus.as_str(), "usb" | "sd" | "mmc" | "sdio" | "ieee1394")
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    System,
    Internal,
    Virtual,
    TooSmall,
    HostsApp,
    OpenVolume,
    ReadOnly,
}

impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Refusal::System => "system",
            Refusal::Internal => "internal",
            Refusal::Virtual => "virtual",
            Refusal::TooSmall => "too_small",
            Refusal::HostsApp => "hosts_app",
            Refusal::OpenVolume => "open_volume",
            Refusal::ReadOnly => "read_only",
        }
    }
    pub fn error(self) -> ApiError {
        ApiError::new("device_refused", self.code())
    }
}

/// What this machine runs from: a disk holding any of it is never offered.
#[derive(Clone, Debug, Default)]
pub struct HostCtx {
    /// `/`, `/home`, `$HOME`, the exe's folder, the config dir (Windows `%SystemDrive%\`, ...).
    pub protected: Vec<PathBuf>,
    /// Active swap devices (Linux `/proc/swaps`).
    pub swap: Vec<String>,
    /// Files of the volumes open in this app.
    pub open_volumes: Vec<PathBuf>,
    /// Test override (`NICEVAULT_DEVICE_TEST=1`): loop devices, VHDs and RAM disks are allowed.
    pub allow_virtual: bool,
}

impl HostCtx {
    /// This machine and this process.
    pub fn detect(open_volumes: Vec<PathBuf>) -> HostCtx {
        let mut protected: Vec<PathBuf> = vec![];
        if cfg!(unix) {
            protected.extend(["/", "/home", "/usr", "/boot"].map(PathBuf::from));
        }
        if let Ok(sd) = std::env::var("SystemDrive") {
            protected.push(PathBuf::from(format!("{sd}\\")));
        }
        protected.extend(dirs::home_dir());
        protected.extend(dirs::config_dir());
        if let Ok(exe) = std::env::current_exe() {
            protected.push(exe);
        }
        let swap = std::fs::read_to_string("/proc/swaps")
            .map(|s| s.lines().skip(1).filter_map(|l| l.split_whitespace().next().map(str::to_owned)).collect())
            .unwrap_or_default();
        HostCtx { protected, swap, open_volumes, allow_virtual: std::env::var("NICEVAULT_DEVICE_TEST").is_ok_and(|v| v == "1") }
    }
}

fn path_on(p: &Path, mount: &str) -> bool {
    if mount.is_empty() {
        return false;
    }
    let (p, m) = (p.to_string_lossy().to_lowercase(), mount.to_lowercase());
    if cfg!(windows) || m.contains('\\') {
        let m = m.trim_end_matches('\\');
        return p == m || p.starts_with(&format!("{m}\\"));
    }
    Path::new(&p).starts_with(Path::new(&m))
}

/// Why `d` cannot become a NiceVault device, `None` when it can (the spec's section 2 rules).
pub fn refusal(d: &Disk, ctx: &HostCtx) -> Option<Refusal> {
    if d.is_system {
        return Some(Refusal::System);
    }
    if d.is_virtual {
        if !ctx.allow_virtual {
            return Some(Refusal::Virtual);
        }
    } else if !is_external(d) {
        return Some(Refusal::Internal);
    }
    if d.read_only {
        return Some(Refusal::ReadOnly);
    }
    if d.size < MIN_DISK {
        return Some(Refusal::TooSmall);
    }
    let mounts: Vec<&str> = d.mountpoints().collect();
    if ctx.protected.iter().any(|p| mounts.iter().any(|m| path_on(p, m)))
        || d.partitions.iter().any(|p| ctx.swap.iter().any(|s| s == &p.path))
    {
        return Some(Refusal::HostsApp);
    }
    if ctx.open_volumes.iter().any(|p| mounts.iter().any(|m| path_on(p, m))) {
        return Some(Refusal::OpenVolume);
    }
    None
}

/// The disks that may be offered, sorted by name.
pub fn candidates(disks: Vec<Disk>, ctx: &HostCtx) -> Vec<Disk> {
    let mut out: Vec<Disk> = disks.into_iter().filter(|d| refusal(d, ctx).is_none()).collect();
    out.sort_by_key(|d| (d.name().to_lowercase(), d.id.clone()));
    out
}

// ---------------------------------------------------------------------------------------------
// the plan

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VolumeMode {
    /// "Tutta la partizione": a preallocated volume filling VaultData.
    Whole,
    /// "Un pezzo": a volume of a chosen size (preallocated or growing).
    Piece,
}

/// The optional first volume (created by the UI afterwards; no password here).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VolumeOpts {
    pub mode: VolumeMode,
    /// "Un pezzo": its size (fixed) or None (grows as needed).
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanOpts {
    pub id: String,
    #[serde(default = "default_table")]
    pub table: Table,
    #[serde(default = "default_fs")]
    pub data_fs: DataFs,
    #[serde(default)]
    pub first_volume: Option<VolumeOpts>,
    /// Download the latest signed builds (local copies only as fallback).
    #[serde(default = "yes")]
    pub download: bool,
}
fn default_table() -> Table {
    Table::Mbr
}
fn default_fs() -> DataFs {
    DataFs::Exfat
}
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartSpec {
    pub label: String,
    pub fs: String,
    pub offset: u64,
    /// 0 = the rest of the disk.
    pub size: u64,
}

pub const STEPS: [&str; 11] =
    ["check", "download", "unmount", "partition", "format_client", "format_data", "mount", "copy", "verify", "marker", "volume"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Plan {
    pub id: String,
    pub os: Os,
    pub disk: Disk,
    pub table: Table,
    /// GPT was required (over 2 TiB, or APFS) although MBR was asked for.
    pub table_forced: bool,
    pub data_fs: DataFs,
    pub client: PartSpec,
    pub data: PartSpec,
    /// Approximate size of VaultData (what the new layout card shows).
    pub data_size: u64,
    pub first_volume: Option<VolumeOpts>,
    pub download: bool,
    pub steps: Vec<String>,
    /// Every privileged command, as it will run (the "Mostra comandi" expander).
    pub commands_preview: Vec<String>,
    /// What the user must type (`Vendor Model`; empty = `CANCELLA` / `ERASE`).
    pub confirm_name: String,
    /// NiceVault volumes found on the drive's mounted partitions, which the erase destroys
    /// (additive: filled by `device_plan`, read-only scan, see [`volumes_on`]).
    #[serde(default)]
    pub volumes: Vec<FoundVolume>,
}

/// A NiceVault volume that is on a drive about to be erased.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FoundVolume {
    /// File name (`SH2RE.nvault`).
    pub name: String,
    pub path: String,
    /// Open in this app right now.
    pub open: bool,
}

/// Entries looked at per mounted partition at most (a full stick must not stall the plan).
pub const SCAN_CAP: usize = 4000;

fn is_volume_name(n: &str) -> bool {
    n.len() > ".nvault".len() && n.to_ascii_lowercase().ends_with(".nvault")
}

/// The NiceVault volumes on `d`'s mounted partitions: `*.nvault` at the top level of each
/// mountpoint or one folder down (read-only `read_dir`, at most [`SCAN_CAP`] entries per
/// partition), plus any volume open in this app whose file is on the drive. Sorted by name.
pub fn volumes_on(d: &Disk, open: &[PathBuf]) -> Vec<FoundVolume> {
    let mut found: Vec<FoundVolume> = vec![];
    let add = |p: &Path, found: &mut Vec<FoundVolume>| {
        let path = p.to_string_lossy().into_owned();
        if found.iter().any(|f| f.path == path) {
            return;
        }
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone());
        let open = open.iter().any(|o| o == p);
        found.push(FoundVolume { name, path, open });
    };
    for m in d.mountpoints() {
        let mut seen = 0usize;
        let Ok(top) = std::fs::read_dir(m) else { continue };
        let mut subdirs = vec![];
        for e in top.flatten() {
            seen += 1;
            if seen > SCAN_CAP {
                break;
            }
            let Ok(t) = e.file_type() else { continue };
            let n = e.file_name().to_string_lossy().into_owned();
            if t.is_dir() {
                subdirs.push(e.path());
            } else if t.is_file() && is_volume_name(&n) {
                add(&e.path(), &mut found);
            }
        }
        'sub: for dir in subdirs {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                seen += 1;
                if seen > SCAN_CAP {
                    break 'sub;
                }
                if e.file_type().is_ok_and(|t| t.is_file()) && is_volume_name(&e.file_name().to_string_lossy()) {
                    add(&e.path(), &mut found);
                }
            }
        }
    }
    let mounts: Vec<&str> = d.mountpoints().collect();
    for o in open.iter().filter(|o| mounts.iter().any(|m| path_on(o, m))) {
        add(o, &mut found);
    }
    found.sort_by_key(|f| (f.name.to_lowercase(), f.path.clone()));
    found
}

/// `plan(disk, opts) -> Plan`: the layout, the steps and the commands, or why not.
pub fn plan(disk: &Disk, opts: &PlanOpts, os: Os, ctx: &HostCtx, plan_id: &str) -> Result<Plan, ApiError> {
    if let Some(r) = refusal(disk, ctx) {
        return Err(r.error());
    }
    if os == Os::Windows && disk.number.is_none() {
        // the script addresses the disk by number: never guess one
        return Err(ApiError::new("device_not_found", "No Windows disk number for this drive."));
    }
    if !opts.data_fs.creatable_on(os) {
        return Err(ApiError::new("fs_unsupported", format!("{}:", opts.data_fs.id())));
    }
    let need_gpt = disk.size > MBR_MAX || opts.data_fs == DataFs::Apfs;
    let table = if need_gpt { Table::Gpt } else { opts.table };
    if let Some(v) = &opts.first_volume
        && v.mode == VolumeMode::Piece
        && v.size.is_some_and(|s| s < nv_core::Fixed::MIN || s > disk.size.saturating_sub(DATA_OFFSET + 16 * MIB))
    {
        return Err(ApiError::new("invalid_size", "The volume does not fit VaultData."));
    }
    let mut steps: Vec<String> = STEPS.iter().map(|s| s.to_string()).collect();
    if !opts.download {
        steps.retain(|s| s != "download");
    }
    if opts.first_volume.is_none() {
        steps.retain(|s| s != "volume");
    }
    let mut p = Plan {
        id: plan_id.to_owned(),
        os,
        disk: disk.clone(),
        table,
        table_forced: table != opts.table,
        data_fs: opts.data_fs,
        client: PartSpec { label: CLIENT_LABEL.into(), fs: "exfat".into(), offset: CLIENT_OFFSET, size: CLIENT_SIZE },
        data: PartSpec { label: DATA_LABEL.into(), fs: opts.data_fs.id().into(), offset: DATA_OFFSET, size: 0 },
        data_size: disk.size.saturating_sub(DATA_OFFSET + MIB),
        first_volume: opts.first_volume.clone(),
        download: opts.download,
        steps,
        commands_preview: vec![],
        confirm_name: disk.name(),
        volumes: vec![],
    };
    p.commands_preview = commands::preview(&p);
    Ok(p)
}

/// The typed confirmation: the drive name, case-insensitive and trimmed (inner spaces
/// collapsed); for a drive without a name `CANCELLA` or `ERASE`.
pub fn confirmation_ok(plan: &Plan, typed: &str) -> bool {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let t = norm(typed);
    if plan.confirm_name.trim().is_empty() {
        return t == "cancella" || t == "erase";
    }
    !t.is_empty() && t == norm(&plan.confirm_name)
}

/// What to do with a disk seen again (spec 5, "Unplugged midway").
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Resume {
    /// The layout is not ours: start over, with both confirmations.
    Redo,
    /// Layout OK, no marker: mount, copy again, marker; no reformat.
    Complete,
    /// Marker present: nothing to redo (the first volume may still be offered).
    Done,
}

/// `marker_present`: `None` when the client partition could not be read (not mounted).
pub fn resume_action(d: &Disk, marker_present: Option<bool>) -> Resume {
    let client_ok = d.by_label(CLIENT_LABEL).is_some_and(|p| p.fs.eq_ignore_ascii_case("exfat"));
    if !client_ok || d.by_label(DATA_LABEL).is_none() {
        return Resume::Redo;
    }
    if marker_present == Some(true) { Resume::Done } else { Resume::Complete }
}

/// The marker file (written last, spec 1).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Marker {
    pub format: u32,
    pub created: u64,
    pub app_version: String,
    pub data_fs: String,
    pub device_id: String,
    /// Extra (additive): table and the versions of the builds copied (`null` = missing).
    #[serde(default)]
    pub table: Option<Table>,
    #[serde(default)]
    pub clients: std::collections::BTreeMap<String, Option<String>>,
    /// Extra (additive): when "Aggiorna i programmi sulla chiavetta" last rewrote them (unix ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<u64>,
    /// Extra (additive): `NiceVault.app` as it was verified when written (the zip is not kept).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac_app: Option<MacRecord>,
}

/// `NiceVault.app` on the NiceVault partition, checked against its signed zip when written: the
/// zip's SHA-256 (None: copied from the running app) and a digest of the bundle's files
/// (`client::tree_digest`), so the app can later be checked without the zip.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MacRecord {
    pub zip_sha256: Option<String>,
    pub tree_sha256: String,
}

/// `nicevault` state from what is on the client partition.
pub fn state_of(d: &Disk) -> NvState {
    let Some(p) = d.by_label(CLIENT_LABEL) else { return NvState::None };
    if p.mountpoints.iter().any(|m| Path::new(m).join(MARKER).is_file()) { NvState::Complete } else { NvState::Incomplete }
}

/// Room for a "Tutta la partizione" volume: VaultData's free space less a small reserve
/// (file system bookkeeping, and room for a journal tail past the capacity in a nearly full
/// volume — the volume is single-file, its journal is inside it), in whole MiB.
pub fn whole_volume_size(free: u64) -> u64 {
    let reserve = (free / 200).clamp(16 * MIB, 512 * MIB);
    let s = free.saturating_sub(reserve);
    s - s % MIB
}

#[cfg(test)]
mod tests;
