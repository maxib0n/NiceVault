use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use nv_fs::{Fs, Kind, NodeId};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{AppHandle, Manager, State, Wry};
use tauri_plugin_fs::{FilePath, FsExt, OpenOptions};

use crate::mobile_paths::uri_name;
use crate::mobile_paths::{clean_name, unique_in, volume_name};

fn staging_of(p: &Path) -> Option<PathBuf> {
    crate::mobile_paths::staging_of(&import_root(), p)
}

pub struct Bridge(Option<PluginHandle<Wry>>);

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_nvmobile);

static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

pub fn plugin() -> TauriPlugin<Wry> {
    Builder::new("nvmobile")
        .setup(|app, _api| {
            let _ = APP.set(app.clone());
            #[cfg(target_os = "android")]
            let handle = Some(_api.register_android_plugin("app.nicevault.desktop", "NvPlugin")?);
            #[cfg(target_os = "ios")]
            let handle = Some(_api.register_ios_plugin(init_plugin_nvmobile)?);
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            let handle = None;
            app.manage(Bridge(handle));
            Ok(())
        })
        .build()
}

pub fn app_state() -> Option<Arc<AppState>> {
    APP.get().and_then(|a| a.try_state::<Arc<AppState>>()).map(|s| s.inner().clone())
}

pub fn docs_changed(app: &AppHandle) {
    if cfg!(target_os = "android")
        && let Ok(h) = bridge(app)
    {
        std::thread::spawn(move || {
            let _ = h.run_mobile_plugin::<()>("docsChanged", serde_json::json!({}));
        });
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn mobile_docs_busy() -> bool {
    #[cfg(target_os = "android")]
    return crate::docs::busy();
    #[cfg(not(target_os = "android"))]
    false
}

#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn nv_mobile_lock_all() {
    if let Some(st) = APP.get().and_then(|a| a.try_state::<Arc<AppState>>()) {
        st.lock_all("screen");
    }
}

#[cfg(target_os = "ios")]
pub fn web_content_gone(w: tauri::Webview) {
    std::thread::spawn(move || {
        if let Some(st) = w.try_state::<Arc<AppState>>() {
            st.lock_all("screen");
        }
        let _ = w.reload();
    });
}

fn bridge(app: &AppHandle) -> ApiResult<PluginHandle<Wry>> {
    app.try_state::<Bridge>().and_then(|b| b.0.clone()).ok_or_else(|| ApiError::new("unsupported", "Not available on this device."))
}

fn io(e: impl std::fmt::Display) -> ApiError {
    ApiError::new("io", e.to_string())
}

fn stamp() -> String {
    format!("{:x}", SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0))
}

fn import_root() -> PathBuf {
    std::env::temp_dir().join("nv-import")
}

fn gallery_root() -> PathBuf {
    std::env::temp_dir().join("nv-gallery")
}

fn share_root() -> PathBuf {
    std::env::temp_dir().join("nv-share")
}

static DOCUMENTS: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub fn prepare(app: &AppHandle) {
    let p = app.path();
    let (Ok(data), Ok(cache), Ok(config)) = (p.app_data_dir(), p.app_cache_dir(), p.app_config_dir()) else { return };
    let docs = p.document_dir().ok();
    if let Some(d) = &docs {
        let _ = DOCUMENTS.set(d.clone());
    }
    let volumes = data.join("Volumes");
    let tmp = cache.join("tmp");
    for d in [&data, &volumes, &tmp, &config] {
        let _ = std::fs::create_dir_all(d);
    }
    unsafe {
        std::env::set_var("HOME", &data);
        std::env::set_var("TMPDIR", &tmp);
        std::env::set_var("NICEVAULT_CONFIG_DIR", config.join("NiceVault"));
        std::env::set_var("NICEVAULT_PORTABLE", "0");
        std::env::set_var("NICEVAULT_VOLUME_DIR", &volumes);
        let scan: Vec<&Path> = std::iter::once(volumes.as_path()).chain(docs.as_deref()).collect();
        if let Ok(v) = std::env::join_paths(scan) {
            std::env::set_var("NICEVAULT_SCAN_DIRS", v);
        }
    }
    for d in [import_root(), share_root(), gallery_root()] {
        wipe_dir(&d);
    }
}

