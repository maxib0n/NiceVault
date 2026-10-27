use super::backend::DiskOps;
use super::client::{self, Build, Builds, Local, Room};
use super::job::{DeviceCtx, Log};
use super::{CLIENT_LABEL, DATA_LABEL, DataFs, Disk, MARKER, Marker, Os};
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, Job, lock_or_recover, random_id};
use crate::updater;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub fn program_name(os: Os) -> &'static str {
    match os {
        Os::Linux => client::LINUX_NAME,
        Os::Windows => client::WINDOWS_NAME,
        Os::Macos => client::MAC_APP,
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Running,
    Download,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct StickOs {
    pub os: Os,
    pub file: String,
    pub present: bool,
    pub version: Option<String>,
    pub target: Option<String>,
    pub source: Source,
    pub current: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Stick {
    pub disk_id: String,
    pub name: String,
    pub client_mount: Option<String>,
    pub mounted: bool,
    pub complete: bool,
    pub data_fs: Option<String>,
    pub running_version: String,
    pub programs: Vec<StickOs>,
    pub outdated: bool,
    pub runs_from_stick: bool,
    pub space: Option<Space>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Safe,
    Tight,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct OsSpace {
    pub os: Os,
    pub now: u64,
    pub next: u64,
    pub mode: Option<Mode>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Space {
    pub size: Option<u64>,
    pub free: Option<u64>,
    pub reclaim: u64,
    pub programs: Vec<OsSpace>,
    pub tight: bool,
    pub fits: bool,
}

fn estimate(os: Os) -> u64 {
    match os {
        Os::Linux => 30 << 20,
        Os::Windows => 26 << 20,
        Os::Macos => 42 << 20,
    }
}

fn program_files(os: Os) -> Vec<String> {
    let p = program_name(os);
    match os {
        Os::Macos => std::iter::once(p.to_owned()).chain(client::MAC_LEGACY.iter().map(|s| s.to_string())).collect(),
        _ => vec![p.to_owned(), format!("{p}.sig")],
    }
}

fn on_stick(root: &Path, os: Os) -> u64 {
    program_files(os).iter().flat_map(|n| [n.clone(), format!("{n}.part"), format!("{n}.old")]).map(|n| client::dir_size(&root.join(n))).sum()
}

fn reclaimable(root: &Path) -> u64 {
    let mut n = 0;
    for os in Os::ALL {
        for f in program_files(os) {
            for x in [format!("{f}.part"), format!("{f}.old")] {
                n += client::dir_size(&root.join(x));
            }
        }
    }
    if root.join(client::MAC_APP).is_dir() {
        n += client::MAC_LEGACY.iter().map(|f| client::dir_size(&root.join(f))).sum::<u64>();
    }
    n
}

pub fn plan_space(free: Option<u64>, items: &[(Os, u64, u64)]) -> Vec<OsSpace> {
    let mut v: Vec<(Os, u64, u64)> = items.to_vec();
    v.sort_by_key(|i| (i.2, i.0 as u8));
    let mut left = free;
    v.into_iter()
        .map(|(os, now, next)| {
            let mode = match left {
                None => Some(Mode::Safe),
                Some(f) if f >= next + client::SLACK => Some(Mode::Safe),
                Some(f) if f + now >= next + client::SLACK => Some(Mode::Tight),
                Some(_) => None,
            };
            if mode.is_some() {
                left = left.map(|f| (f + now).saturating_sub(next));
            }
            OsSpace { os, now, next, mode }
        })
        .collect()
}

fn space_of(size: Option<u64>, free: Option<u64>, reclaim: u64, programs: Vec<OsSpace>) -> Space {
    Space { size, free, reclaim, tight: programs.iter().any(|p| p.mode == Some(Mode::Tight)), fits: programs.iter().all(|p| p.mode.is_some()), programs }
}

fn newer(current: Option<&str>, candidate: &str) -> bool {
    match current {
        None => true,
        Some(c) => updater::is_newer(c, candidate).unwrap_or(true),
    }
}

pub fn is_device(d: &Disk) -> bool {
    d.by_label(CLIENT_LABEL).is_some() && d.by_label(DATA_LABEL).is_some()
}

fn under(p: &Path, root: &Path) -> bool {
    let canon = |x: &Path| std::fs::canonicalize(x).unwrap_or_else(|_| x.to_path_buf());
    canon(p).starts_with(canon(root))
}

pub fn device_for_path<'a>(disks: &'a [Disk], path: &Path) -> Option<&'a Disk> {
    disks
        .iter()
        .filter(|d| is_device(d))
        .filter_map(|d| {
            let best = d.mountpoints().filter(|m| !m.is_empty() && under(path, Path::new(m))).map(str::len).max()?;
            Some((best, d))
        })
        .max_by_key(|(n, _)| *n)
        .map(|(_, d)| d)
}

pub fn status(d: &Disk, client_root: Option<&Path>, l: &Local, free: Option<u64>) -> Stick {
    let marker = client_root.and_then(client::read_marker);
    let runs_from_stick = client_root.is_some_and(|r| under(&l.exe, r));
    let programs = Os::ALL
        .into_iter()
        .map(|os| {
            let file = program_name(os);
            let present = client_root.is_some_and(|r| std::fs::symlink_metadata(r.join(file)).is_ok());
            let version = present.then(|| marker.as_ref().and_then(|m| m.clients.get(client::os_key(os)).cloned().flatten())).flatten();
            let own = os == l.os;
            let current = if own && runs_from_stick { true } else { present && !newer(version.as_deref(), &l.version) };
            StickOs {
                os,
                file: file.to_owned(),
                present,
                version: if own && runs_from_stick { version.or_else(|| Some(l.version.clone())) } else { version },
                target: (own && runs_from_stick).then(|| l.version.clone()),
                source: if own && runs_from_stick { Source::Running } else { Source::Download },
                current,
            }
        })
        .collect::<Vec<_>>();
    let mounted = client_root.is_some();
    let space = client_root.filter(|_| marker.is_some()).map(|root| {
        let reclaim = reclaimable(root);
        let items: Vec<(Os, u64, u64)> = programs
            .iter()
            .filter(|p| !p.current)
            .map(|p| {
                let now = on_stick(root, p.os);
                let own = p.os == l.os && matches!(p.os, Os::Linux | Os::Windows);
                let next = if own { std::fs::metadata(&l.exe).map_or(0, |m| m.len()) } else if p.present { client::dir_size(&root.join(program_name(p.os))) } else { 0 };
                (p.os, now, if next > 0 { next } else { estimate(p.os) })
            })
            .collect();
        let size = d.by_label(CLIENT_LABEL).map(|p| p.size).filter(|s| *s > 0);
        space_of(size, free, reclaim, plan_space(free.map(|f| f + reclaim), &items))
    });
    Stick {
        space,
        disk_id: d.id.clone(),
        name: d.name(),
        client_mount: client_root.map(|r| r.to_string_lossy().into_owned()),
        mounted,
        complete: marker.is_some(),
        data_fs: marker.as_ref().map(|m| m.data_fs.clone()),
        running_version: l.version.clone(),
        outdated: mounted && marker.is_some() && programs.iter().any(|p| !p.current),
        programs,
        runs_from_stick,
    }
}

pub fn sticks(ops: &dyn DiskOps, l: &Local, path: Option<&Path>) -> Vec<Stick> {
    let disks = ops.disks();
    let chosen: Vec<&Disk> = match path {
        Some(p) => device_for_path(&disks, p).into_iter().collect(),
        None => disks.iter().filter(|d| is_device(d)).collect(),
    };
    chosen
        .into_iter()
        .map(|d| {
            let root = ops.client_root(d);
            let free = root.as_deref().and_then(|r| ops.client_free(d, r));
            status(d, root.as_deref(), l, free)
        })
        .collect()
}

fn find_device(ops: &dyn DiskOps, id: &str) -> ApiResult<Disk> {
    let d = ops.disks().into_iter().find(|d| d.id == id).ok_or_else(|| ApiError::new("device_not_found", "The drive is no longer connected."))?;
    if !is_device(&d) {
        return Err(ApiError::new("stick_not_device", "This drive is not a NiceVault device."));
    }
    Ok(d)
}

pub fn mount(ctx: &DeviceCtx, id: &str) -> ApiResult<Stick> {
    let d = find_device(ctx.ops.as_ref(), id)?;
    if ctx.ops.client_root(&d).is_none() {
        ctx.ops.mount_client(&d, &Log::open(&ctx.log_dir))?;
    }
    let d = find_device(ctx.ops.as_ref(), id)?;
    Ok(fresh(ctx.ops.as_ref(), &d, &ctx.local))
}

fn fresh(ops: &dyn DiskOps, d: &Disk, l: &Local) -> Stick {
    let root = ops.client_root(d);
    let free = root.as_deref().and_then(|r| ops.client_free(d, r));
    status(d, root.as_deref(), l, free)
}

pub type Downloaded = (Vec<Build>, Vec<(Os, ApiError)>);

pub fn download_signed_builds(
    l: &Local,
    manifest_url: &str,
    want: &[(Os, Option<String>)],
    progress: &mut dyn FnMut(u64, u64),
) -> ApiResult<Downloaded> {
    let m = updater::fetch_manifest(manifest_url)?;
    let need: Vec<Os> = want.iter().filter(|(_, have)| newer(have.as_deref(), &m.version)).map(|(o, _)| *o).collect();
    if need.is_empty() {
        return Ok((vec![], vec![]));
    }
    let got = client::download(l, manifest_url, &need, progress);
    let failed = need
        .iter()
        .filter(|o| !got.has(**o))
        .map(|o| {
            let key = client::BUILDS.iter().find(|b| b.0 == *o).map_or("", |b| b.1);
            let why = got.notes.iter().find(|n| n.starts_with(&format!("{key}:"))).or(got.notes.first()).cloned().unwrap_or_default();
            (*o, ApiError::new("device_download", why))
        })
        .collect();
    Ok((got.files, failed))
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

use client::remove_any;

fn copy_err(name: &str, e: impl std::fmt::Display) -> ApiError {
    ApiError::new("copy_failed", format!("{name}: {e}"))
}

fn no_space(name: &str) -> ApiError {
    ApiError::new("stick_full", format!("Not enough room on the NiceVault partition for {name}."))
}

fn mib(n: u64) -> String {
    format!("{:.1} MB", n as f64 / 1_000_000.0)
}

fn free_bytes(dir: &Path) -> Option<u64> {
    let f = super::job::free_space(dir);
    (f > 0).then_some(f)
}

fn swap_checked(dir: &Path, name: &str, staged: &Path, check: impl Fn(&Path) -> bool) -> ApiResult<()> {
    let dest = dir.join(name);
    let old = dir.join(format!("{name}.old"));
    remove_any(&old);
    let had = std::fs::symlink_metadata(&dest).is_ok();
    if had {
        std::fs::rename(&dest, &old).map_err(|e| {
            remove_any(staged);
            copy_err(name, e)
        })?;
    }
    if let Err(e) = std::fs::rename(staged, &dest) {
        remove_any(staged);
        if had {
            let _ = std::fs::rename(&old, &dest);
        }
        return Err(copy_err(name, e));
    }
    sync_dir(dir);
    if !check(&dest) {
        remove_any(&dest);
        if had {
            let _ = std::fs::rename(&old, &dest);
        }
        sync_dir(dir);
        return Err(ApiError::new("copy_failed", format!("{name}: the copy on the drive differs from the original")));
    }
    remove_any(&old);
    sync_dir(dir);
    Ok(())
}

pub fn replace_file(dir: &Path, name: &str, bytes: &[u8]) -> ApiResult<()> {
    if free_bytes(dir).is_some_and(|f| f < bytes.len() as u64 + (256 << 10)) {
        return Err(no_space(name));
    }
    let part = write_part(dir, name, bytes)?;
    let want = sha(bytes);
    swap_checked(dir, name, &part, |d| std::fs::read(d).is_ok_and(|b| sha(&b) == want))
}

fn write_part(dir: &Path, name: &str, bytes: &[u8]) -> ApiResult<std::path::PathBuf> {
    let part = dir.join(format!("{name}.part"));
    let r = (|| {
        let mut f = std::fs::File::create(&part)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        Ok::<_, std::io::Error>(())
    })();
    if let Err(e) = r {
        remove_any(&part);
        return Err(if e.raw_os_error() == Some(28) { no_space(name) } else { copy_err(name, e) });
    }
    #[cfg(unix)]
    if name == client::LINUX_NAME {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755));
    }
    Ok(part)
}

fn replace_sig(dir: &Path, name: &str, sig: Option<&str>) -> ApiResult<()> {
    let file = format!("{name}.sig");
    match sig {
        Some(s) => replace_file(dir, &file, s.as_bytes()),
        None => {
            let _ = std::fs::remove_file(dir.join(file));
            Ok(())
        }
    }
}

struct Src<'a> {
    os: Os,
    file: Option<&'a Build>,
    bundle: Option<&'a Path>,
}

