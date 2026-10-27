use super::{DataFs, Marker, Os, Plan};
use crate::error::{ApiError, ApiResult};
use crate::updater;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const LINUX_NAME: &str = "NiceVault-linux-x86_64";
pub const WINDOWS_NAME: &str = "NiceVault-windows-x86_64.exe";
pub const LINUX_APPIMAGE: &str = "NiceVault-linux-x86_64.AppImage";
pub const MAC_ZIP: &str = "NiceVault-macos-universal.zip";
pub const MAC_APP: &str = "NiceVault.app";

pub const BUILDS: [(Os, &str, &str); 3] =
    [(Os::Linux, "linux-x86_64", LINUX_NAME), (Os::Windows, "windows-x86_64", WINDOWS_NAME), (Os::Macos, "darwin-aarch64", MAC_ZIP)];

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Download,
    Running,
    Sibling,
}

pub struct Build {
    pub os: Os,
    pub name: String,
    pub bytes: Vec<u8>,
    pub sig: Option<String>,
    pub version: Option<String>,
    pub origin: Origin,
}

pub struct Bundle {
    pub dir: PathBuf,
}

#[derive(Default)]
pub struct Builds {
    pub files: Vec<Build>,
    pub mac_bundle: Option<Bundle>,
    pub notes: Vec<String>,
}

impl Builds {
    pub fn has(&self, os: Os) -> bool {
        self.files.iter().any(|b| b.os == os) || (os == Os::Macos && self.mac_bundle.is_some())
    }
    pub fn missing(&self) -> Vec<Os> {
        Os::ALL.into_iter().filter(|o| !self.has(*o)).collect()
    }
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|b| b.bytes.len() as u64).sum::<u64>()
            + self.mac_bundle.as_ref().map_or(0, |b| dir_size(&b.dir))
    }
}

