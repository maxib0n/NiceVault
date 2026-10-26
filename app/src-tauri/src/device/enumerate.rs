//! Whole-disk enumeration, read-only and without any prompt on every OS:
//!  - Linux: `/sys/block` (size, removable, ro, vendor/model, partitions), the udev database
//!    (`/run/udev/data/b<maj>:<min>`: bus, serial, labels, file systems) and
//!    `/proc/self/mountinfo` (mount points, also through device-mapper holders). No D-Bus.
//!  - Windows: one PowerShell script printing `Get-Disk`, `Get-Partition` and `Get-Volume` as
//!    JSON (no elevation needed to read them).
//!  - macOS: `diskutil list -plist` and `diskutil info -plist`, turned into JSON by `plutil`.
//!
//! The parsers take text / a directory tree, so the tests feed them fixtures.

use super::{Disk, Partition};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------------------------
// Linux

/// `E:KEY=value` lines of a udev database entry.
pub fn parse_udev(text: &str) -> HashMap<String, String> {
    text.lines().filter_map(|l| l.strip_prefix("E:")).filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_owned(), v.to_owned())).collect()
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).map(|s| s.trim().to_owned()).unwrap_or_default()
}

/// Mount points per `major:minor` from mountinfo text.
pub fn mounts_by_devno(mountinfo: &str) -> HashMap<String, Vec<String>> {
    let mut m: HashMap<String, Vec<String>> = HashMap::new();
    for l in mountinfo.lines().filter_map(crate::drives::parse_mountinfo_line) {
        let e = m.entry(l.devno).or_default();
        if !e.contains(&l.mount_point) {
            e.push(l.mount_point);
        }
    }
    for v in m.values_mut() {
        v.sort_by_key(|s| s.len());
    }
    m
}

/// Mount points of a block device and of everything stacked on it (LUKS, LVM: `holders/`).
fn mounts_of(sys_dev: &Path, mounts: &HashMap<String, Vec<String>>, depth: u8) -> Vec<String> {
    let mut out = mounts.get(&read(&sys_dev.join("dev"))).cloned().unwrap_or_default();
    if depth < 4
        && let Ok(rd) = std::fs::read_dir(sys_dev.join("holders"))
    {
        for h in rd.flatten() {
            if let Ok(p) = std::fs::canonicalize(h.path()) {
                out.extend(mounts_of(&p, mounts, depth + 1));
            }
        }
    }
    out
}

const SYSTEM_MOUNTS: [&str; 6] = ["/", "/boot", "/boot/efi", "/efi", "/usr", "/home"];

