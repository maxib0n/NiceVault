use super::backend::{DiskOps, Linux, Mac, ok_output};
use super::commands::*;
use super::job::Log;
use super::*;
use std::sync::{Arc, Mutex};

const GB: u64 = 1_000_000_000;

fn stick() -> Disk {
    Disk {
        id: "/dev/sdd".into(),
        os_path: "/dev/sdd".into(),
        vendor: "SanDisk".into(),
        model: "SanDisk Ultra".into(),
        serial: "4C530001071103235".into(),
        bus: "usb".into(),
        size: 30_752_000_000,
        removable: true,
        partitions: vec![Partition {
            path: "/dev/sdd1".into(),
            label: "STICK".into(),
            fs: "vfat".into(),
            size: 30 * GB,
            mountpoints: vec!["/run/media/u/STICK".into()],
        }],
        ..Default::default()
    }
}

fn ctx() -> HostCtx {
    HostCtx {
        protected: ["/", "/home", "/home/u", "/home/u/.config", "/opt/nv/NiceVault"].map(PathBuf::from).to_vec(),
        swap: vec!["/dev/nvme0n1p3".into()],
        open_volumes: vec![],
        allow_virtual: false,
    }
}

fn opts(fs: DataFs) -> PlanOpts {
    PlanOpts { id: "/dev/sdd".into(), table: Table::Mbr, data_fs: fs, first_volume: None, download: true }
}