fn install(root: &Path, src: &Src, room: &Room, mode: Mode, fault: &dyn Fn() -> Option<ApiError>) -> ApiResult<Option<super::MacRecord>> {
    let name = program_name(src.os);
    let part = root.join(format!("{name}.part"));
    remove_any(&part);
    let staged = (|| {
        if !room.fits(client::new_size_of(src.os, src.file, src.bundle)) {
            return Err(no_space(name));
        }
        match (src.os, src.file, src.bundle) {
            (Os::Macos, Some(z), _) => updater::extract_bundle(&z.bytes, &part).map_err(|e| copy_err(name, e)),
            (Os::Macos, None, Some(b)) => client::copy_tree(b, &part).map_err(|e| copy_err(name, e)),
            (_, Some(f), _) => write_part(root, name, &f.bytes).map(|_| ()),
            _ => Ok(()),
        }?;
        if room.free() == Some(0) {
            return Err(no_space(name));
        }
        fault().map_or(Ok(()), Err)
    })();
    if let Err(e) = staged {
        remove_any(&part);
        return Err(e);
    }
    let check = |p: &Path| match (src.os, src.file, src.bundle) {
        (Os::Macos, Some(z), _) => client::bundle_matches_zip(&z.bytes, p),
        (Os::Macos, None, Some(b)) => client::same_tree(b, p),
        (_, Some(f), _) => std::fs::read(p).is_ok_and(|b| sha(&b) == sha(&f.bytes)),
        _ => false,
    };
    match mode {
        Mode::Safe => swap_checked(root, name, &part, check)?,
        Mode::Tight => {
            let dest = root.join(name);
            if let Err(e) = std::fs::rename(&part, &dest) {
                remove_any(&part);
                return Err(copy_err(name, e));
            }
            sync_dir(root);
            if !check(&dest) {
                remove_any(&dest);
                sync_dir(root);
                return Err(ApiError::new("copy_failed", format!("{name}: the copy on the drive differs from the original")));
            }
        }
    }
    if src.os == Os::Macos {
        for l in client::MAC_LEGACY {
            remove_any(&root.join(l));
        }
        sync_dir(root);
        return Ok(client::mac_record(src.file.map(|z| z.bytes.as_slice()), &root.join(name)));
    }
    replace_sig(root, name, src.file.and_then(|f| f.sig.as_deref()))?;
    Ok(None)
}