/// Every whole disk under `sys_block` (normally `/sys/block`), `udev_data` the udev database
/// folder, `mountinfo` the text of /proc/self/mountinfo.
pub fn linux_disks(sys_block: &Path, udev_data: &Path, mountinfo: &str) -> Vec<Disk> {
    let mounts = mounts_by_devno(mountinfo);
    let Ok(rd) = std::fs::read_dir(sys_block) else { return vec![] };
    let mut out = vec![];
    let mut names: Vec<_> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    for name in names {
        if ["ram", "zram", "dm-", "md", "sr", "fd", "nbd"].iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let dir = std::fs::canonicalize(sys_block.join(&name)).unwrap_or_else(|_| sys_block.join(&name));
        let size = read(&dir.join("size")).parse::<u64>().unwrap_or(0) * 512;
        if size == 0 {
            continue; // empty card reader, unattached loop
        }
        let udev = parse_udev(&std::fs::read_to_string(udev_data.join(format!("b{}", read(&dir.join("dev"))))).unwrap_or_default());
        let get = |k: &str| udev.get(k).cloned().unwrap_or_default();
        let is_loop = name.starts_with("loop");
        let canon = dir.to_string_lossy().into_owned();
        let bus = match get("ID_BUS").as_str() {
            "" if is_loop => "virtual".to_owned(),
            // a USB-SATA bridge (UAS) reports ID_BUS=ata: the sysfs path says where the disk hangs
            _ if canon.contains("/usb") => "usb".to_owned(),
            "" if name.starts_with("mmcblk") => if get("ID_DRIVE_FLASH_SD") == "1" { "sd".into() } else { "mmc".into() },
            "" if name.starts_with("nvme") => "nvme".to_owned(),
            "" => "other".to_owned(),
            "ata" => "sata".to_owned(),
            b => b.to_owned(),
        };
        let mut partitions = vec![];
        let mut parts: Vec<_> = std::fs::read_dir(&dir).map(|r| r.flatten().map(|e| e.path()).filter(|p| p.join("partition").exists()).collect()).unwrap_or_default();
        parts.sort();
        for p in parts {
            let pname = p.file_name().unwrap().to_string_lossy().into_owned();
            let pu = parse_udev(&std::fs::read_to_string(udev_data.join(format!("b{}", read(&p.join("dev"))))).unwrap_or_default());
            let label = pu.get("ID_FS_LABEL_ENC").map(|l| crate::drives::unescape_udev(l)).or_else(|| pu.get("ID_FS_LABEL").cloned()).unwrap_or_default();
            partitions.push(Partition {
                path: format!("/dev/{pname}"),
                label,
                fs: pu.get("ID_FS_TYPE").cloned().unwrap_or_default(),
                size: read(&p.join("size")).parse::<u64>().unwrap_or(0) * 512,
                mountpoints: mounts_of(&p, &mounts, 0),
            });
        }
        let whole_mounts = mounts_of(&dir, &mounts, 0);
        let all_mounts: Vec<&String> = partitions.iter().flat_map(|p| &p.mountpoints).chain(&whole_mounts).collect();
        let mut d = Disk {
            id: format!("/dev/{name}"),
            os_path: format!("/dev/{name}"),
            number: None,
            vendor: if get("ID_VENDOR").is_empty() { read(&dir.join("device/vendor")) } else { get("ID_VENDOR").replace('_', " ") },
            model: if get("ID_MODEL").is_empty() { read(&dir.join("device/model")) } else { get("ID_MODEL").replace('_', " ") },
            serial: get("ID_SERIAL_SHORT"),
            bus,
            size,
            removable: read(&dir.join("removable")) == "1",
            is_system: all_mounts.iter().any(|m| SYSTEM_MOUNTS.contains(&m.as_str())),
            is_virtual: is_loop,
            read_only: read(&dir.join("ro")) == "1",
            partitions,
            nicevault: Default::default(),
        };
        d.nicevault = super::state_of(&d);
        out.push(d);
    }
    out
}

#[cfg(target_os = "linux")]
pub fn os_disks() -> Vec<Disk> {
    let mi = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    linux_disks(Path::new("/sys/block"), Path::new("/run/udev/data"), &mi)
}

// ---------------------------------------------------------------------------------------------
// Windows

/// Read-only: prints `{disks, partitions, volumes}` as JSON.
pub const WINDOWS_LIST_SCRIPT: &str = "$ProgressPreference='SilentlyContinue'; \
$d=@(Get-Disk | Select-Object Number,FriendlyName,Manufacturer,Model,SerialNumber,@{n='BusType';e={[int]$_.BusType}},Size,IsBoot,IsSystem,BootFromDisk,IsReadOnly,Path); \
$p=@(Get-Partition -ErrorAction SilentlyContinue | Select-Object DiskNumber,PartitionNumber,DriveLetter,Size,AccessPaths,@{n='VolumePath';e={($_ | Get-Volume -ErrorAction SilentlyContinue).Path}}); \
$v=@(Get-Volume -ErrorAction SilentlyContinue | Select-Object Path,FileSystemLabel,FileSystem); \
@{disks=$d;partitions=$p;volumes=$v} | ConvertTo-Json -Depth 4 -Compress";