fn wipe_dir(d: &Path) {
    if let Ok(rd) = std::fs::read_dir(d) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                wipe_dir(&p);
            } else {
                crate::external::wipe_file(&p);
            }
        }
    }
    let _ = std::fs::remove_dir_all(d);
}

#[derive(Serialize)]
struct UrisArgs<'a> {
    uris: &'a [String],
}
#[derive(Deserialize)]
struct Names {
    names: Vec<String>,
}

fn names_of(app: &AppHandle, uris: &[String]) -> Vec<String> {
    let native = app
        .try_state::<Bridge>()
        .and_then(|b| b.0.as_ref().and_then(|h| h.run_mobile_plugin::<Names>("displayNames", UrisArgs { uris }).ok()))
        .map(|n| n.names)
        .unwrap_or_default();
    uris.iter().enumerate().map(|(i, u)| native.get(i).filter(|n| !n.trim().is_empty()).map(|n| clean_name(n)).unwrap_or_else(|| uri_name(u))).collect()
}

fn copy_in(app: &AppHandle, uri: &str, dest: &Path) -> ApiResult<()> {
    let url = tauri::Url::parse(uri).map_err(|e| ApiError::new("io", format!("{uri}: {e}")))?;
    let mut opts = OpenOptions::new();
    opts.read(true);
    let mut src = app.fs().open(FilePath::Url(url), opts).map_err(io)?;
    let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(dest).map_err(io)?;
    std::io::copy(&mut src, &mut out).map_err(io)?;
    out.sync_all().map_err(io)?;
    Ok(())
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> ApiResult<T> + Send + 'static) -> ApiResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| ApiError::new("error", format!("Internal error: {e}")))?
}