pub fn tidy(root: &Path, marker: &mut Marker, room: &Room) -> u64 {
    let mut freed = 0;
    let mut gone = |p: &Path| {
        if std::fs::symlink_metadata(p).is_ok() && !room.is_running(p) {
            freed += client::dir_size(p);
            remove_any(p);
        }
    };
    for os in Os::ALL {
        for f in program_files(os) {
            let (cur, old) = (root.join(&f), root.join(format!("{f}.old")));
            if std::fs::symlink_metadata(&cur).is_err() && std::fs::symlink_metadata(&old).is_ok() {
                let _ = std::fs::rename(&old, &cur);
            }
            gone(&root.join(format!("{f}.part")));
            gone(&old);
        }
    }
    let app = root.join(client::MAC_APP);
    let zip = root.join(client::MAC_ZIP);
    if app.is_dir() {
        if let Ok(z) = std::fs::read(&zip) {
            if client::bundle_matches_zip(&z, &app) {
                marker.mac_app = client::mac_record(Some(&z), &app);
                for l in client::MAC_LEGACY {
                    gone(&root.join(l));
                }
            }
        } else if let Some(rec) = &marker.mac_app
            && client::tree_digest(&app).as_deref() != Some(rec.tree_sha256.as_str())
            && !room.is_running(&app)
        {
            set_client(marker, Os::Macos, None);
        }
    }
    sync_dir(root);
    freed
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Updated,
    Unchanged,
    Offline,
    Failed,
    Rewrite,
    InUse,
}

