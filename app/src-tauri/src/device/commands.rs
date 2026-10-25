//! The privileged commands, generated from a [`Plan`] (pure; tests assert them exactly).
//!
//! - Linux: udisks2 D-Bus calls, run with `gdbus call --system` (GVariant text arguments).
//!   Calls a user started pass `auth.no_user_interaction = false` (polkit may ask); probes are
//!   never made over D-Bus at all (enumeration reads sysfs, `CanFormat` is a tool lookup).
//! - Windows: one PowerShell Storage-module script run by the elevated helper with an absolute
//!   powershell.exe path and `-EncodedCommand`.
//! - macOS: `diskutil partitionDisk` argv; if it needs admin rights, the same argv through
//!   `osascript ... with administrator privileges`, every argument a `quoted form of` literal.

use super::{DataFs, Disk, Plan, Table};

pub const UDISKS: &str = "org.freedesktop.UDisks2";
pub const GDBUS: &str = "/usr/bin/gdbus";
pub const POWERSHELL: &str = r"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe";
pub const DISKUTIL: &str = "/usr/sbin/diskutil";
pub const OSASCRIPT: &str = "/usr/bin/osascript";

/// MBR types and GPT type GUIDs (spec 1).
pub fn mbr_type(fs: &str) -> &'static str {
    if matches!(fs, "ext4" | "btrfs") { "0x83" } else { "0x07" }
}
pub fn gpt_type(fs: &str) -> &'static str {
    if matches!(fs, "ext4" | "btrfs") { "0fc63daf-8483-4772-8e79-3d69d8477de4" } else { "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7" }
}

// ---------------------------------------------------------------------------------------------
// Linux: udisks2

/// One D-Bus method call. `object` may be a placeholder (`$client`, `$data`) for a partition
/// created by an earlier call; the runner substitutes the object path that call returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbusCall {
    pub step: &'static str,
    pub object: String,
    /// `org.freedesktop.UDisks2.Block.Format`
    pub method: String,
    /// GVariant text, one per method argument.
    pub args: Vec<String>,
}

impl DbusCall {
    pub fn argv(&self) -> Vec<String> {
        let mut v: Vec<String> = ["call", "--system", "--dest", UDISKS, "--object-path", &self.object, "--method", &self.method]
            .iter()
            .map(|s| s.to_string())
            .collect();
        v.extend(self.args.iter().cloned());
        v
    }
    pub fn display(&self) -> String {
        format!("gdbus {}", self.argv().iter().map(|a| sh_quote(a)).collect::<Vec<_>>().join(" "))
    }
}

/// A GVariant string literal.
pub fn gv_str(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// udisks object path of a block device: `/dev/sdd` -> `.../block_devices/sdd` (bytes other
/// than [A-Za-z0-9] escaped as `_xx`, as udisks does).
pub fn block_object(dev: &str) -> String {
    let name = dev.strip_prefix("/dev/").unwrap_or(dev);
    let mut out = String::from("/org/freedesktop/UDisks2/block_devices/");
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() {
            out.push(b as char);
        } else {
            out.push_str(&format!("_{b:02x}"));
        }
    }
    out
}

const INTERACTIVE: &str = "'auth.no_user_interaction': <false>";

/// Unmount everything mounted on the disk, write the table, create and format both partitions.
pub fn linux_calls(plan: &Plan) -> Vec<DbusCall> {
    let mut v = vec![];
    for p in plan.disk.partitions.iter().filter(|p| !p.mountpoints.is_empty()) {
        v.push(DbusCall {
            step: "unmount",
            object: block_object(&p.path),
            method: format!("{UDISKS}.Filesystem.Unmount"),
            args: vec![format!("{{{INTERACTIVE}, 'force': <false>}}")],
        });
    }
    let disk = block_object(&plan.disk.os_path);
    let table = match plan.table {
        Table::Mbr => "dos",
        Table::Gpt => "gpt",
    };
    v.push(DbusCall { step: "partition", object: disk.clone(), method: format!("{UDISKS}.Block.Format"), args: vec![gv_str(table), format!("{{{INTERACTIVE}}}")] });
    for (step, spec) in [("format_client", &plan.client), ("format_data", &plan.data)] {
        let (ptype, name, popts) = match plan.table {
            // MBR: no names; the first is a primary partition (so is the second: only two)
            Table::Mbr => (mbr_type(&spec.fs).to_owned(), String::new(), format!("{{'partition-type': <'primary'>, {INTERACTIVE}}}")),
            Table::Gpt => (gpt_type(&spec.fs).to_owned(), spec.label.clone(), format!("{{{INTERACTIVE}}}")),
        };
        v.push(DbusCall {
            step,
            object: disk.clone(),
            method: format!("{UDISKS}.PartitionTable.CreatePartitionAndFormat"),
            args: vec![
                format!("uint64 {}", spec.offset),
                format!("uint64 {}", spec.size),
                gv_str(&ptype),
                gv_str(&name),
                popts,
                gv_str(&spec.fs),
                format!("{{'label': <{}>, 'take-ownership': <true>, {INTERACTIVE}}}", gv_str(&spec.label)),
            ],
        });
    }
    v
}