pub fn dir_size(p: &Path) -> u64 {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => std::fs::read_dir(p).map_or(0, |rd| rd.flatten().map(|e| dir_size(&e.path())).sum()),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

fn channel_key(m: &updater::Manifest, os: Os, base: &'static str) -> &'static str {
    if os == Os::Linux && m.platforms.contains_key("linux-x86_64-appimage") { "linux-x86_64-appimage" } else { base }
}

#[derive(Clone, Debug)]
pub struct Local {
    pub exe: PathBuf,
    pub os: Os,
    pub version: String,
    pub pubkey: String,
}

impl Local {
    pub fn this_app() -> Local {
        Local {
            exe: updater::app_exe().unwrap_or_default(),
            os: Os::current(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            pubkey: updater::embedded_config().1,
        }
    }
}

fn verified_sig(pubkey: &str, file: &Path, bytes: &[u8]) -> Option<(String, Option<String>)> {
    let sig = std::fs::read_to_string(updater::sibling(file, ".sig")).ok()?;
    let trusted = updater::verify_signature(pubkey, bytes, sig.trim()).ok()?;
    Some((sig.trim().to_owned(), updater::signed_version(&trusted)))
}

pub fn local_builds(l: &Local, want: &[Os]) -> Builds {
    let mut out = Builds::default();
    let dirs: Vec<PathBuf> = {
        let mut v = vec![];
        if let Some(d) = l.exe.parent() {
            v.push(d.to_path_buf());
        }
        if let Ok(updater::Target::Bundle(b)) = updater::target_for(&l.exe, true)
            && let Some(p) = b.parent()
        {
            v.push(p.to_path_buf());
        }
        v
    };
    for &(os, _, name) in BUILDS.iter().filter(|(o, _, _)| want.contains(o)) {
        if os == l.os {
            match os {
                Os::Linux | Os::Windows => {
                    if let Ok(bytes) = std::fs::read(&l.exe) {
                        let s = verified_sig(&l.pubkey, &l.exe, &bytes);
                        let version = s.as_ref().and_then(|s| s.1.clone()).or(Some(l.version.clone()));
                        out.files.push(Build { os, name: name.into(), sig: s.map(|s| s.0), bytes, version, origin: Origin::Running });
                        continue;
                    }
                }
                Os::Macos => {
                    if let Ok(updater::Target::Bundle(b)) = updater::target_for(&l.exe, true) {
                        let zip = dirs.iter().map(|d| d.join(MAC_ZIP)).find(|z| z.is_file());
                        if zip.is_none() {
                            out.mac_bundle = Some(Bundle { dir: b });
                            continue;
                        }
                    }
                }
            }
        }
        for d in &dirs {
            let candidates = if os == Os::Linux { vec![d.join(LINUX_APPIMAGE), d.join(name)] } else { vec![d.join(name)] };
            let Some((f, bytes)) = candidates.into_iter().find_map(|f| std::fs::read(&f).ok().map(|b| (f, b))) else { continue };
            if let Some((sig, version)) = verified_sig(&l.pubkey, &f, &bytes) {
                out.files.push(Build { os, name: name.into(), bytes, sig: Some(sig), version, origin: Origin::Sibling });
                break;
            }
        }
    }
    out
}

pub fn local_available(l: &Local, want: &[Os]) -> Vec<Os> {
    want.iter()
        .copied()
        .filter(|&os| (os == l.os && matches!(os, Os::Linux | Os::Windows) && l.exe.is_file()) || local_builds(l, &[os]).has(os))
        .collect()
}

pub fn download(l: &Local, manifest_url: &str, want: &[Os], progress: &mut dyn FnMut(u64, u64)) -> Builds {
    let mut out = Builds::default();
    match updater::fetch_manifest(manifest_url) {
            Ok(m) => {
                let mut done_before = 0u64;
                for &(os, base_key, name) in BUILDS.iter().filter(|(o, _, _)| want.contains(o)) {
                    let mut last = 0;
                    let key = channel_key(&m, os, base_key);
                    let r = updater::fetch_verified(manifest_url, &m, key, &l.pubkey, &l.version, &mut |d, t| {
                        last = d;
                        progress(done_before + d, done_before + t.unwrap_or(d) + 1);
                    });
                    done_before += last;
                    match r {
                        Ok(bytes) => out.files.push(Build {
                            os,
                            name: name.into(),
                            bytes,
                            sig: m.platforms.get(key).map(|a| a.signature.clone()),
                            version: Some(m.version.clone()),
                            origin: Origin::Download,
                        }),
                        Err(e) => {
                            out.notes.push(format!("{key}: {e}"));
                        }
                    }
                }
            }
            Err(e) => {
                out.notes.push(format!("latest.json: {e}"));
            }
    }
    out
}

pub fn gather(l: &Local, manifest_url: &str, download_them: bool, progress: &mut dyn FnMut(u64, u64)) -> Builds {
    let mut out = if download_them { download(l, manifest_url, &Os::ALL, progress) } else { Builds::default() };
    let want: Vec<Os> = Os::ALL.into_iter().filter(|o| !out.has(*o)).collect();
    let local = local_builds(l, &want);
    out.files.extend(local.files);
    out.mac_bundle = local.mac_bundle;
    out
}

fn sha(b: &[u8]) -> [u8; 32] {
    Sha256::digest(b).into()
}

fn sync_dir(_d: &Path) {
    #[cfg(unix)]
    if let Ok(f) = std::fs::File::open(_d) {
        let _ = f.sync_all();
    }
}

fn copy_err(e: impl std::fmt::Display) -> ApiError {
    ApiError::new("copy_failed", format!("Copy to the drive failed: {e}"))
}

pub fn write_verified(dir: &Path, name: &str, bytes: &[u8]) -> ApiResult<()> {
    let dest = dir.join(name);
    let part = dir.join(format!("{name}.part"));
    let r = (|| {
        let mut f = std::fs::File::create(&part)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&part, &dest)?;
        sync_dir(dir);
        Ok::<_, std::io::Error>(())
    })();
    if let Err(e) = r {
        let _ = std::fs::remove_file(&part);
        return Err(copy_err(e));
    }
    let back = std::fs::read(&dest).map_err(copy_err)?;
    if sha(&back) != sha(bytes) {
        return Err(ApiError::new("copy_failed", format!("{name}: the copy on the drive differs from the original")));
    }
    Ok(())
}

pub(crate) fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let m = std::fs::symlink_metadata(src)?;
    if m.is_dir() {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)?.flatten() {
            copy_tree(&e.path(), &dst.join(e.file_name()))?;
        }
    } else if m.is_file() {
        std::fs::copy(src, dst)?;
        std::fs::File::open(dst)?.sync_all()?;
    }
    Ok(())
}