#[derive(Clone, Debug, Serialize)]
pub struct OsResult {
    pub os: Os,
    pub from: Option<String>,
    pub to: Option<String>,
    pub result: Change,
    pub error: Option<ApiError>,
    pub mode: Option<Mode>,
}

#[derive(Clone, Debug, Serialize)]
pub struct UpdateOutcome {
    pub results: Vec<OsResult>,
    pub stick: Stick,
    pub space: Option<Space>,
}

fn busy() -> &'static Mutex<HashSet<String>> {
    static B: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    B.get_or_init(Mutex::default)
}

struct Emit<'a> {
    st: &'a AppState,
    job: &'a str,
    last: Instant,
}

impl Emit<'_> {
    fn progress(&mut self, step: &str, os: Option<Os>, done: u64, total: u64, force: bool) {
        if force || self.last.elapsed() >= Duration::from_millis(100) {
            self.last = Instant::now();
            self.st.emit("job-progress", json!({ "job": self.job, "step": step, "os": os, "done_bytes": done, "total_bytes": total, "current": step }));
        }
    }
}

fn res(os: Os, from: Option<String>, to: Option<String>, result: Change, error: Option<ApiError>, mode: Option<Mode>) -> OsResult {
    OsResult { os, from, to, result, error, mode }
}