struct Staged(Vec<PathBuf>);
impl Drop for Staged {
    fn drop(&mut self) {
        self.0.iter().for_each(|d| wipe_dir(d));
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn fs_import(app: AppHandle, st: State<'_, Arc<AppState>>, vault: String, parent: NodeId, host_paths: Vec<String>) -> ApiResult<String> {
    let s = st.inner().clone();
    blocking(move || {
        let uris: Vec<String> = host_paths.iter().filter(|p| p.contains("://")).cloned().collect();
        let names = names_of(&app, &uris);
        let dir = import_root().join(stamp());
        let mut paths = Vec::with_capacity(host_paths.len());
        let mut staged: Vec<PathBuf> = Vec::new();
        let mut k = 0;
        let r = (|| -> ApiResult<()> {
            for p in host_paths {
                if p.contains("://") {
                    if !staged.contains(&dir) {
                        std::fs::create_dir_all(&dir).map_err(io)?;
                        staged.push(dir.clone());
                    }
                    let sub = dir.join(k.to_string());
                    std::fs::create_dir(&sub).map_err(io)?;
                    let dest = sub.join(&names[k]);
                    copy_in(&app, &p, &dest)?;
                    paths.push(dest.to_string_lossy().into_owned());
                    k += 1;
                } else {
                    if let Some(d) = staging_of(Path::new(&p))
                        && !staged.contains(&d)
                    {
                        staged.push(d);
                    }
                    paths.push(p);
                }
            }
            Ok(())
        })();
        let wipe = Staged(staged);
        r?;
        crate::jobs::fs_import_then(&s, &vault, parent, paths, move || drop(wipe))
    })
    .await
}

#[derive(Serialize)]
struct PickArgs {
    dir: String,
    multiple: bool,
}
#[derive(Deserialize)]
struct Picked {
    files: Vec<String>,
}

fn pick(app: &AppHandle, cmd: &str, multiple: bool) -> ApiResult<Vec<String>> {
    let handle = bridge(app)?;
    let dir = import_root().join(stamp());
    std::fs::create_dir_all(&dir).map_err(io)?;
    let r = handle.run_mobile_plugin::<Picked>(cmd, PickArgs { dir: dir.to_string_lossy().into_owned(), multiple }).map_err(|e| ApiError::new("io", e.to_string()));
    if !r.as_ref().is_ok_and(|p| p.files.iter().any(|f| !f.contains("://"))) {
        wipe_dir(&dir);
    }
    Ok(r?.files)
}

enum PickSrc {
    Ios { session: i64, names: Vec<String> },
    Android { uris: Vec<String>, names: Vec<String> },
}

impl PickSrc {
    fn len(&self) -> usize {
        match self {
            PickSrc::Ios { names, .. } | PickSrc::Android { names, .. } => names.len(),
        }
    }
    fn names(&self) -> &[String] {
        match self {
            PickSrc::Ios { names, .. } | PickSrc::Android { names, .. } => names,
        }
    }
}

static PICK: std::sync::Mutex<Option<Arc<PickSrc>>> = std::sync::Mutex::new(None);
static PHOTOS_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static FRACTIONS: std::sync::Mutex<Option<std::collections::HashMap<usize, f64>>> = std::sync::Mutex::new(None);

fn fractions() -> std::sync::MutexGuard<'static, Option<std::collections::HashMap<usize, f64>>> {
    FRACTIONS.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn nv_mobile_pick_progress(index: isize, fraction: f64) {
    if let Ok(i) = usize::try_from(index) {
        fractions().get_or_insert_with(Default::default).insert(i, fraction.clamp(0.0, 1.0));
    }
}

#[derive(Deserialize, Default)]
struct PhotoPick {
    #[serde(default)]
    session: i64,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Serialize)]
struct LoadArgs {
    session: i64,
    index: usize,
    dir: String,
}
#[derive(Deserialize)]
struct Loaded {
    path: String,
}

#[derive(Serialize)]
pub struct PhotoImport {
    job: String,
    total: usize,
}

struct Busy;
impl Drop for Busy {
    fn drop(&mut self) {
        PHOTOS_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

fn native_fail(e: tauri::plugin::mobile::PluginInvokeError) -> crate::pick_queue::Fail {
    use crate::pick_queue::{Fail, Reason};
    match e {
        tauri::plugin::mobile::PluginInvokeError::InvokeRejected(r) => {
            Fail::new(Reason::from_code(r.code.as_deref().unwrap_or("")), r.message.unwrap_or_default())
        }
        e => Fail::new(Reason::Other, e.to_string()),
    }
}

fn io_fail(e: &ApiError) -> crate::pick_queue::Fail {
    use crate::pick_queue::{Fail, Reason};
    let m = e.message.to_ascii_lowercase();
    let reason = if m.contains("no space") || m.contains("enospc") || m.contains("disk full") { Reason::NoSpace } else { Reason::Other };
    Fail::new(reason, e.message.clone())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mobile_import_photos(app: AppHandle, st: State<'_, Arc<AppState>>, vault: String, parent: NodeId, retry: Option<Vec<usize>>) -> ApiResult<Option<PhotoImport>> {
    let s = st.inner().clone();
    blocking(move || {
        use std::sync::atomic::Ordering;
        if PHOTOS_BUSY.load(Ordering::SeqCst) {
            return Err(ApiError::new("busy", "Photos are already being imported."));
        }
        s.vault(&vault)?;
        let (src, indices) = match retry {
            Some(ix) => {
                let src = PICK.lock().unwrap_or_else(|e| e.into_inner()).clone().ok_or_else(|| ApiError::new("expired", "The photos are no longer available: pick them again."))?;
                let n = src.len();
                (src, ix.into_iter().filter(|&i| i < n).collect::<Vec<_>>())
            }
            None => {
                let handle = bridge(&app)?;
                let r = handle.run_mobile_plugin::<PhotoPick>("pickPhotos", PickArgs { dir: String::new(), multiple: true }).map_err(|e| ApiError::new("io", e.to_string()))?;
                let src = if cfg!(target_os = "ios") {
                    PickSrc::Ios { session: r.session, names: r.names.iter().map(|n| clean_name(n)).collect() }
                } else {
                    let names = names_of(&app, &r.files);
                    PickSrc::Android { uris: r.files, names }
                };
                let src = Arc::new(src);
                *PICK.lock().unwrap_or_else(|e| e.into_inner()) = Some(src.clone());
                let n = src.len();
                (src, (0..n).collect())
            }
        };
        if indices.is_empty() {
            return Ok(None);
        }
        if PHOTOS_BUSY.swap(true, Ordering::SeqCst) {
            return Err(ApiError::new("busy", "Photos are already being imported."));
        }
        let busy = Busy;
        let total = indices.len();
        let st2 = s.clone();
        let job = crate::jobs::import_stream(&s, &vault, parent, move |sink| {
            let _busy = busy;
            photo_feed(&app, &st2, &src, &indices, sink)
        })?;
        Ok(Some(PhotoImport { job, total }))
    })
    .await
}

fn photo_feed(app: &AppHandle, st: &Arc<AppState>, src: &PickSrc, indices: &[usize], sink: &mut crate::jobs::Sink) -> ApiResult<()> {
    use crate::pick_queue::{Fail, Fetched, Hooks, ImportFail, Reason, Status};
    use std::sync::atomic::{AtomicBool, Ordering};
    let job = sink.job().to_owned();
    let cancel = sink.cancel_flag();
    let dir = import_root().join(stamp());
    std::fs::create_dir_all(&dir).map_err(io)?;
    let _wipe = Staged(vec![dir.clone()]);
    *fractions() = Some(Default::default());
    let status = std::sync::Mutex::new(Status::default());
    let finished = AtomicBool::new(false);
    let names = src.names();
    let fetch = |i: usize, attempt: u32| -> Result<PathBuf, Fail> {
        let slot = dir.join(format!("{i}-{attempt}"));
        std::fs::create_dir_all(&slot).map_err(|e| Fail::new(Reason::Other, e.to_string()))?;
        let r = match src {
            PickSrc::Ios { session, .. } => bridge(app)
                .map_err(|e| Fail::new(Reason::Other, e.message))?
                .run_mobile_plugin::<Loaded>("loadPhoto", LoadArgs { session: *session, index: i, dir: slot.to_string_lossy().into_owned() })
                .map(|l| PathBuf::from(l.path))
                .map_err(native_fail),
            PickSrc::Android { uris, names } => {
                let dest = slot.join(&names[i]);
                copy_in(app, &uris[i], &dest).map(|()| dest).map_err(|e| io_fail(&e))
            }
        };
        if r.is_err() {
            wipe_dir(&slot);
        }
        if let Some(f) = fractions().as_mut() {
            f.remove(&i);
        }
        r
    };
    let mut import = |f: &Fetched| -> Result<(), ImportFail> {
        sink.put(&f.path).map_err(|e| {
            let reason = match e.code.as_str() {
                "cancelled" => Reason::Cancelled,
                _ if io_fail(&e).reason == Reason::NoSpace => Reason::NoSpace,
                _ => Reason::Other,
            };
            let stop = reason == Reason::NoSpace || e.code == "locked" || e.code == "not_found";
            ImportFail { fail: Fail::new(reason, e.message), stop }
        })
    };
    let wipe = |f: &Fetched| {
        if let Some(slot) = f.path.parent().filter(|p| p.starts_with(&dir) && *p != dir.as_path()) {
            wipe_dir(slot);
        }
    };
    let free = || fs4::available_space(&dir).ok();
    let sleep = |d: Duration| std::thread::sleep(d);
    let name = |i: usize| names.get(i).cloned().unwrap_or_default();
    let emit = |fin: bool, failures: Option<&[crate::pick_queue::Failure]>| {
        let s = status.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let inflight: f64 = fractions().as_ref().map_or(0.0, |f| s.fetching.iter().filter_map(|i| f.get(i)).sum());
        let done = s.imported + s.failed;
        st.emit(
            "mobile-pick-progress",
            serde_json::json!({
                "job": job, "total": s.total, "done": done, "imported": s.imported, "failed": s.failed,
                "fetching": s.fetching.len(), "fraction": if s.total > 0 { ((done as f64 + inflight) / s.total as f64).min(1.0) } else { 1.0 },
                "finished": fin, "cancelled": cancel.load(Ordering::SeqCst), "failures": failures,
            }),
        );
    };
    let summary = std::thread::scope(|sc| {
        sc.spawn(|| {
            let mut told = false;
            while !finished.load(Ordering::SeqCst) {
                emit(false, None);
                if cancel.load(Ordering::SeqCst) && !told {
                    told = true;
                    if let (PickSrc::Ios { session, .. }, Ok(h)) = (src, bridge(app)) {
                        let _ = h.run_mobile_plugin::<()>("cancelPhotos", serde_json::json!({ "session": session }));
                    }
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        });
        let r = crate::pick_queue::run(indices, Hooks { fetch: &fetch, import: &mut import, wipe: &wipe, free_space: &free, sleep: &sleep, name: &name }, cancel, &status);
        finished.store(true, Ordering::SeqCst);
        r
    });
    *fractions() = None;
    emit(true, Some(&summary.failures));
    if cancel.load(Ordering::SeqCst) {
        return Err(ApiError::cancelled());
    }
    if summary.imported.is_empty() && !summary.failures.is_empty() {
        return Err(ApiError::new("photos_failed", format!("None of the {} photos could be imported.", summary.failures.len())));
    }
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mobile_pick_files(app: AppHandle, multiple: bool) -> ApiResult<Vec<String>> {
    blocking(move || {
        if cfg!(target_os = "android") {
            return Err(ApiError::new("unsupported", "Not available on this device."));
        }
        pick(&app, "pickDocuments", multiple)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mobile_volume_in(app: AppHandle, uri: String) -> ApiResult<String> {
    blocking(move || {
        let staged = staging_of(Path::new(&uri));
        if !uri.contains("://") && staged.is_none() {
            return Ok(uri);
        }
        let dir = PathBuf::from(std::env::var_os("NICEVAULT_VOLUME_DIR").ok_or_else(|| io("no volume folder"))?);
        if let Some(st) = staged {
            let src = PathBuf::from(&uri);
            let dest = unique_in(&dir, &volume_name(&clean_name(&src.file_name().map(|n| n.to_string_lossy()).unwrap_or_default())));
            let r = std::fs::rename(&src, &dest).map_err(io);
            wipe_dir(&st);
            return r.map(|()| dest.to_string_lossy().into_owned());
        }
        let name = volume_name(&names_of(&app, std::slice::from_ref(&uri)).remove(0));
        let dest = unique_in(&dir, &name);
        let tmp = dir.join(format!(".{}.part", stamp()));
        let r = copy_in(&app, &uri, &tmp).and_then(|()| std::fs::rename(&tmp, &dest).map_err(io));
        if r.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        r.map(|()| dest.to_string_lossy().into_owned())
    })
    .await
}

fn decrypt_to(v: &nv_core::Vault, n: &nv_fs::Node, out: &mut impl Write, buf: &mut [u8]) -> ApiResult<()> {
    let mut off = 0u64;
    let r = (|| {
        while off < n.size {
            let want = buf.len().min((n.size - off) as usize);
            let got = v.read(n.id, off, &mut buf[..want])?;
            if got == 0 {
                break;
            }
            out.write_all(&buf[..got]).map_err(io)?;
            off += got as u64;
        }
        out.flush().map_err(io)
    })();
    buf.fill(0);
    r
}

fn native_err(e: tauri::plugin::mobile::PluginInvokeError) -> ApiError {
    match e {
        tauri::plugin::mobile::PluginInvokeError::InvokeRejected(r) => {
            ApiError::new(r.code.as_deref().filter(|c| !c.is_empty()).unwrap_or("io"), r.message.unwrap_or_default())
        }
        e => ApiError::new("io", e.to_string()),
    }
}

#[derive(Serialize)]
pub struct GallerySaved {
    saved: Vec<NodeId>,
    others: Vec<NodeId>,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mobile_save_gallery(app: AppHandle, st: State<'_, Arc<AppState>>, vault: String, nodes: Vec<NodeId>) -> ApiResult<GallerySaved> {
    let s = st.inner().clone();
    blocking(move || {
        use crate::mobile_paths::{Media, doc_mime, gallery_kind};
        let handle = bridge(&app)?;
        let v = s.vault(&vault)?;
        let mut out = GallerySaved { saved: vec![], others: vec![] };
        let mut buf = vec![0u8; 1 << 20];
        for id in nodes {
            let n = v.getattr(id)?;
            let kind = if n.kind == Kind::File { gallery_kind(&n.name, cfg!(target_os = "ios")) } else { None };
            let Some(kind) = kind else {
                out.others.push(id);
                continue;
            };
            let name = clean_name(&n.name);
            let video = kind == Media::Video;
            if cfg!(target_os = "android") {
                #[derive(Deserialize)]
                struct Opened {
                    uri: String,
                    fd: i32,
                }
                let o: Opened = handle
                    .run_mobile_plugin("galleryOpen", serde_json::json!({ "name": name, "mime": doc_mime(&n.name), "video": video }))
                    .map_err(native_err)?;
                #[cfg(unix)]
                let r = {
                    use std::os::fd::FromRawFd;
                    let mut f = unsafe { std::fs::File::from_raw_fd(o.fd) };
                    decrypt_to(&v, &n, &mut f, &mut buf).and_then(|()| f.sync_all().map_err(io))
                };
                #[cfg(not(unix))]
                let r: ApiResult<()> = Err(io(o.fd));
                handle.run_mobile_plugin::<()>("galleryClose", serde_json::json!({ "uri": o.uri, "ok": r.is_ok() })).map_err(native_err)?;
                r?;
            } else {
                let dir = gallery_root().join(stamp());
                std::fs::create_dir_all(&dir).map_err(io)?;
                let _wipe = Staged(vec![dir.clone()]);
                let dest = dir.join(&name);
                let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&dest).map_err(io)?;
                decrypt_to(&v, &n, &mut f, &mut buf)?;
                f.sync_all().map_err(io)?;
                drop(f);
                handle
                    .run_mobile_plugin::<()>("saveToGallery", serde_json::json!({ "path": dest.to_string_lossy(), "video": video }))
                    .map_err(native_err)?;
            }
            out.saved.push(id);
        }
        Ok(out)
    })
    .await
}

#[derive(Serialize)]
struct ShareArgs {
    paths: Vec<String>,
    title: Option<String>,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mobile_share(app: AppHandle, st: State<'_, Arc<AppState>>, vault: String, nodes: Vec<NodeId>, title: Option<String>) -> ApiResult<()> {
    let s = st.inner().clone();
    blocking(move || {
        let handle = bridge(&app)?;
        if let Ok(rd) = std::fs::read_dir(share_root()) {
            for e in rd.flatten() {
                let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d > Duration::from_secs(600));
                if old {
                    wipe_dir(&e.path());
                }
            }
        }
        let v = s.vault(&vault)?;
        let dir = share_root().join(stamp());
        std::fs::create_dir_all(&dir).map_err(io)?;
        let mut paths = Vec::new();
        let mut buf = vec![0u8; 1 << 20];
        for (k, id) in nodes.into_iter().enumerate() {
            let n = v.getattr(id)?;
            if n.kind != Kind::File {
                continue;
            }
            let sub = dir.join(k.to_string());
            std::fs::create_dir(&sub).map_err(io)?;
            let dest = sub.join(clean_name(&n.name));
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&dest).map_err(io)?;
            decrypt_to(&v, &n, &mut f, &mut buf)?;
            paths.push(dest.to_string_lossy().into_owned());
        }
        if paths.is_empty() {
            return Ok(());
        }
        handle.run_mobile_plugin::<()>("share", ShareArgs { paths, title }).map_err(|e| ApiError::new("io", e.to_string()))
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mobile_export_dir(app: AppHandle) -> ApiResult<String> {
    #[cfg(target_os = "android")]
    let base = {
        let _ = &app;
        PathBuf::from("/storage/emulated/0/Download/NiceVault")
    };
    #[cfg(not(target_os = "android"))]
    let base = match DOCUMENTS.get() {
        Some(d) => d.clone(),
        None => app.path().document_dir().map_err(io)?,
    }
    .join("NiceVault");
    std::fs::create_dir_all(&base).map_err(io)?;
    Ok(base.to_string_lossy().into_owned())
}