pub(crate) fn same_tree(a: &Path, b: &Path) -> bool {
    match std::fs::symlink_metadata(a) {
        Ok(m) if m.is_dir() => std::fs::read_dir(a).is_ok_and(|rd| rd.flatten().all(|e| same_tree(&e.path(), &b.join(e.file_name())))),
        Ok(m) if m.is_file() => matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if sha(&x) == sha(&y)),
        _ => true,
    }
}

pub(crate) fn bundle_matches_zip(zip_bytes: &[u8], app: &Path) -> bool {
    use std::io::Read;
    let Ok(mut z) = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)) else { return false };
    for i in 0..z.len() {
        let Ok(mut e) = z.by_index(i) else { return false };
        if e.is_dir() || e.is_symlink() {
            continue;
        }
        let Some(name) = e.enclosed_name() else { return false };
        let rel: PathBuf = name.components().skip(1).collect();
        let mut want = vec![];
        if e.read_to_end(&mut want).is_err() || std::fs::read(app.join(rel)).map(|b| sha(&b)).ok() != Some(sha(&want)) {
            return false;
        }
    }
    true
}

pub fn remove_any(p: &Path) {
    if std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()) {
        let _ = std::fs::remove_dir_all(p);
    } else {
        let _ = std::fs::remove_file(p);
    }
}

pub const SLACK: u64 = 1 << 20;

pub struct Room<'a> {
    pub free: &'a dyn Fn() -> Option<u64>,
    pub running: &'a Path,
}

impl Room<'_> {
    pub fn free(&self) -> Option<u64> {
        (self.free)()
    }
    pub fn fits(&self, n: u64) -> bool {
        self.free().is_none_or(|f| f >= n + SLACK)
    }
    pub fn is_running(&self, p: &Path) -> bool {
        let canon = |x: &Path| std::fs::canonicalize(x).unwrap_or_else(|_| x.to_path_buf());
        !self.running.as_os_str().is_empty() && canon(self.running).starts_with(canon(p))
    }
}

pub fn mac_record(zip_bytes: Option<&[u8]>, app: &Path) -> Option<super::MacRecord> {
    Some(super::MacRecord { zip_sha256: zip_bytes.map(|z| hex(&sha(z))), tree_sha256: tree_digest(app)? })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn tree_digest(dir: &Path) -> Option<String> {
    fn walk(base: &Path, p: &Path, out: &mut Vec<(String, [u8; 32])>) -> Option<()> {
        for e in std::fs::read_dir(p).ok()?.flatten() {
            let path = e.path();
            let m = std::fs::symlink_metadata(&path).ok()?;
            if m.is_dir() {
                walk(base, &path, out)?;
            } else if m.is_file() {
                let rel = path.strip_prefix(base).ok()?.to_string_lossy().replace('\\', "/");
                out.push((rel, sha(&std::fs::read(&path).ok()?)));
            }
        }
        Some(())
    }
    if !std::fs::symlink_metadata(dir).ok()?.is_dir() {
        return None;
    }
    let mut files = vec![];
    walk(dir, dir, &mut files)?;
    files.sort();
    let mut h = Sha256::new();
    for (rel, d) in files {
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(hex(&d).as_bytes());
        h.update(b"\n");
    }
    Some(hex(&h.finalize()))
}

pub fn zip_unpacked_size(zip_bytes: &[u8]) -> u64 {
    let Ok(mut z) = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)) else { return 0 };
    (0..z.len()).filter_map(|i| z.by_index(i).ok().filter(|e| e.is_file()).map(|e| e.size())).sum()
}