#[test]
fn refusal_rules() {
    let c = ctx();
    assert_eq!(refusal(&stick(), &c), None);
    let mut d = stick();
    d.is_system = true;
    assert_eq!(refusal(&d, &c), Some(Refusal::System));
    let mut d = stick();
    (d.removable, d.bus) = (false, "nvme".into());
    assert_eq!(refusal(&d, &c), Some(Refusal::Internal));
    let mut d = stick();
    d.removable = false;
    assert_eq!(refusal(&d, &c), None);
    let mut d = stick();
    (d.is_virtual, d.removable, d.bus) = (true, false, "virtual".into());
    assert_eq!(refusal(&d, &c), Some(Refusal::Virtual));
    assert_eq!(refusal(&d, &HostCtx { allow_virtual: true, ..ctx() }), None, "test override");
    let mut d = stick();
    d.size = GIB - 1;
    assert_eq!(refusal(&d, &c), Some(Refusal::TooSmall));
    let mut d = stick();
    d.read_only = true;
    assert_eq!(refusal(&d, &c), Some(Refusal::ReadOnly));
    let mut d = stick();
    d.partitions[0].mountpoints = vec!["/opt/nv".into()];
    assert_eq!(refusal(&d, &c), Some(Refusal::HostsApp));
    let mut d = stick();
    d.partitions[0].mountpoints = vec!["/".into()];
    assert_eq!(refusal(&d, &c), Some(Refusal::HostsApp));
    let mut d = stick();
    d.partitions[0].path = "/dev/nvme0n1p3".into();
    assert_eq!(refusal(&d, &c), Some(Refusal::HostsApp));
    let c2 = HostCtx { open_volumes: vec![PathBuf::from("/run/media/u/STICK/Foto.nvault")], ..ctx() };
    assert_eq!(refusal(&stick(), &c2), Some(Refusal::OpenVolume));
    let c3 = HostCtx { open_volumes: vec![PathBuf::from("/run/media/u/STICK2/a.nvault")], ..ctx() };
    assert_eq!(refusal(&stick(), &c3), None);
    let mut d = stick();
    d.partitions[0].mountpoints = vec![r"E:\".into()];
    let cw = HostCtx { protected: vec![PathBuf::from(r"C:\Users\u")], open_volumes: vec![PathBuf::from(r"e:\Foto.nvault")], ..Default::default() };
    assert_eq!(refusal(&d, &cw), Some(Refusal::OpenVolume));
    assert_eq!(Refusal::TooSmall.error().code, "device_refused");
    assert_eq!(Refusal::TooSmall.error().message, "too_small");
}

#[test]
fn candidates_filter_and_sort() {
    let mut internal = stick();
    (internal.id, internal.removable, internal.bus, internal.vendor, internal.model) = ("/dev/nvme0n1".into(), false, "nvme".into(), String::new(), "Samsung".into());
    let mut b = stick();
    (b.id, b.vendor, b.model) = ("/dev/sde".into(), "Kingston".into(), "DataTraveler".into());
    let l = candidates(vec![stick(), internal, b], &ctx());
    assert_eq!(l.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["/dev/sde", "/dev/sdd"]);
}

#[test]
fn names_and_serial_tail() {
    assert_eq!(stick().name(), "SanDisk Ultra", "vendor not repeated");
    assert_eq!(stick().serial_tail(), "1103235");
    let d = Disk { vendor: "  Generic ".into(), model: " Flash   Disk ".into(), serial: "12".into(), ..Default::default() };
    assert_eq!((d.name().as_str(), d.serial_tail().as_str()), ("Generic Flash Disk", "12"));
    assert_eq!(Disk::default().name(), "");
}

#[test]
fn plan_mbr_default_layout() {
    let p = plan(&stick(), &opts(DataFs::Exfat), Os::Linux, &ctx(), "p1").unwrap();
    assert_eq!((p.table, p.table_forced), (Table::Mbr, false));
    assert_eq!((p.client.offset, p.client.size, p.client.label.as_str(), p.client.fs.as_str()), (1_048_576, 536_870_912, "NiceVault", "exfat"));
    assert_eq!((p.data.offset, p.data.size, p.data.label.as_str()), (537_919_488, 0, "VaultData"));
    assert_eq!(p.data.offset % MIB, 0);
    assert_eq!(p.steps, ["check", "download", "unmount", "partition", "format_client", "format_data", "mount", "copy", "verify", "marker"]);
    assert_eq!(p.confirm_name, "SanDisk Ultra");
    assert!(!p.commands_preview.is_empty());
    let mut o = opts(DataFs::Exfat);
    o.download = false;
    o.first_volume = Some(VolumeOpts { mode: VolumeMode::Whole, size: None, name: "Dati".into() });
    let p = plan(&stick(), &o, Os::Linux, &ctx(), "p2").unwrap();
    assert!(!p.steps.contains(&"download".to_owned()));
    assert_eq!(p.steps.last().unwrap(), "volume");
}

#[test]
fn plan_forces_gpt_and_checks_fs() {
    let mut big = stick();
    big.size = 4 << 40;
    let p = plan(&big, &opts(DataFs::Exfat), Os::Linux, &ctx(), "x").unwrap();
    assert_eq!((p.table, p.table_forced), (Table::Gpt, true), "over 2 TiB");
    let p = plan(&stick(), &opts(DataFs::Apfs), Os::Macos, &ctx(), "x").unwrap();
    assert_eq!((p.table, p.table_forced), (Table::Gpt, true), "APFS needs GPT");
    let mut g = opts(DataFs::Exfat);
    g.table = Table::Gpt;
    assert_eq!(plan(&stick(), &g, Os::Linux, &ctx(), "x").unwrap().table, Table::Gpt);
    assert_eq!(plan(&stick(), &opts(DataFs::Exfat), Os::Windows, &ctx(), "x").unwrap_err().code, "device_not_found", "no disk number");
    let mut wd = stick();
    wd.number = Some(3);
    for (fs, os) in [(DataFs::Ext4, Os::Windows), (DataFs::Ntfs, Os::Macos), (DataFs::Apfs, Os::Linux), (DataFs::Btrfs, Os::Macos)] {
        assert_eq!(plan(&wd, &opts(fs), os, &ctx(), "x").unwrap_err().code, "fs_unsupported", "{fs:?} on {os:?}");
    }
    let mut d = stick();
    d.is_system = true;
    assert_eq!(plan(&d, &opts(DataFs::Exfat), Os::Linux, &ctx(), "x").unwrap_err().code, "device_refused");
    let mut o = opts(DataFs::Exfat);
    o.first_volume = Some(VolumeOpts { mode: VolumeMode::Piece, size: Some(40 * GB), name: "x".into() });
    assert_eq!(plan(&stick(), &o, Os::Linux, &ctx(), "x").unwrap_err().code, "invalid_size");
}

#[test]
fn readability_table() {
    use Access::*;
    assert_eq!(DataFs::Exfat.readable_on(), [(Os::Windows, Rw), (Os::Macos, Rw), (Os::Linux, Rw)]);
    assert_eq!(DataFs::Ntfs.readable_on(), [(Os::Windows, Rw), (Os::Macos, Ro), (Os::Linux, Rw)]);
    assert_eq!(DataFs::Ext4.readable_on()[2], (Os::Linux, Rw));
    assert_eq!(DataFs::Apfs.readable_on(), [(Os::Windows, No), (Os::Macos, Rw), (Os::Linux, Ro)]);
    assert!(DataFs::Exfat.creatable_on(Os::Windows) && DataFs::Exfat.creatable_on(Os::Macos) && DataFs::Exfat.creatable_on(Os::Linux));
    assert!(!DataFs::Ntfs.creatable_on(Os::Macos) && DataFs::Ntfs.creatable_on(Os::Windows));
    assert_eq!(DataFs::Btrfs.linux_tool(), Some(("mkfs.btrfs", "btrfs-progs")));
}

#[test]
fn confirmation_matching() {
    let p = plan(&stick(), &opts(DataFs::Exfat), Os::Linux, &ctx(), "x").unwrap();
    assert!(confirmation_ok(&p, "SanDisk Ultra"));
    assert!(confirmation_ok(&p, "  sandisk   ULTRA "));
    assert!(!confirmation_ok(&p, "SanDisk"));
    assert!(!confirmation_ok(&p, ""));
    assert!(!confirmation_ok(&p, "ERASE"));
    let mut d = stick();
    (d.vendor, d.model) = (String::new(), String::new());
    let p = plan(&d, &opts(DataFs::Exfat), Os::Linux, &ctx(), "x").unwrap();
    assert!(confirmation_ok(&p, "CANCELLA") && confirmation_ok(&p, "erase"));
    assert!(!confirmation_ok(&p, "") && !confirmation_ok(&p, "SanDisk Ultra"));
}

#[test]
fn resume_decision_table() {
    let mut d = stick();
    assert_eq!(resume_action(&d, None), Resume::Redo, "not our layout");
    d.partitions = vec![
        Partition { path: "/dev/sdd1".into(), label: "NiceVault".into(), fs: "exfat".into(), ..Default::default() },
        Partition { path: "/dev/sdd2".into(), label: "VaultData".into(), fs: "ntfs".into(), ..Default::default() },
    ];
    assert_eq!(resume_action(&d, Some(false)), Resume::Complete);
    assert_eq!(resume_action(&d, None), Resume::Complete);
    assert_eq!(resume_action(&d, Some(true)), Resume::Done);
    d.partitions[0].fs = "vfat".into();
    assert_eq!(resume_action(&d, Some(true)), Resume::Redo, "client must be exFAT");
    d.partitions.clear();
    assert_eq!(state_of(&d), NvState::None);
}

#[test]
fn nicevault_state_from_marker() {
    let t = tempfile::tempdir().unwrap();
    let mut d = stick();
    d.partitions[0].label = "NiceVault".into();
    d.partitions[0].mountpoints = vec![t.path().to_string_lossy().into_owned()];
    assert_eq!(state_of(&d), NvState::Incomplete);
    std::fs::write(t.path().join(MARKER), "{}").unwrap();
    assert_eq!(state_of(&d), NvState::Complete);
}

#[test]
fn whole_volume_reserve() {
    assert_eq!(whole_volume_size(30 * GB) % MIB, 0);
    assert!(whole_volume_size(30 * GB) < 30 * GB - 16 * MIB && whole_volume_size(30 * GB) > 29 * GB);
    assert_eq!(whole_volume_size(10 * MIB), 0);
    assert!(30 * GB - whole_volume_size(30 * GB) <= 512 * MIB + MIB);
}

#[test]
fn udisks_calls_exact() {
    let p = plan(&stick(), &opts(DataFs::Exfat), Os::Linux, &ctx(), "x").unwrap();
    let c = linux_calls(&p);
    let s: Vec<(&str, &str, &str)> = c.iter().map(|c| (c.step, c.object.as_str(), c.method.as_str())).collect();
    assert_eq!(
        s,
        [
            ("unmount", "/org/freedesktop/UDisks2/block_devices/sdd1", "org.freedesktop.UDisks2.Filesystem.Unmount"),
            ("partition", "/org/freedesktop/UDisks2/block_devices/sdd", "org.freedesktop.UDisks2.Block.Format"),
            ("format_client", "/org/freedesktop/UDisks2/block_devices/sdd", "org.freedesktop.UDisks2.PartitionTable.CreatePartitionAndFormat"),
            ("format_data", "/org/freedesktop/UDisks2/block_devices/sdd", "org.freedesktop.UDisks2.PartitionTable.CreatePartitionAndFormat"),
        ]
    );
    assert_eq!(c[0].args, ["{'auth.no_user_interaction': <false>, 'force': <false>}"]);
    assert_eq!(c[1].args, ["'dos'", "{'auth.no_user_interaction': <false>}"]);
    assert_eq!(
        c[2].args,
        [
            "uint64 1048576",
            "uint64 536870912",
            "'0x07'",
            "''",
            "{'partition-type': <'primary'>, 'auth.no_user_interaction': <false>}",
            "'exfat'",
            "{'label': <'NiceVault'>, 'take-ownership': <true>, 'auth.no_user_interaction': <false>}",
        ]
    );
    assert_eq!(c[3].args[..4], ["uint64 537919488", "uint64 0", "'0x07'", "''"]);
    assert_eq!(c[3].args[5], "'exfat'");
    assert_eq!(
        c[1].argv(),
        [
            "call", "--system", "--dest", "org.freedesktop.UDisks2", "--object-path", "/org/freedesktop/UDisks2/block_devices/sdd", "--method",
            "org.freedesktop.UDisks2.Block.Format", "'dos'", "{'auth.no_user_interaction': <false>}"
        ]
    );
    let mut o = opts(DataFs::Ext4);
    o.table = Table::Gpt;
    let p = plan(&stick(), &o, Os::Linux, &ctx(), "x").unwrap();
    let c = linux_calls(&p);
    assert_eq!(c[1].args[0], "'gpt'");
    assert_eq!(c[2].args[2..5], ["'ebd0a0a2-b9e5-4433-87c0-68b6b72699c7'", "'NiceVault'", "{'auth.no_user_interaction': <false>}"]);
    assert_eq!(c[3].args[2..4], ["'0fc63daf-8483-4772-8e79-3d69d8477de4'", "'VaultData'"]);
    assert_eq!(c[3].args[5], "'ext4'");
    assert!(p.commands_preview.iter().any(|l| l.contains("chmod 0777")));
    assert!(c.iter().all(|c| c.args.last().unwrap().contains("'auth.no_user_interaction': <false>")));
    assert!(!c.iter().any(|c| c.method.contains("Loop") || c.method.contains("CanFormat")));
}

#[test]
fn udisks_helpers() {
    assert_eq!(block_object("/dev/mmcblk0"), "/org/freedesktop/UDisks2/block_devices/mmcblk0");
    assert_eq!(block_object("/dev/dm-1"), "/org/freedesktop/UDisks2/block_devices/dm_2d1");
    assert_eq!(gv_str("it's"), r"'it\'s'");
    assert_eq!(parse_object_path("(objectpath '/org/freedesktop/UDisks2/block_devices/sdd1',)\n").as_deref(), Some("/org/freedesktop/UDisks2/block_devices/sdd1"));
    assert_eq!(parse_string_reply("('/run/media/u/VaultData',)\n").as_deref(), Some("/run/media/u/VaultData"));
    assert_eq!(udisks_err("partition", "GDBus.Error:org.freedesktop.UDisks2.Error.NotAuthorizedDismissed: Not authorized"), "elevation_denied");
    assert_eq!(udisks_err("partition", "GDBus.Error:org.freedesktop.UDisks2.Error.NotAuthorized: no agent"), "elevation_unavailable");
    assert_eq!(udisks_err("partition", "Error opening /dev/sdd: Device or resource busy"), "device_busy");
    assert_eq!(udisks_err("partition", "Error creating partition table: x"), "partition_failed");
    assert_eq!(udisks_err("format_data", "mkfs failed"), "format_failed");
    assert_eq!(udisks_err("format_data", "GDBus.Error:org.freedesktop.DBus.Error.UnknownObject: No such object"), "device_gone");
}

fn udisks_err(step: &str, e: &str) -> String {
    super::backend::udisks_error(step, e).code
}

type Calls = Arc<Mutex<Vec<(String, Vec<String>)>>>;

fn recorder(calls: Calls, fail_on: Option<&'static str>) -> super::backend::Runner {
    Box::new(move |prog: &str, args: &[String]| {
        calls.lock().unwrap().push((prog.to_owned(), args.to_vec()));
        if let Some(m) = fail_on
            && args.iter().any(|a| a.ends_with(m))
        {
            #[cfg(unix)]
            let status = std::os::unix::process::ExitStatusExt::from_raw(1 << 8);
            #[cfg(windows)]
            let status = std::os::windows::process::ExitStatusExt::from_raw(1);
            return Ok(std::process::Output { status, stdout: vec![], stderr: b"GDBus.Error:org.freedesktop.UDisks2.Error.NotAuthorizedDismissed: no".to_vec() });
        }
        Ok(ok_output("(objectpath '/org/freedesktop/UDisks2/block_devices/sdd1',)"))
    })
}

#[test]
fn linux_backend_runs_exactly_the_plan() {
    let p = plan(&stick(), &opts(DataFs::Exfat), Os::Linux, &ctx(), "x").unwrap();
    let calls: Calls = Arc::default();
    let l = Linux::new(recorder(calls.clone(), None));
    let mut steps = vec![];
    l.partition(&p, &Log::null(), &mut |s| steps.push(s.to_owned())).unwrap();
    assert_eq!(steps, ["unmount", "partition", "format_client", "format_data"]);
    let got = calls.lock().unwrap().clone();
    let want: Vec<(String, Vec<String>)> = linux_calls(&p).iter().map(|c| (String::new(), c.argv())).collect();
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(&want) {
        assert!(g.0.ends_with("gdbus"));
        assert_eq!(g.1, w.1);
    }
    let calls: Calls = Arc::default();
    let l = Linux::new(recorder(calls.clone(), Some("Block.Format")));
    assert_eq!(l.partition(&p, &Log::null(), &mut |_| {}).unwrap_err().code, "elevation_denied");
    assert_eq!(calls.lock().unwrap().len(), 2, "nothing after the refused call");
}

#[test]
fn windows_script_exact() {
    let mut d = stick();
    (d.id, d.os_path, d.number, d.serial) = ("PhysicalDrive3".into(), r"\\.\PhysicalDrive3".into(), Some(3), "AA'; Format-Volume C".into());
    d.partitions[0].mountpoints = vec![r"E:\".into()];
    let mut o = opts(DataFs::Ntfs);
    o.id = d.id.clone();
    let p = plan(&d, &o, Os::Windows, &ctx(), "x").unwrap();
    let s = windows_script(&p);
    let lines: Vec<&str> = s.split("\r\n").collect();
    assert_eq!(lines[1], "$n=3");
    assert_eq!(
        lines[4],
        "if (\"$($d.SerialNumber)\".Trim() -ne 'AA''; Format-Volume C' -or $d.Size -ne 30752000000 -or $d.IsBoot -or $d.IsSystem -or $d.BootFromDisk -or @(7,12,13) -notcontains [int]$d.BusType) { exit 20 }",
        "the serial is a quoted literal (quote doubled)"
    );
    assert!(lines.contains(&"Initialize-Disk -Number $n -PartitionStyle MBR"));
    assert!(lines.contains(&"$p1=New-Partition -DiskNumber $n -Offset 1048576 -Size 536870912 -MbrType IFS"));
    assert!(lines.contains(&"Format-Volume -Partition $p1 -FileSystem exFAT -NewFileSystemLabel 'NiceVault' -Confirm:$false | Out-Null"));
    assert!(lines.contains(&"$p2=New-Partition -DiskNumber $n -UseMaximumSize -MbrType IFS"));
    assert!(lines.contains(&"Format-Volume -Partition $p2 -FileSystem NTFS -NewFileSystemLabel 'VaultData' -Confirm:$false | Out-Null"));
    assert_eq!(lines[lines.len() - 2], "$p1,$p2 | Add-PartitionAccessPath -AssignDriveLetter");
    let mut o = o.clone();
    o.table = Table::Gpt;
    o.data_fs = DataFs::Exfat;
    let s = windows_script(&plan(&d, &o, Os::Windows, &ctx(), "x").unwrap());
    assert!(s.contains("Initialize-Disk -Number $n -PartitionStyle GPT"));
    assert!(s.contains("-GptType '{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}'"));
    use base64::Engine;
    let argv = windows_argv(&plan(&d, &o, Os::Windows, &ctx(), "x").unwrap());
    assert_eq!(argv[..5], ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"]);
    let raw = base64::engine::general_purpose::STANDARD.decode(&argv[5]).unwrap();
    let back = String::from_utf16(&raw.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()).unwrap();
    assert_eq!(back, s);
    assert!(POWERSHELL.starts_with("%SystemRoot%\\System32\\"), "absolute, never PATH");
}

#[test]
fn windows_helper_exit_codes_and_recheck() {
    use super::backend::helper_exit_error;
    assert!(helper_exit_error(0, "done").is_none());
    assert_eq!(helper_exit_error(20, "check").unwrap().code, "device_changed");
    assert_eq!(helper_exit_error(21, "check").unwrap().code, "device_refused");
    assert_eq!(helper_exit_error(1, "partition").unwrap().code, "partition_failed");
    assert_eq!(helper_exit_error(1, "format_data").unwrap().code, "format_failed");
    let mut d = stick();
    (d.id, d.number) = ("PhysicalDrive3".into(), Some(3));
    let mut o = opts(DataFs::Exfat);
    o.id = d.id.clone();
    let p = plan(&d, &o, Os::Windows, &ctx(), "x").unwrap();
    let c = HostCtx::default();
    assert_eq!(super::helper::recheck(&p, Some(&d), &c), Ok(()));
    assert_eq!(super::helper::recheck(&p, None, &c), Err(20));
    for change in [|d: &mut Disk| d.serial.push('9'), |d: &mut Disk| d.size += 512, |d: &mut Disk| d.is_system = true, |d: &mut Disk| d.bus = "sata".into(), |d: &mut Disk| d.number = Some(4)] {
        let mut n = d.clone();
        change(&mut n);
        assert_eq!(super::helper::recheck(&p, Some(&n), &c), Err(20));
    }
    let mut n = d.clone();
    n.read_only = true;
    assert_eq!(super::helper::recheck(&p, Some(&n), &c), Err(21));
}

#[test]
fn diskutil_args_and_osascript_quoting() {
    let mut d = stick();
    (d.id, d.os_path) = ("disk4".into(), "/dev/disk4".into());
    d.model = r#"Evil" & do shell script "rm -rf ~" & "\x'"#.into();
    let mut o = opts(DataFs::Exfat);
    o.id = d.id.clone();
    let p = plan(&d, &o, Os::Macos, &ctx(), "x").unwrap();
    assert_eq!(mac_unmount_args(&p), ["unmountDisk", "/dev/disk4"]);
    assert_eq!(mac_partition_args(&p), ["partitionDisk", "/dev/disk4", "2", "MBR", "ExFAT", "NiceVault", "512M", "ExFAT", "VaultData", "R"]);
    o.data_fs = DataFs::Apfs;
    let pa = plan(&d, &o, Os::Macos, &ctx(), "x").unwrap();
    assert_eq!(mac_partition_args(&pa)[3..], ["GPT", "ExFAT", "NiceVault", "512M", "APFS", "VaultData", "R"]);
    let script = mac_osascript(&mac_partition_args(&p), &mac_prompt(&p.disk, true));
    assert!(script.starts_with(r#"do shell script quoted form of "/usr/sbin/diskutil" & space & quoted form of "partitionDisk" & space & quoted form of "/dev/disk4""#));
    assert!(script.ends_with(&format!("with administrator privileges with prompt {}", as_str(&mac_prompt(&p.disk, true)))));
    assert!(script.contains(r#"Evil\" & do shell script \"rm -rf ~\""#), "the model's quotes stay inside the literal: {script}");
    let unescaped = script.as_bytes().iter().enumerate().filter(|(i, b)| **b == b'"' && (*i == 0 || script.as_bytes()[i - 1] != b'\\' || script.as_bytes()[i - 2] == b'\\')).count();
    assert_eq!(unescaped % 2, 0);
    assert_eq!(as_str(r#"a"b\c"#), r#""a\"b\\c""#);
    assert!(mac_needs_admin("Error: -69877: Couldn't open device"));
    assert!(!mac_needs_admin("Error: -69888: no such disk"), "unknown codes are not admin");
}

#[test]
fn mac_backend_falls_back_to_osascript() {
    let mut d = stick();
    (d.id, d.os_path) = ("disk4".into(), "/dev/disk4".into());
    let mut o = opts(DataFs::Exfat);
    o.id = d.id.clone();
    let p = plan(&d, &o, Os::Macos, &ctx(), "x").unwrap();
    let calls: Calls = Arc::default();
    let m = Mac::new(recorder(calls.clone(), None));
    if !cfg!(target_os = "macos") {
        assert_eq!(m.partition(&p, &Log::null(), &mut |_| {}).unwrap_err().code, "device_gone");
        assert!(calls.lock().unwrap().is_empty());
    }
}

fn w(p: &std::path::Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

#[test]
fn linux_sysfs_enumeration() {
    let t = tempfile::tempdir().unwrap();
    let sys = t.path().join("sys/block");
    let udev = t.path().join("udev");
    let sdd = t.path().join("sys/devices/pci0000:00/usb2/2-1/host6/block/sdd");
    w(&sdd.join("size"), "60062500\n");
    w(&sdd.join("removable"), "1\n");
    w(&sdd.join("ro"), "0\n");
    w(&sdd.join("dev"), "8:48\n");
    w(&sdd.join("sdd1/partition"), "1\n");
    w(&sdd.join("sdd1/size"), "60060452\n");
    w(&sdd.join("sdd1/dev"), "8:49\n");
    w(&udev.join("b8:48"), "E:ID_BUS=usb\nE:ID_VENDOR=SanDisk\nE:ID_MODEL=Ultra\nE:ID_SERIAL_SHORT=4C530001071103235\n");
    w(&udev.join("b8:49"), "E:ID_FS_TYPE=vfat\nE:ID_FS_LABEL=MY_STICK\nE:ID_FS_LABEL_ENC=MY\\x20STICK\n");
    let sdc = t.path().join("sys/devices/pci0000:00/usb2/2-3/2-3:1.0/host8/block/sdc");
    w(&sdc.join("size"), "1953525168\n");
    w(&sdc.join("removable"), "0\n");
    w(&sdc.join("dev"), "8:32\n");
    w(&udev.join("b8:32"), "E:ID_BUS=ata\nE:ID_MODEL=ST1000LM025\n");
    let nvme = t.path().join("sys/devices/pci0000:00/nvme/nvme0/nvme0n1");
    w(&nvme.join("size"), "1000215216\n");
    w(&nvme.join("removable"), "0\n");
    w(&nvme.join("dev"), "259:0\n");
    w(&nvme.join("nvme0n1p2/partition"), "2\n");
    w(&nvme.join("nvme0n1p2/dev"), "259:2\n");
    w(&nvme.join("nvme0n1p2/size"), "100\n");
    let dm = t.path().join("sys/devices/virtual/block/dm-0");
    w(&dm.join("dev"), "253:0\n");
    std::fs::create_dir_all(nvme.join("nvme0n1p2/holders")).unwrap();
    std::os::unix::fs::symlink(&dm, nvme.join("nvme0n1p2/holders/dm-0")).unwrap();
    let lo = t.path().join("sys/devices/virtual/block/loop0");
    w(&lo.join("size"), "4194304\n");
    w(&lo.join("dev"), "7:0\n");
    let sde = t.path().join("sys/devices/usb3/block/sde");
    w(&sde.join("size"), "0\n");
    std::fs::create_dir_all(&sys).unwrap();
    for (n, p) in [("sdc", &sdc), ("sdd", &sdd), ("nvme0n1", &nvme), ("loop0", &lo), ("sde", &sde), ("dm-0", &dm)] {
        std::os::unix::fs::symlink(p, sys.join(n)).unwrap();
    }
    let mi = "29 1 253:0 / / rw,relatime shared:1 - btrfs /dev/mapper/luks-x rw\n90 54 8:49 / /run/media/u/MY\\040STICK rw,nosuid shared:2 - vfat /dev/sdd1 rw\n";
    let d = super::enumerate::linux_disks(&sys, &udev, mi);
    assert_eq!(d.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["/dev/loop0", "/dev/nvme0n1", "/dev/sdc", "/dev/sdd"]);
    assert!(d[2].bus == "usb" && super::is_external(&d[2]), "a UAS bridge is external");
    let s = &d[3];
    assert_eq!((s.vendor.as_str(), s.model.as_str(), s.serial.as_str(), s.bus.as_str()), ("SanDisk", "Ultra", "4C530001071103235", "usb"));
    assert_eq!((s.size, s.removable, s.is_system, s.is_virtual), (60062500 * 512, true, false, false));
    assert_eq!(s.partitions[0].label, "MY STICK");
    assert_eq!(s.partitions[0].mountpoints, ["/run/media/u/MY STICK"]);
    let n = &d[1];
    assert!(n.is_system, "root through LUKS");
    assert_eq!((n.bus.as_str(), n.removable), ("nvme", false));
    assert!(d[0].is_virtual && d[0].bus == "virtual");
    let c = candidates(d, &HostCtx::default());
    assert_eq!(c.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["/dev/sdd", "/dev/sdc"]);
}

#[test]
fn windows_json_enumeration() {
    let json = r#"{"disks":[
        {"Number":0,"FriendlyName":"Samsung SSD","Manufacturer":"","Model":"Samsung SSD 980","SerialNumber":"S1","BusType":17,"Size":1000204886016,"IsBoot":true,"IsSystem":true,"BootFromDisk":true,"IsReadOnly":false},
        {"Number":3,"FriendlyName":"SanDisk Ultra","Manufacturer":"SanDisk","Model":"Ultra","SerialNumber":"  4C5300 ","BusType":7,"Size":30752000000,"IsBoot":false,"IsSystem":false,"BootFromDisk":false,"IsReadOnly":false},
        {"Number":4,"FriendlyName":"Msft Virtual Disk","Model":"Virtual Disk","SerialNumber":"","BusType":15,"Size":2147483648,"IsBoot":false,"IsSystem":false,"BootFromDisk":false,"IsReadOnly":false}],
      "partitions":[{"DiskNumber":3,"PartitionNumber":1,"DriveLetter":"E","Size":30751000000,"AccessPaths":["E:\\","\\\\?\\Volume{abc}\\"],"VolumePath":"\\\\?\\Volume{abc}\\"},
        {"DiskNumber":0,"PartitionNumber":3,"DriveLetter":"C","Size":999,"AccessPaths":["C:\\"],"VolumePath":"\\\\?\\Volume{c}\\"}],
      "volumes":{"Path":"\\\\?\\Volume{abc}\\","FileSystemLabel":"NiceVault","FileSystem":"exFAT"}}"#;
    let d = super::enumerate::parse_windows(json, "C:");
    assert_eq!(d.len(), 3);
    assert!(d[0].is_system && d[0].bus == "nvme");
    let s = &d[1];
    assert_eq!((s.id.as_str(), s.number, s.serial.as_str(), s.bus.as_str(), s.removable), ("PhysicalDrive3", Some(3), "4C5300", "usb", true));
    assert_eq!((s.partitions[0].label.as_str(), s.partitions[0].fs.as_str()), ("NiceVault", "exfat"));
    assert_eq!(s.partitions[0].mountpoints, [r"E:\"]);
    assert_eq!(s.nicevault, NvState::Incomplete);
    assert!(d[2].is_virtual && d[2].bus == "virtual");
    assert_eq!(candidates(d, &HostCtx::default()).len(), 1);
    assert!(super::enumerate::parse_windows("garbage", "C:").is_empty());
}

#[test]
fn macos_json_enumeration() {
    let list = r#"{"AllDisksAndPartitions":[
        {"DeviceIdentifier":"disk0","Size":500277790720,"Partitions":[{"DeviceIdentifier":"disk0s2","Size":500,"Content":"Apple_APFS"}]},
        {"DeviceIdentifier":"disk4","Size":30752000000,"Partitions":[{"DeviceIdentifier":"disk4s1","Size":30751000000,"VolumeName":"STICK","Content":"Windows_FAT_32","MountPoint":"/Volumes/STICK"}]},
        {"DeviceIdentifier":"disk5","Size":1073741824,"Partitions":[]}]}"#;
    let info = |id: &str| -> Option<serde_json::Value> {
        Some(match id {
            "disk0" => serde_json::json!({"Internal": true, "WholeDisk": true, "BusProtocol": "PCI-Express", "MediaName": "APPLE SSD", "Size": 500277790720u64}),
            "disk4" => serde_json::json!({"Internal": false, "WholeDisk": true, "RemovableMedia": true, "BusProtocol": "USB", "MediaName": "SanDisk Ultra", "Size": 30752000000u64, "WritableMedia": true}),
            _ => serde_json::json!({"Internal": false, "WholeDisk": true, "BusProtocol": "Disk Image", "VirtualOrPhysical": "Virtual", "Size": 1073741824u64}),
        })
    };
    let d = super::enumerate::parse_macos(list, info, &["disk0".to_owned()]);
    assert_eq!(d.len(), 3);
    assert!(d[0].is_system);
    assert_eq!((d[1].bus.as_str(), d[1].removable, d[1].model.as_str()), ("usb", true, "SanDisk Ultra"));
    assert_eq!(d[1].partitions[0].mountpoints, ["/Volumes/STICK"]);
    assert!(d[2].is_virtual);
    assert_eq!(candidates(d.clone(), &HostCtx::default()).len(), 1);
    assert_eq!(candidates(d, &HostCtx { allow_virtual: true, ..Default::default() }).len(), 2, "hdiutil ram disk with the test override");
}

#[test]
fn readme_lists_readability_and_missing() {
    let r = super::client::readme(true, DataFs::Ntfs, &[Os::Macos]);
    assert!(r.contains("File system: NTFS"));
    assert!(r.contains("Windows, macOS (solo lettura), Linux"));
    assert!(r.contains("Mancano i programmi per: macOS"));
    assert!(r.contains("\r\n"));
    let e = super::client::readme(false, DataFs::Exfat, &[]);
    assert!(e.contains("Readable on: Windows, macOS, Linux") && !e.contains("missing"));
    assert!(!e.contains('{'), "every placeholder filled");
}

#[test]
fn write_verified_replaces_atomically() {
    let t = tempfile::tempdir().unwrap();
    super::client::write_verified(t.path(), "a.bin", b"one").unwrap();
    super::client::write_verified(t.path(), "a.bin", b"two").unwrap();
    assert_eq!(std::fs::read(t.path().join("a.bin")).unwrap(), b"two");
    assert!(!t.path().join("a.bin.part").exists());
}

#[test]
#[ignore = "network: downloads the real public release (diagnostic)"]
fn real_release_downloads() {
    let l = super::client::Local { exe: std::path::PathBuf::new(), os: Os::Linux, version: env!("CARGO_PKG_VERSION").into(), pubkey: crate::updater::embedded_config().1 };
    let url = crate::updater::manifest_url();
    let b = super::client::download(&l, &url, &[Os::Linux, Os::Windows, Os::Macos], &mut |_, _| {});
    for f in &b.files {
        eprintln!("ok {:?} {} {} bytes v{:?}", f.os, f.name, f.bytes.len(), f.version);
    }
    for n in &b.notes {
        eprintln!("NOTE {n}");
    }
}