/// Mounting a created partition (unprivileged on an active session: `filesystem-mount` = yes).
pub fn linux_mount(object: &str) -> DbusCall {
    DbusCall { step: "mount", object: object.to_owned(), method: format!("{UDISKS}.Filesystem.Mount"), args: vec![format!("{{{INTERACTIVE}}}")] }
}

/// The object path in a `gdbus call` reply such as `(objectpath '/org/.../sdd1',)`.
pub fn parse_object_path(reply: &str) -> Option<String> {
    let i = reply.find("objectpath '")? + "objectpath '".len();
    let rest = &reply[i..];
    Some(rest[..rest.find('\'')?].to_owned())
}

/// The mount path in a Mount reply: `('/run/media/u/VaultData',)`.
pub fn parse_string_reply(reply: &str) -> Option<String> {
    let i = reply.find('\'')? + 1;
    let rest = &reply[i..];
    Some(rest[..rest.find('\'')?].to_owned())
}

// ---------------------------------------------------------------------------------------------
// Windows: PowerShell Storage cmdlets in the elevated helper

/// A PowerShell single-quoted literal.
pub fn ps_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Exit codes of the script / helper.
pub const EXIT_CHANGED: i32 = 20;
pub const EXIT_REFUSED: i32 = 21;

pub fn windows_script(plan: &Plan) -> String {
    let n = plan.disk.number.unwrap_or(u32::MAX);
    let style = match plan.table {
        Table::Mbr => "MBR",
        Table::Gpt => "GPT",
    };
    let ptype = |fs: &str| match plan.table {
        Table::Mbr => "-MbrType IFS".to_owned(),
        Table::Gpt => format!("-GptType '{{{}}}'", gpt_type(fs)),
    };
    let wfs = |fs: &str| if fs == "ntfs" { "NTFS" } else { "exFAT" };
    let lines = [
        "$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'".to_owned(),
        format!("$n={n}"),
        "Write-Output 'NV-STEP check'".to_owned(),
        "$d=Get-Disk -Number $n".to_owned(),
        format!(
            "if (\"$($d.SerialNumber)\".Trim() -ne {} -or $d.Size -ne {} -or $d.IsBoot -or $d.IsSystem -or $d.BootFromDisk -or @(7,12,13) -notcontains [int]$d.BusType) {{ exit {EXIT_CHANGED} }}",
            ps_str(plan.disk.serial.trim()),
            plan.disk.size
        ),
        "Write-Output 'NV-STEP unmount'".to_owned(),
        "if ($d.IsReadOnly) { Set-Disk -Number $n -IsReadOnly $false }".to_owned(),
        "if ($d.IsOffline) { Set-Disk -Number $n -IsOffline $false }".to_owned(),
        "Write-Output 'NV-STEP partition'".to_owned(),
        "if ($d.PartitionStyle -ne 'RAW') { Clear-Disk -Number $n -RemoveData -RemoveOEM -Confirm:$false }".to_owned(),
        format!("Initialize-Disk -Number $n -PartitionStyle {style}"),
        "Write-Output 'NV-STEP format_client'".to_owned(),
        format!("$p1=New-Partition -DiskNumber $n -Offset {} -Size {} {}", plan.client.offset, plan.client.size, ptype(&plan.client.fs)),
        format!("Format-Volume -Partition $p1 -FileSystem exFAT -NewFileSystemLabel {} -Confirm:$false | Out-Null", ps_str(&plan.client.label)),
        "Write-Output 'NV-STEP format_data'".to_owned(),
        format!("$p2=New-Partition -DiskNumber $n -UseMaximumSize {}", ptype(&plan.data.fs)),
        format!(
            "Format-Volume -Partition $p2 -FileSystem {} -NewFileSystemLabel {} -Confirm:$false | Out-Null",
            wfs(&plan.data.fs),
            ps_str(&plan.data.label)
        ),
        "Write-Output 'NV-STEP mount'".to_owned(),
        "$p1,$p2 | Add-PartitionAccessPath -AssignDriveLetter".to_owned(),
        "Write-Output 'NV-STEP done'".to_owned(),
    ];
    lines.join("\r\n")
}