pub fn new_size(b: &Builds, os: Os) -> u64 {
    new_size_of(os, b.files.iter().find(|f| f.os == os), (os == Os::Macos).then_some(b.mac_bundle.as_ref().map(|m| m.dir.as_path())).flatten())
}

pub fn new_size_of(os: Os, file: Option<&Build>, bundle: Option<&Path>) -> u64 {
    match file {
        Some(f) if os == Os::Macos => zip_unpacked_size(&f.bytes),
        Some(f) => f.bytes.len() as u64 + f.sig.as_ref().map_or(0, |s| s.len() as u64),
        None => bundle.map_or(0, dir_size),
    }
}

pub const MAC_LEGACY: [&str; 2] = [MAC_ZIP, "NiceVault-macos-universal.zip.sig"];

fn write_mac(dir: &Path, b: &Builds, room: &Room) -> ApiResult<Option<super::MacRecord>> {
    let app = dir.join(MAC_APP);
    let part = dir.join(format!("{MAC_APP}.part"));
    let zip = b.files.iter().find(|f| f.os == Os::Macos);
    if zip.is_none() && b.mac_bundle.is_none() {
        return Ok(None);
    }
    remove_any(&part);
    for l in MAC_LEGACY {
        remove_any(&dir.join(l));
    }
    if !room.fits(new_size(b, Os::Macos)) && !room.is_running(&app) {
        remove_any(&app);
    }
    let fill = || match (zip, &b.mac_bundle) {
        (Some(z), _) => updater::extract_bundle(&z.bytes, &part).map_err(copy_err),
        (None, Some(bundle)) => copy_tree(&bundle.dir, &part).map_err(copy_err),
        _ => Ok(()),
    };
    if let Err(e) = fill() {
        remove_any(&part);
        return Err(e);
    }
    remove_any(&app);
    std::fs::rename(&part, &app).map_err(copy_err)?;
    sync_dir(dir);
    let ok = match (zip, &b.mac_bundle) {
        (Some(z), _) => bundle_matches_zip(&z.bytes, &app),
        (None, Some(bundle)) => same_tree(&bundle.dir, &app),
        _ => true,
    };
    if !ok {
        return Err(ApiError::new("copy_failed", "NiceVault.app on the drive differs from the original"));
    }
    Ok(mac_record(zip.map(|z| z.bytes.as_slice()), &app))
}

pub fn write_client(dir: &Path, b: &Builds, data_fs: DataFs, room: &Room, progress: &mut dyn FnMut(u64, u64)) -> ApiResult<Option<super::MacRecord>> {
    let rec = write_builds(dir, b, room, progress)?;
    write_readmes(dir, data_fs, &b.missing())?;
    Ok(rec)
}

pub fn write_builds(dir: &Path, b: &Builds, room: &Room, progress: &mut dyn FnMut(u64, u64)) -> ApiResult<Option<super::MacRecord>> {
    let total = b.total_bytes().max(1);
    let mut done = 0;
    progress(0, total);
    for f in b.files.iter().filter(|f| f.os != Os::Macos) {
        let dest = dir.join(&f.name);
        remove_any(&dir.join(format!("{}.part", f.name)));
        if !room.fits(f.bytes.len() as u64) && !room.is_running(&dest) {
            remove_any(&dest);
        }
        if !room.fits(f.bytes.len() as u64) {
            return Err(ApiError::new("stick_full", format!("Not enough room on the NiceVault partition for {} ({} bytes needed, {} free).", f.name, f.bytes.len() as u64 + SLACK, room.free().unwrap_or(0))));
        }
        write_verified(dir, &f.name, &f.bytes)?;
        match &f.sig {
            Some(s) => write_verified(dir, &format!("{}.sig", f.name), s.as_bytes())?,
            None => {
                let _ = std::fs::remove_file(dir.join(format!("{}.sig", f.name)));
            }
        }
        #[cfg(unix)]
        if f.os == Os::Linux {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir.join(&f.name), std::fs::Permissions::from_mode(0o755));
        }
        done += f.bytes.len() as u64;
        progress(done, total);
    }
    let rec = write_mac(dir, b, room)?;
    progress(total, total);
    Ok(rec)
}