pub fn execute(ctx: &DeviceCtx, st: &AppState, job: &str, cancel: &AtomicBool, disk_id: &str, log: &Log) -> ApiResult<UpdateOutcome> {
    let mut em = Emit { st, job, last: Instant::now() };
    let cancelled = || if cancel.load(Ordering::Relaxed) { Err(ApiError::cancelled()) } else { Ok(()) };
    let ops = ctx.ops.as_ref();
    let l = &ctx.local;

    em.progress("check", None, 0, 0, true);
    let d = find_device(ops, disk_id)?;
    let root = ops.client_root(&d).ok_or_else(|| ApiError::new("stick_not_mounted", "The NiceVault partition is not mounted."))?;
    let before = fresh(ops, &d, l);
    if !before.complete {
        return Err(ApiError::new("stick_not_device", "The NiceVault partition has no marker: finish it with Crea dispositivo > Completa."));
    }
    if let Some(data) = d.by_label(DATA_LABEL)
        && data.mountpoints.iter().any(|m| !m.is_empty() && (under(&root, Path::new(m)) || under(Path::new(m), &root)))
    {
        return Err(ApiError::new("stick_not_device", "The NiceVault and VaultData partitions overlap."));
    }
    let mut marker: Marker = client::read_marker(&root).ok_or_else(|| ApiError::new("stick_not_device", "Unreadable marker."))?;
    log.event("stick_update", json!({ "disk": d.id, "client": root.to_string_lossy(), "before": before }));
    let had = |os: Os| before.programs.iter().find(|p| p.os == os).and_then(|p| p.version.clone().filter(|_| p.present));
    cancelled()?;

    let mut builds = Builds::default();

    let free = || ops.client_free(&d, &root);
    let room = Room { free: &free, running: &l.exe };
    let mut others: Vec<(Os, Option<String>)> = Os::ALL.into_iter().filter(|o| !(*o == l.os && before.runs_from_stick)).map(|o| (o, had(o))).collect();
    if app_changed(&root, &marker) {
        for o in others.iter_mut().filter(|o| o.0 == Os::Macos) {
            o.1 = None;
        }
    }
    em.progress("download", None, 0, 0, true);
    let mut offline: Option<ApiError> = None;
    let mut failed: Vec<(Os, ApiError)> = vec![];
    match download_signed_builds(l, &ctx.manifest_url, &others, &mut |d, t| em.progress("download", None, d, t, false)) {
        Ok((files, f)) => {
            builds.files.extend(files);
            failed = f;
        }
        Err(e) => offline = Some(e),
    }
    if !before.runs_from_stick && !builds.has(l.os) && offline.is_some() {
        let local = client::local_builds(l, &[l.os]);
        builds.files.extend(local.files);
        builds.mac_bundle = local.mac_bundle;
    }
    log.event("builds", json!({ "offline": offline, "failed": failed.iter().map(|(o, e)| json!({"os": o, "error": e})).collect::<Vec<_>>(), "files": builds.files.iter().map(|f| json!({"name": f.name, "origin": f.origin, "version": f.version, "bytes": f.bytes.len(), "signed": f.sig.is_some()})).collect::<Vec<_>>() }));
    cancelled()?;

    let freed = tidy(&root, &mut marker, &room);
    let writes: Vec<(Os, u64, u64)> = Os::ALL.into_iter().filter(|o| builds.has(*o)).map(|o| (o, on_stick(&root, o), client::new_size(&builds, o))).collect();
    let size = d.by_label(CLIENT_LABEL).map(|p| p.size).filter(|s| *s > 0);
    let space = space_of(size, room.free(), 0, plan_space(room.free(), &writes));
    log.event("space", json!({ "freed": freed, "space": space }));
    st.emit("job-progress", json!({ "job": job, "step": "space", "os": null, "done_bytes": 0, "total_bytes": 0, "current": "space", "space": space }));

    let total = builds.total_bytes().max(1);
    let mut done = 0u64;
    let mut results: Vec<OsResult> = vec![];
    let order: Vec<Os> = space.programs.iter().map(|p| p.os).chain(Os::ALL.into_iter().filter(|o| !builds.has(*o))).collect();
    for os in order {
        let from = had(os);
        let file = builds.files.iter().find(|f| f.os == os);
        let bundle = (os == Os::Macos).then(|| builds.mac_bundle.as_ref().map(|b| b.dir.as_path())).flatten();
        if file.is_none() && bundle.is_none() {
            let (result, error) = if let Some((_, e)) = failed.iter().find(|(o, _)| *o == os) {
                (Change::Failed, Some(e.clone()))
            } else if os == l.os && before.runs_from_stick {
                (Change::InUse, None)
            } else if offline.is_some() {
                (Change::Offline, offline.clone())
            } else {
                (Change::Unchanged, None)
            };
            results.push(res(os, from.clone(), from, result, error, None));
            continue;
        }
        let to = file.and_then(|f| f.version.clone()).or_else(|| bundle.is_some().then(|| l.version.clone()));
        if let Some(f) = file
            && os != Os::Macos
            && std::fs::read(root.join(&f.name)).is_ok_and(|b| sha(&b) == sha(&f.bytes))
        {
            results.push(res(os, from.clone(), to.clone(), Change::Unchanged, None, None));
            set_client(&mut marker, os, to);
            continue;
        }
        let prog = root.join(program_name(os));
        if room.is_running(&prog) {
            results.push(res(os, from.clone(), from, Change::InUse, None, None));
            continue;
        }
        cancelled()?;
        let (now, next) = (on_stick(&root, os), client::new_size(&builds, os));
        let mode = match room.free() {
            None => Mode::Safe,
            Some(f) if f >= next + client::SLACK => Mode::Safe,
            Some(f) if f + now >= next + client::SLACK => Mode::Tight,
            Some(f) => {
                let e = ApiError::new("stick_full", format!("{}: {} needed, {} free even without the current copy.", program_name(os), mib(next + client::SLACK), mib(f + now)));
                log.event("error", json!({ "os": os, "code": e.code, "message": e.message }));
                results.push(res(os, from.clone(), from, Change::Failed, Some(e), None));
                continue;
            }
        };
        em.progress("copy", Some(os), done, total, true);
        if mode == Mode::Tight {
            set_client(&mut marker, os, None);
            if os == Os::Macos {
                marker.mac_app = None;
            }
            client::write_marker(&root, &marker)?;
            for f in program_files(os) {
                remove_any(&root.join(f));
            }
            sync_dir(&root);
            log.event("tight", json!({ "os": os, "removed": now, "next": next, "free": room.free() }));
        }
        let key = format!("write_{}", client::os_key(os));
        let fault = || ops.fault(&key);
        match install(&root, &Src { os, file, bundle }, &room, mode, &fault) {
            Ok(rec) => {
                if os == Os::Macos {
                    marker.mac_app = rec;
                }
                done += file.map_or(0, |f| f.bytes.len() as u64);
                em.progress("verify", Some(os), done, total, true);
                set_client(&mut marker, os, to.clone());
                results.push(res(os, from, to, Change::Updated, None, Some(mode)));
            }
            Err(e) => {
                log.event("error", json!({ "os": os, "code": e.code, "message": e.message, "mode": mode }));
                if !root.is_dir() {
                    return Err(ApiError::new("device_gone", "The drive was disconnected."));
                }
                let result = if mode == Mode::Tight { Change::Rewrite } else { Change::Failed };
                let left = if mode == Mode::Tight { None } else { from.clone() };
                results.push(res(os, from, left, result, Some(e), Some(mode)));
            }
        }
    }
    results.sort_by_key(|r| Os::ALL.iter().position(|o| *o == r.os));

    em.progress("marker", None, total, total, true);
    finish(&root, &mut marker)?;
    let d = find_device(ops, disk_id)?;
    let stick = fresh(ops, &d, l);
    log.event("done", json!({ "results": results, "marker": marker }));
    Ok(UpdateOutcome { results, stick, space: Some(space) })
}