fn s(v: &serde_json::Value, k: &str) -> String {
    match &v[k] {
        serde_json::Value::String(s) => s.trim().to_owned(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}
fn arr(v: &serde_json::Value) -> Vec<serde_json::Value> {
    match v {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Null => vec![],
        o => vec![o.clone()], // ConvertTo-Json unwraps one-element arrays
    }
}

/// BusType numbers of MSFT_Disk.
pub fn windows_bus(n: u64) -> &'static str {
    match n {
        7 => "usb",
        12 => "sd",
        13 => "mmc",
        4 => "ieee1394",
        11 => "sata",
        17 => "nvme",
        10 | 1 => "scsi",
        14 | 15 => "virtual",
        _ => "other",
    }
}

pub fn parse_windows(json: &str, system_drive: &str) -> Vec<Disk> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    let vols = arr(&root["volumes"]);
    let parts = arr(&root["partitions"]);
    let sys = system_drive.trim_end_matches('\\').to_ascii_uppercase();
    let mut out = vec![];
    for d in arr(&root["disks"]) {
        let Some(n) = d["Number"].as_u64() else { continue };
        let bus = d["BusType"].as_u64().unwrap_or(0);
        let mut partitions = vec![];
        for p in parts.iter().filter(|p| p["DiskNumber"].as_u64() == Some(n)) {
            let vp = s(p, "VolumePath");
            let vol = vols.iter().find(|v| !vp.is_empty() && s(v, "Path") == vp);
            let mut mps: Vec<String> = arr(&p["AccessPaths"]).iter().filter_map(|a| a.as_str()).filter(|a| a.len() == 3 && a.ends_with(":\\")).map(str::to_owned).collect();
            let letter = s(p, "DriveLetter");
            if mps.is_empty() && letter.len() == 1 && letter != "\u{0}" {
                mps.push(format!("{letter}:\\"));
            }
            partitions.push(Partition {
                path: format!("{n}:{}", s(p, "PartitionNumber")),
                label: vol.map(|v| s(v, "FileSystemLabel")).unwrap_or_default(),
                fs: vol.map(|v| s(v, "FileSystem").to_lowercase()).unwrap_or_default(),
                size: p["Size"].as_u64().unwrap_or(0),
                mountpoints: mps,
            });
        }
        let holds_system = partitions.iter().flat_map(|p| &p.mountpoints).any(|m| m.trim_end_matches('\\').eq_ignore_ascii_case(&sys));
        let model = { let m = s(&d, "Model"); if m.is_empty() { s(&d, "FriendlyName") } else { m } };
        let mut disk = Disk {
            id: format!("PhysicalDrive{n}"),
            os_path: format!(r"\\.\PhysicalDrive{n}"),
            number: Some(n as u32),
            vendor: s(&d, "Manufacturer"),
            model,
            serial: s(&d, "SerialNumber"),
            bus: windows_bus(bus).to_owned(),
            size: d["Size"].as_u64().unwrap_or(0),
            removable: matches!(bus, 7 | 12 | 13),
            is_system: d["IsBoot"].as_bool().unwrap_or(true) || d["IsSystem"].as_bool().unwrap_or(true) || d["BootFromDisk"].as_bool().unwrap_or(false) || holds_system,
            is_virtual: matches!(bus, 14 | 15),
            read_only: d["IsReadOnly"].as_bool().unwrap_or(false),
            partitions,
            nicevault: Default::default(),
        };
        disk.nicevault = super::state_of(&disk);
        out.push(disk);
    }
    out
}

#[cfg(windows)]
pub fn powershell(script: &str) -> std::io::Result<std::process::Output> {
    use std::os::windows::process::CommandExt;
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    std::process::Command::new(format!(r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe"))
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &super::commands::encode_ps(script)])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdin(std::process::Stdio::null())
        .output()
}

#[cfg(windows)]
pub fn os_disks() -> Vec<Disk> {
    let Ok(o) = powershell(WINDOWS_LIST_SCRIPT) else { return vec![] };
    parse_windows(&String::from_utf8_lossy(&o.stdout), &std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()))
}

// ---------------------------------------------------------------------------------------------
// macOS

