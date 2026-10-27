use super::job::{self, DeviceCtx, Log, Outcome};
use super::{Access, DataFs, Disk, Os, Plan, PlanOpts, Resume, Table};
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, lock_or_recover, random_id};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    #[serde(flatten)]
    pub disk: Disk,
    pub name: String,
    pub serial_tail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FsOption {
    pub id: DataFs,
    pub creatable: bool,
    pub missing_tool: Option<String>,
    pub package: Option<String>,
    pub readable_on: Vec<(Os, Access)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceInfo {
    pub os: Os,
    pub local: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Options {
    pub os: Os,
    pub fs: Vec<FsOption>,
    pub sources: Vec<SourceInfo>,
    pub gpt_required: bool,
    pub default_table: Table,
    pub data_size: u64,
    pub update_host: String,
}

fn part<T>(name: &str, f: impl FnOnce() -> T) -> T {
    let t = std::time::Instant::now();
    let v = f();
    if trace_on() {
        eprintln!("[device]   {name}: {} ms", t.elapsed().as_millis());
    }
    v
}

pub fn candidates(ctx: &DeviceCtx, st: &AppState) -> Vec<Candidate> {
    let disks = part("disks", || ctx.ops.disks());
    let host = part("host_ctx", || ctx.host_ctx(st));
    super::candidates(disks, &host)
        .into_iter()
        .map(|d| Candidate { name: d.name(), serial_tail: d.serial_tail(), disk: d })
        .collect()
}

fn allowed(ctx: &DeviceCtx, st: &AppState, id: &str) -> ApiResult<Disk> {
    let d = part("find", || ctx.find(id))?;
    if let Some(r) = super::refusal(&d, &part("host_ctx", || ctx.host_ctx(st))) {
        return Err(r.error());
    }
    Ok(d)
}

pub fn options(ctx: &DeviceCtx, st: &AppState, id: &str) -> ApiResult<Options> {
    let d = allowed(ctx, st, id)?;
    let os = Os::current();
    let fs = DataFs::ALL
        .into_iter()
        .map(|f| {
            let missing = if f.creatable_on(os) && os == Os::Linux { part("missing_tool", || ctx.ops.missing_tool(f)) } else { None };
            FsOption {
                id: f,
                creatable: f.creatable_on(os) && missing.is_none(),
                package: missing.as_ref().and(f.linux_tool()).map(|t| t.1.to_owned()),
                missing_tool: missing,
                readable_on: f.readable_on().to_vec(),
            }
        })
        .collect();
    let local = part("local_available", || super::client::local_available(&ctx.local, &Os::ALL));
    let host = ctx.manifest_url.split('/').nth(2).unwrap_or_default().to_owned();
    Ok(Options {
        os,
        fs,
        sources: Os::ALL.into_iter().map(|o| SourceInfo { os: o, local: local.contains(&o) }).collect(),
        gpt_required: d.size > super::MBR_MAX,
        default_table: Table::Mbr,
        data_size: d.size.saturating_sub(super::DATA_OFFSET + super::MIB),
        update_host: host,
    })
}

pub fn make_plan(ctx: &DeviceCtx, st: &AppState, opts: &PlanOpts) -> ApiResult<Plan> {
    let d = part("find", || ctx.find(&opts.id))?;
    let os = Os::current();
    if os == Os::Linux
        && let Some(tool) = ctx.ops.missing_tool(opts.data_fs)
    {
        return Err(ApiError::new("fs_unsupported", format!("{}:{tool}", opts.data_fs.id())));
    }
    if os == Os::Linux
        && let Some(tool) = ctx.ops.missing_tool(DataFs::Exfat)
    {
        return Err(ApiError::new("fs_unsupported", format!("exfat:{tool}")));
    }
    let host = part("host_ctx", || ctx.host_ctx(st));
    let mut p = super::plan(&d, opts, os, &host, &random_id())?;
    p.volumes = part("volumes_on", || super::volumes_on(&d, &host.open_volumes));
    lock_or_recover(&ctx.plans).insert(p.id.clone(), p.clone());
    Ok(p)
}

pub fn create(ctx: &Arc<DeviceCtx>, st: &AppState, plan_id: &str, typed: &str) -> ApiResult<String> {
    let plan = lock_or_recover(&ctx.plans).get(plan_id).cloned().ok_or_else(|| ApiError::not_found("Plan"))?;
    if !super::confirmation_ok(&plan, typed) {
        return Err(ApiError::new("confirmation_mismatch", "The name typed does not match the drive."));
    }
    let now = allowed(ctx, st, &plan.disk.id)?;
    if !now.same_as(&plan.disk) {
        return Err(ApiError::new("device_changed", "The drive changed since it was chosen."));
    }
    lock_or_recover(&ctx.plans).remove(plan_id);
    Ok(job::start(ctx.clone(), st.arc(), plan, false))
}

pub fn resume(ctx: &Arc<DeviceCtx>, st: &AppState, id: &str) -> ApiResult<String> {
    let d = allowed(ctx, st, id)?;
    let marker = d.by_label(super::CLIENT_LABEL).and_then(|p| p.mountpoints.first()).map(|m| PathBuf::from(m).join(super::MARKER).is_file());
    let data_fs = d
        .by_label(super::DATA_LABEL)
        .and_then(|p| super::commands::fs_from_os_name(&p.fs))
        .unwrap_or(DataFs::Exfat);
    let action = super::resume_action(&d, marker);
    if action == Resume::Redo {
        return Err(ApiError::new("device_changed", "This drive does not have the NiceVault layout: create the device again."));
    }
    let os = Os::current();
    let opts = PlanOpts { id: d.id.clone(), table: Table::Mbr, data_fs, first_volume: None, download: action == Resume::Complete };
    let mut plan = super::plan(&d, &opts, os, &ctx.host_ctx(st), &random_id())?;
    plan.table_forced = false;
    plan.steps = if action == Resume::Done {
        vec!["check".into(), "mount".into()]
    } else {
        ["check", "download", "mount", "copy", "verify", "marker"].iter().map(|s| s.to_string()).collect()
    };
    if action == Resume::Done {
        let job = random_id();
        let (ctx2, st2, j) = (ctx.clone(), st.arc(), job.clone());
        std::thread::spawn(move || {
            let log = Log::null();
            let r = ctx2.ops.mount(&plan, &log).map(|(c, v)| {
                let missing = super::client::read_marker(&c).map(|m| super::client::marker_missing(&m)).unwrap_or_default();
                job::outcome(&plan, &c, &v, missing, log.path())
            });
            if let Ok(o) = &r {
                lock_or_recover(&ctx2.outcomes).insert(j.clone(), o.clone());
            }
            st2.emit("job-progress", serde_json::json!({ "job": j, "step": "mount", "done_bytes": 0, "total_bytes": 0 }));
            st2.emit("job-done", serde_json::json!({ "job": j, "error": r.as_ref().err(), "outcome": r.ok() }));
        });
        return Ok(job);
    }
    Ok(job::start(ctx.clone(), st.arc(), plan, true))
}

pub fn fetch_missing(ctx: &DeviceCtx, job: &str) -> ApiResult<Outcome> {
    let mut o = outcome(ctx, job)?;
    if o.missing.is_empty() {
        return Ok(o);
    }
    let dir = PathBuf::from(&o.client_mount);
    let gone = || ApiError::new("device_gone", "The drive was disconnected.");
    let mut marker = super::client::read_marker(&dir).ok_or_else(gone)?;
    let got = super::client::download(&ctx.local, &ctx.manifest_url, &o.missing, &mut |_, _| {});
    if got.files.is_empty() {
        return Err(ApiError::new("device_download", format!("The programs could not be downloaded ({}).", got.notes.join("; "))));
    }
    let disk = ctx.ops.disks().into_iter().find(|d| d.id == o.disk_id);
    let free = || match &disk {
        Some(d) => ctx.ops.client_free(d, &dir),
        None => Some(super::job::free_space(&dir)).filter(|f| *f > 0),
    };
    let room = super::client::Room { free: &free, running: &ctx.local.exe };
    super::stick::tidy(&dir, &mut marker, &room);
    let rec = super::client::write_builds(&dir, &got, &room, &mut |_, _| {}).map_err(|e| if dir.is_dir() { e } else { gone() })?;
    if rec.is_some() {
        marker.mac_app = rec;
    }
    for f in &got.files {
        marker.clients.insert(super::client::os_key(f.os).to_owned(), f.version.clone());
    }
    o.missing.retain(|os| !got.has(*os));
    super::client::write_readmes(&dir, o.data_fs.unwrap_or(DataFs::Exfat), &o.missing)?;
    super::client::write_marker(&dir, &marker)?;
    lock_or_recover(&ctx.outcomes).insert(job.to_owned(), o.clone());
    Ok(o)
}

pub fn eject(ctx: &DeviceCtx, id: &str) -> ApiResult<()> {
    let d = ctx.find(id)?;
    if d.is_system {
        return Err(super::Refusal::System.error());
    }
    if !d.is_virtual && !super::is_external(&d) {
        return Err(super::Refusal::Internal.error());
    }
    let log = Log::open(&ctx.log_dir);
    ctx.ops.eject(&d, &log)
}

pub fn log_text(ctx: &DeviceCtx, job: &str) -> ApiResult<String> {
    let p = lock_or_recover(&ctx.logs).get(job).cloned().ok_or_else(|| ApiError::not_found("Log"))?;
    std::fs::read_to_string(p).map_err(ApiError::io)
}

pub fn outcome(ctx: &DeviceCtx, job: &str) -> ApiResult<Outcome> {
    lock_or_recover(&ctx.outcomes).get(job).cloned().ok_or_else(|| ApiError::not_found("Device"))
}

pub fn open_data(ctx: &DeviceCtx, job: &str) -> ApiResult<()> {
    let o = outcome(ctx, job)?;
    open::that_detached(&o.data_mount).map_err(ApiError::io)
}

type Dev<'a> = State<'a, Arc<DeviceCtx>>;
type St<'a> = State<'a, Arc<AppState>>;

fn trace_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NICEVAULT_DEVICE_TRACE").is_ok_and(|v| v == "1"))
}

async fn blocking<T: Send + 'static>(
    name: &'static str,
    dev: &Dev<'_>,
    st: &St<'_>,
    f: impl FnOnce(&Arc<DeviceCtx>, &AppState) -> ApiResult<T> + Send + 'static,
) -> ApiResult<T> {
    let (d, s) = (dev.inner().clone(), st.inner().clone());
    let t0 = std::time::Instant::now();
    tauri::async_runtime::spawn_blocking(move || {
        let t1 = std::time::Instant::now();
        let r = f(&d, &s);
        if trace_on() {
            eprintln!("[device] {name}: waited {} ms, ran {} ms, {}", (t1 - t0).as_millis(), t1.elapsed().as_millis(), if r.is_ok() { "ok" } else { "error" });
        }
        r
    })
    .await
    .map_err(|e| ApiError::new("error", format!("Internal error: {e}")))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn device_candidates(dev: Dev<'_>, st: St<'_>) -> ApiResult<Vec<Candidate>> {
    blocking("device_candidates", &dev, &st, |d, s| Ok(candidates(d, s))).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn device_options(dev: Dev<'_>, st: St<'_>, id: String) -> ApiResult<Options> {
    blocking("device_options", &dev, &st, move |d, s| options(d, s, &id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn device_plan(dev: Dev<'_>, st: St<'_>, opts: PlanOpts) -> ApiResult<Plan> {
    blocking("device_plan", &dev, &st, move |d, s| make_plan(d, s, &opts)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn device_create(dev: Dev<'_>, st: St<'_>, plan_id: String, typed: String) -> ApiResult<String> {
    blocking("device_create", &dev, &st, move |d, s| create(d, s, &plan_id, &typed)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn device_resume(dev: Dev<'_>, st: St<'_>, id: String) -> ApiResult<String> {
    blocking("device_resume", &dev, &st, move |d, s| resume(d, s, &id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn device_fetch_missing(dev: Dev<'_>, st: St<'_>, job: String) -> ApiResult<Outcome> {
    blocking("device_fetch_missing", &dev, &st, move |d, _| fetch_missing(d, &job)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn device_eject(dev: Dev<'_>, st: St<'_>, id: String) -> ApiResult<()> {
    blocking("device_eject", &dev, &st, move |d, _| eject(d, &id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn device_log(dev: Dev<'_>, st: St<'_>, job: String) -> ApiResult<String> {
    blocking("device_log", &dev, &st, move |d, _| log_text(d, &job)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn device_open(dev: Dev<'_>, st: St<'_>, job: String) -> ApiResult<()> {
    blocking("device_open", &dev, &st, move |d, _| open_data(d, &job)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn device_outcome(dev: Dev<'_>, st: St<'_>, job: String) -> ApiResult<Outcome> {
    blocking("device_outcome", &dev, &st, move |d, _| outcome(d, &job)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn device_sticks(dev: Dev<'_>, st: St<'_>, path: Option<String>) -> ApiResult<Vec<super::stick::Stick>> {
    blocking("device_sticks", &dev, &st, move |d, _| Ok(super::stick::sticks(d.ops.as_ref(), &d.local, path.as_deref().map(std::path::Path::new)))).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn device_stick_mount(dev: Dev<'_>, st: St<'_>, id: String) -> ApiResult<super::stick::Stick> {
    blocking("device_stick_mount", &dev, &st, move |d, _| super::stick::mount(d, &id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn device_stick_update(dev: Dev<'_>, st: St<'_>, id: String) -> ApiResult<String> {
    blocking("device_stick_update", &dev, &st, move |d, s| super::stick::start_update(d, s, &id)).await
}

pub fn dry_run_cli(args: &[String]) -> i32 {
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let ops = super::backend::detect();
    let disks = ops.disks();
    let ctx = super::HostCtx::detect(vec![]);
    println!("NiceVault device dry run (backend: {}; nothing is executed)\n", ops.name());
    for d in &disks {
        let why = super::refusal(d, &ctx).map(|r| r.code()).unwrap_or("ok");
        println!("{:<14} {:<32} {:>8.1} GB  {:<8} S/N …{:<8} [{why}]", d.id, d.name(), d.size as f64 / 1e9, d.bus, d.serial_tail());
        for p in &d.partitions {
            println!("    {:<16} {:<12} {:<8} {:>8.1} GB {}", p.path, p.label, p.fs, p.size as f64 / 1e9, p.mountpoints.join(", "));
        }
    }
    let cands = super::candidates(disks.clone(), &ctx);
    let target = match arg("--disk") {
        Some(id) => disks.iter().find(|d| d.id == id).cloned(),
        None => cands.first().cloned(),
    };
    let Some(d) = target else {
        println!("\nNo external drive to plan for.");
        return 1;
    };
    let fs = match arg("--fs").as_deref() {
        Some("ntfs") => DataFs::Ntfs,
        Some("ext4") => DataFs::Ext4,
        Some("btrfs") => DataFs::Btrfs,
        Some("apfs") => DataFs::Apfs,
        _ => DataFs::Exfat,
    };
    let os = match arg("--os").as_deref() {
        Some("windows") => Os::Windows,
        Some("macos") => Os::Macos,
        Some("linux") => Os::Linux,
        _ => Os::current(),
    };
    let table = if arg("--table").as_deref() == Some("gpt") { Table::Gpt } else { Table::Mbr };
    let opts = PlanOpts { id: d.id.clone(), table, data_fs: fs, first_volume: None, download: true };
    match super::plan(&d, &opts, os, &ctx, "dry-run") {
        Ok(p) => {
            println!("\nPlan for {} ({}): {:?}, data {} ({} bytes)", d.name(), d.id, p.table, p.data_fs.id(), p.data_size);
            println!("Type to confirm: {}", if p.confirm_name.is_empty() { "CANCELLA / ERASE" } else { &p.confirm_name });
            for v in super::volumes_on(&d, &[]) {
                println!("NiceVault volume on it (erased for good): {} ({})", v.name, v.path);
            }
            println!("Steps: {}\n\nCommands:", p.steps.join(" > "));
            for c in &p.commands_preview {
                println!("  {c}");
            }
            0
        }
        Err(e) => {
            println!("\nRefused: {e}");
            1
        }
    }
}