fn app_changed(root: &Path, m: &Marker) -> bool {
    let app = root.join(client::MAC_APP);
    app.is_dir() && !root.join(client::MAC_ZIP).is_file() && m.mac_app.as_ref().is_some_and(|r| client::tree_digest(&app).as_deref() != Some(r.tree_sha256.as_str()))
}

fn set_client(m: &mut Marker, os: Os, v: Option<String>) {
    m.clients.insert(client::os_key(os).to_owned(), v);
}

fn finish(root: &Path, marker: &mut Marker) -> ApiResult<()> {
    let missing: Vec<Os> = Os::ALL.into_iter().filter(|o| std::fs::symlink_metadata(root.join(program_name(*o))).is_err()).collect();
    let fs: DataFs = serde_json::from_value(serde_json::Value::String(marker.data_fs.clone())).unwrap_or(DataFs::Exfat);
    replace_file(root, "LEGGIMI.txt", client::readme(true, fs, &missing).as_bytes())?;
    replace_file(root, "README.txt", client::readme(false, fs, &missing).as_bytes())?;
    for o in &missing {
        set_client(marker, *o, None);
    }
    marker.updated = Some(updater::now_ms());
    replace_file(root, MARKER, &serde_json::to_vec_pretty(marker).unwrap())
}

pub fn start_update(ctx: &Arc<DeviceCtx>, st: &AppState, disk_id: &str) -> ApiResult<String> {
    let d = find_device(ctx.ops.as_ref(), disk_id)?;
    if ctx.ops.client_root(&d).is_none() {
        return Err(ApiError::new("stick_not_mounted", "The NiceVault partition is not mounted."));
    }
    if !lock_or_recover(busy()).insert(disk_id.to_owned()) {
        return Err(ApiError::new("busy", "The programs on this drive are already being updated."));
    }
    let job = random_id();
    let cancel = Arc::new(AtomicBool::new(false));
    lock_or_recover(&st.jobs).insert(job.clone(), Job { vault: String::new(), cancel: cancel.clone() });
    let (ctx, st, id, disk) = (ctx.clone(), st.arc(), job.clone(), disk_id.to_owned());
    let spawned = std::thread::Builder::new().name("nv-stick".into()).spawn(move || {
        let log = Log::open(&ctx.log_dir);
        lock_or_recover(&ctx.logs).insert(id.clone(), log.path().to_path_buf());
        let r = execute(&ctx, &st, &id, &cancel, &disk, &log);
        if let Err(e) = &r {
            log.event("error", json!({ "code": e.code, "message": e.message }));
        }
        lock_or_recover(&st.jobs).remove(&id);
        lock_or_recover(busy()).remove(&disk);
        st.emit("job-done", json!({ "job": id, "error": r.as_ref().err(), "outcome": r.as_ref().ok(), "log": log.path().to_string_lossy() }));
    });
    if spawned.is_err() {
        lock_or_recover(busy()).remove(disk_id);
        return Err(ApiError::new("error", "Could not start the update."));
    }
    Ok(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_check_puts_the_old_file_back() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("prog"), b"old").unwrap();
        std::fs::write(t.path().join("prog.part"), b"new").unwrap();
        let e = swap_checked(t.path(), "prog", &t.path().join("prog.part"), |_| false).unwrap_err();
        assert_eq!(e.code, "copy_failed");
        assert_eq!(std::fs::read(t.path().join("prog")).unwrap(), b"old");
        assert!(!t.path().join("prog.old").exists() && !t.path().join("prog.part").exists());
        replace_file(t.path(), "prog", b"new").unwrap();
        assert_eq!(std::fs::read(t.path().join("prog")).unwrap(), b"new");
        assert!(!t.path().join("prog.old").exists());
        replace_file(t.path(), "other", b"x").unwrap();
        assert_eq!(std::fs::read(t.path().join("other")).unwrap(), b"x");
    }

    #[test]
    fn versions() {
        assert!(newer(None, "0.1.0"));
        assert!(newer(Some("0.0.5"), "0.1.0"));
        assert!(!newer(Some("0.1.0"), "0.1.0"));
        assert!(!newer(Some("0.2.0"), "0.1.0"));
        assert!(newer(Some("garbage"), "0.1.0"), "an unreadable version counts as older");
    }
}