/// `-EncodedCommand`: base64 of the UTF-16LE script.
pub fn encode_ps(script: &str) -> String {
    use base64::Engine;
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn windows_argv(plan: &Plan) -> Vec<String> {
    ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"]
        .iter()
        .map(|s| s.to_string())
        .chain([encode_ps(&windows_script(plan))])
        .collect()
}

// ---------------------------------------------------------------------------------------------
// macOS: diskutil

pub fn mac_fs(fs: &str) -> &'static str {
    match fs {
        "apfs" => "APFS",
        _ => "ExFAT",
    }
}

pub fn mac_device(d: &Disk) -> String {
    if d.os_path.starts_with("/dev/") { d.os_path.clone() } else { format!("/dev/{}", d.id) }
}

pub fn mac_unmount_args(plan: &Plan) -> Vec<String> {
    vec!["unmountDisk".into(), mac_device(&plan.disk)]
}

pub fn mac_partition_args(plan: &Plan) -> Vec<String> {
    let table = match plan.table {
        Table::Mbr => "MBR",
        Table::Gpt => "GPT",
    };
    [
        "partitionDisk",
        &mac_device(&plan.disk),
        "2",
        table,
        mac_fs(&plan.client.fs),
        &plan.client.label,
        &format!("{}M", plan.client.size / super::MIB),
        mac_fs(&plan.data.fs),
        &plan.data.label,
        "R",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// An AppleScript string literal.
pub fn as_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `do shell script` with every argument as `quoted form of "<literal>"` (never raw).
pub fn mac_osascript(args: &[String], prompt: &str) -> String {
    let mut cmd = format!("quoted form of {}", as_str(DISKUTIL));
    for a in args {
        cmd.push_str(&format!(" & space & quoted form of {}", as_str(a)));
    }
    format!("do shell script {cmd} with administrator privileges with prompt {}", as_str(prompt))
}

pub fn mac_prompt(d: &Disk, lang_it: bool) -> String {
    let name = if d.name().is_empty() { d.id.clone() } else { d.name() };
    if lang_it { format!("NiceVault deve formattare {name}") } else { format!("NiceVault needs to format {name}") }
}

/// A stderr of diskutil meaning "run me as admin".
pub fn mac_needs_admin(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    ["permission", "not permitted", "-69877", "must be run as root", "privilege"].iter().any(|k| s.contains(k))
}

// ---------------------------------------------------------------------------------------------

pub fn sh_quote(s: &str) -> String {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./:=,@%+".contains(&b)) {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Human-readable list of every privileged command of the plan (what will run, verbatim).
pub fn preview(plan: &Plan) -> Vec<String> {
    match plan.os {
        super::Os::Linux => {
            let mut v: Vec<String> = linux_calls(plan).iter().map(DbusCall::display).collect();
            v.push(format!("{} ($client, $data)", linux_mount("$partition").display()));
            if plan.data_fs.unix_perms() {
                v.push("chmod 0777 <VaultData> (as you: the owner after take-ownership)".into());
            }
            v
        }
        super::Os::Windows => {
            let mut v = vec![format!("UAC: {} --device-helper <plan.json>", "NiceVault.exe")];
            v.push(format!("{POWERSHELL} -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand <script below>"));
            v.extend(windows_script(plan).split("\r\n").map(str::to_owned));
            v
        }
        super::Os::Macos => {
            let un = mac_unmount_args(plan);
            let pa = mac_partition_args(plan);
            let line = |a: &[String]| format!("{DISKUTIL} {}", a.iter().map(|x| sh_quote(x)).collect::<Vec<_>>().join(" "));
            vec![
                line(&un),
                line(&pa),
                format!("(if it needs admin rights) {OSASCRIPT} -e {}", sh_quote(&mac_osascript(&pa, &mac_prompt(&plan.disk, false)))),
            ]
        }
    }
}

/// `DataFs` of a Windows Format-Volume / macOS diskutil name (for re-reading the layout).
pub fn fs_from_os_name(s: &str) -> Option<DataFs> {
    match s.to_ascii_lowercase().as_str() {
        "exfat" => Some(DataFs::Exfat),
        "ntfs" => Some(DataFs::Ntfs),
        "ext4" => Some(DataFs::Ext4),
        "btrfs" => Some(DataFs::Btrfs),
        "apfs" => Some(DataFs::Apfs),
        _ => None,
    }
}