/// `list`: `diskutil list -plist external physical` as JSON; `info(id)`: `diskutil info -plist`
/// of that whole disk as JSON; `root_store`: the whole disk(s) holding `/`.
pub fn parse_macos(list: &str, info: impl Fn(&str) -> Option<serde_json::Value>, root_disks: &[String]) -> Vec<Disk> {
    let Ok(l) = serde_json::from_str::<serde_json::Value>(list) else { return vec![] };
    let mut out = vec![];
    for w in arr(&l["AllDisksAndPartitions"]) {
        let id = s(&w, "DeviceIdentifier");
        if id.is_empty() {
            continue;
        }
        let i = info(&id).unwrap_or_default();
        let b = |k: &str| i[k].as_bool().unwrap_or(false);
        let mut partitions = vec![];
        for p in arr(&w["Partitions"]) {
            let mp = s(&p, "MountPoint");
            partitions.push(Partition {
                path: s(&p, "DeviceIdentifier"),
                label: s(&p, "VolumeName"),
                fs: s(&p, "Content").to_lowercase(),
                size: p["Size"].as_u64().unwrap_or(0),
                mountpoints: if mp.is_empty() { vec![] } else { vec![mp] },
            });
        }
        let protocol = s(&i, "BusProtocol").to_lowercase();
        let bus = match protocol.as_str() {
            "usb" => "usb",
            "secure digital" | "sd" => "sd",
            "firewire" => "ieee1394",
            "pci-express" | "pci" => "nvme",
            "sata" => "sata",
            "disk image" | "virtual" => "virtual",
            _ => "other",
        };
        let internal = i["Internal"].as_bool().unwrap_or(true);
        let mut d = Disk {
            id: id.clone(),
            os_path: format!("/dev/{id}"),
            number: None,
            vendor: String::new(),
            model: { let m = s(&i, "MediaName"); if m.is_empty() { s(&i, "IORegistryEntryName") } else { m } },
            serial: s(&i, "DiskUUID"),
            bus: bus.into(),
            size: i["Size"].as_u64().or(i["TotalSize"].as_u64()).or(w["Size"].as_u64()).unwrap_or(0),
            removable: !internal && (b("RemovableMedia") || b("Removable") || b("Ejectable") || bus == "usb"),
            is_system: root_disks.contains(&id) || !b("WholeDisk") && i.get("WholeDisk").is_some(),
            is_virtual: s(&i, "VirtualOrPhysical").eq_ignore_ascii_case("virtual") || bus == "virtual",
            read_only: !i["WritableMedia"].as_bool().unwrap_or(true),
            partitions,
            nicevault: Default::default(),
        };
        d.nicevault = super::state_of(&d);
        out.push(d);
    }
    out
}

#[cfg(target_os = "macos")]
fn plist_json(args: &[&str]) -> Option<serde_json::Value> {
    use std::io::Write;
    let o = std::process::Command::new(super::commands::DISKUTIL).args(args).output().ok()?;
    let mut c = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    c.stdin.take()?.write_all(&o.stdout).ok()?;
    let out = c.wait_with_output().ok()?;
    serde_json::from_slice(&out.stdout).ok()
}

#[cfg(target_os = "macos")]
pub fn os_disks() -> Vec<Disk> {
    let Some(list) = plist_json(&["list", "-plist", "physical"]) else { return vec![] };
    // the whole disk(s) of "/" (APFS: the physical store behind the container)
    let root = plist_json(&["info", "-plist", "/"]).unwrap_or_default();
    let mut roots = vec![s(&root, "ParentWholeDisk")];
    for st in arr(&root["APFSPhysicalStores"]) {
        let id = s(&st, "APFSPhysicalStore");
        if let Some(i) = plist_json(&["info", "-plist", &id]) {
            roots.push(s(&i, "ParentWholeDisk"));
        }
    }
    parse_macos(&list.to_string(), |id| plist_json(&["info", "-plist", id]), &roots)
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
pub fn os_disks() -> Vec<Disk> {
    vec![]
}