pub fn write_readmes(dir: &Path, data_fs: DataFs, missing: &[Os]) -> ApiResult<()> {
    write_verified(dir, "LEGGIMI.txt", readme(true, data_fs, missing).as_bytes())?;
    write_verified(dir, "README.txt", readme(false, data_fs, missing).as_bytes())
}

pub fn marker_missing(m: &Marker) -> Vec<Os> {
    Os::ALL.into_iter().filter(|o| m.clients.get(os_key(*o)).cloned().flatten().is_none()).collect()
}

pub fn os_key(o: Os) -> &'static str {
    match o {
        Os::Linux => "linux",
        Os::Windows => "windows",
        Os::Macos => "macos",
    }
}

fn os_name(o: Os) -> &'static str {
    match o {
        Os::Linux => "Linux",
        Os::Windows => "Windows",
        Os::Macos => "macOS",
    }
}

pub fn readme(it: bool, fs: DataFs, missing: &[Os]) -> String {
    let t = if it { include_str!("LEGGIMI.txt") } else { include_str!("README.txt") };
    let fs_name = match fs {
        DataFs::Exfat => "exFAT",
        DataFs::Ntfs => "NTFS",
        DataFs::Ext4 => "ext4",
        DataFs::Btrfs => "btrfs",
        DataFs::Apfs => "APFS",
    };
    let read: Vec<String> = fs
        .readable_on()
        .iter()
        .filter(|(_, a)| *a != super::Access::No)
        .map(|(o, a)| {
            let ro = if *a == super::Access::Ro { if it { " (solo lettura)" } else { " (read only)" } } else { "" };
            format!("{}{ro}", os_name(*o))
        })
        .collect();
    let missing_line = if missing.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = missing.iter().map(|o| os_name(*o)).collect();
        if it {
            format!("\nMancano i programmi per: {}. Per aggiungerli apri NiceVault su un computer in rete, collega questo dispositivo e alla fine di \"Crea dispositivo\" premi \"Scarica ora\" (o crea di nuovo il dispositivo).\n", names.join(", "))
        } else {
            format!("\nThe programs for {} are missing. To add them, open NiceVault on a computer that is online, plug in this device and at the end of \"Create device\" press \"Download now\" (or create the device again).\n", names.join(", "))
        }
    };
    t.replace("{fs}", fs_name).replace("{readable}", &read.join(", ")).replace("{missing}", &missing_line).replace("\n", "\r\n")
}

pub fn marker(plan: &Plan, b: &Builds) -> Marker {
    let mut id = [0u8; 16];
    getrandom::fill(&mut id).expect("OS random");
    let mut clients = BTreeMap::new();
    for o in Os::ALL {
        let v = b.files.iter().find(|f| f.os == o).and_then(|f| f.version.clone()).or_else(|| (o == Os::Macos && b.mac_bundle.is_some()).then(|| env!("CARGO_PKG_VERSION").to_owned()));
        clients.insert(os_key(o).to_owned(), v);
    }
    Marker {
        format: 1,
        created: updater::now_ms(),
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        data_fs: plan.data_fs.id().to_owned(),
        device_id: id.iter().map(|x| format!("{x:02x}")).collect(),
        table: Some(plan.table),
        clients,
        updated: None,
        mac_app: None,
    }
}

pub fn write_marker(dir: &Path, m: &Marker) -> ApiResult<()> {
    write_verified(dir, super::MARKER, &serde_json::to_vec_pretty(m).unwrap())
}

pub fn read_marker(dir: &Path) -> Option<Marker> {
    serde_json::from_slice(&std::fs::read(dir.join(super::MARKER)).ok()?).ok()
}
