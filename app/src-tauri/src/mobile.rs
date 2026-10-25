//! Android / iOS only: the app sandbox as home, temp and config; files
//! and volumes picked through the system picker (`content://` URIs on Android, security-scoped
//! `file://` URLs on iOS), which std::fs can't open, are copied into the sandbox first; the
//! share sheet for decrypted copies; the camera roll (Android Photo Picker, iOS PHPicker). The
//! native half is `NvPlugin` (Android: gen/android/app/src/main/java/app/nicevault/desktop/
//! NvPlugin.kt; iOS: app/ios/Sources/NiceVault/NvPlugin.swift, which also covers the window in
//! the app switcher and locks the volumes after 30 s in the background).
//!
//! Staged plaintext (picked files and photos) lives only in `<app cache>/tmp/nv-import`, is
//! wiped as soon as its import job ends, and whatever is left there at the next start.

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

/// The native bridge (Android: NvPlugin.kt; iOS: NvPlugin.swift).
pub struct Bridge(Option<PluginHandle<Wry>>);

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_nvmobile);

/// The app, for the iOS lifecycle hook below and Android's DocumentsProvider (docs.rs), set
/// once in the plugin's setup.
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

/// The app's state once it is set up (Android's DocumentsProvider may be called before, or in a
/// process the app's activity never started: then there are no unlocked volumes).
pub fn app_state() -> Option<Arc<AppState>> {
    APP.get().and_then(|a| a.try_state::<Arc<AppState>>()).map(|s| s.inner().clone())
}

/// Android: a volume was unlocked or locked: the system picker / Files app re-reads the roots
/// (NvPlugin.kt `docsChanged`, a root per unlocked volume). Off the caller's thread: `emit`
/// may run with the state's locks held.
pub fn docs_changed(app: &AppHandle) {
    if cfg!(target_os = "android")
        && let Ok(h) = bridge(app)
    {
        std::thread::spawn(move || {
            let _ = h.run_mobile_plugin::<()>("docsChanged", serde_json::json!({}));
        });
    }
}

/// Android: another app is using a volume through the DocumentsProvider (a file open, or a call
/// in the last 30 s): lib/mobile.ts holds the background lock back meanwhile. iOS: never.
#[tauri::command(rename_all = "snake_case")]
pub fn mobile_docs_busy() -> bool {
    #[cfg(target_os = "android")]
    return crate::docs::busy();
    #[cfg(not(target_os = "android"))]
    false
}

/// iOS: NvPlugin.swift calls this (off the main thread, the privacy cover still up) when the
/// app comes back after 30 s or more in the background, or when the 30 s run out while iOS
/// still lets it run there: every open volume is locked (reason "screen", as lib/mobile.ts).
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn nv_mobile_lock_all() {
    if let Some(st) = APP.get().and_then(|a| a.try_state::<Arc<AppState>>()) {
        st.lock_all("screen");
    }
}

/// iOS: the web content process was terminated (lib.rs) or the page did not answer after a
/// return from the background (NvPlugin.swift): every volume locked, then the page reloaded.
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

/// Staging folder for picked files (inside the app cache: private to the app).
fn import_root() -> PathBuf {
    std::env::temp_dir().join("nv-import")
}

/// iOS: decrypted pictures / videos handed to the photo library (moved into it by Photos).
fn gallery_root() -> PathBuf {
    std::env::temp_dir().join("nv-gallery")
}

/// Decrypted copies handed to the share sheet (the only folder the FileProvider exposes).
fn share_root() -> PathBuf {
    std::env::temp_dir().join("nv-share")
}

/// The sandbox's Documents folder as it was before `prepare` moved `HOME`.
static DOCUMENTS: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Points `HOME`, `TMPDIR`, the config folder and the default volume folder at the sandbox.
/// Called first in `setup`, before the store is opened or any temp file is made: Android's
/// default temp dir (`/data/local/tmp`) is not writable by apps.
pub fn prepare(app: &AppHandle) {
    let p = app.path();
    let (Ok(data), Ok(cache), Ok(config)) = (p.app_data_dir(), p.app_cache_dir(), p.app_config_dir()) else { return };
    // iOS: the app's own Documents (shown in Files), resolved before HOME points elsewhere
    let docs = p.document_dir().ok();
    if let Some(d) = &docs {
        let _ = DOCUMENTS.set(d.clone());
    }
    let volumes = data.join("Volumes");
    let tmp = cache.join("tmp");
    for d in [&data, &volumes, &tmp, &config] {
        let _ = std::fs::create_dir_all(d);
    }
    // SAFETY: start-up, before the store, the auto-lock ticker and the drive watcher run; the
    // threads Tauri has started by now don't read these variables.
    unsafe {
        std::env::set_var("HOME", &data);
        std::env::set_var("TMPDIR", &tmp);
        std::env::set_var("NICEVAULT_CONFIG_DIR", config.join("NiceVault"));
        std::env::set_var("NICEVAULT_PORTABLE", "0");
        std::env::set_var("NICEVAULT_VOLUME_DIR", &volumes);
        // Benvenuto lists every volume in the app's own folders whatever known.json says (an
        // iOS update moves the container, store.rs relocates the known paths too); Documents and
        // Documents/NiceVault: volumes put there through the Files app
        let scan: Vec<&Path> = std::iter::once(volumes.as_path()).chain(docs.as_deref()).collect();
        if let Ok(v) = std::env::join_paths(scan) {
            std::env::set_var("NICEVAULT_SCAN_DIRS", v);
        }
    }
    // a previous run's staged and shared copies (plaintext): gone before anything else
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

/// Display names of picked URIs from their providers, `uri_name` where there is none.
fn names_of(app: &AppHandle, uris: &[String]) -> Vec<String> {
    let native = app
        .try_state::<Bridge>()
        .and_then(|b| b.0.as_ref().and_then(|h| h.run_mobile_plugin::<Names>("displayNames", UrisArgs { uris }).ok()))
        .map(|n| n.names)
        .unwrap_or_default();
    uris.iter().enumerate().map(|(i, u)| native.get(i).filter(|n| !n.trim().is_empty()).map(|n| clean_name(n)).unwrap_or_else(|| uri_name(u))).collect()
}

/// Copies a picked URI to `dir/name` (new file).
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

/// Staging folders, wiped when dropped.
struct Staged(Vec<PathBuf>);
impl Drop for Staged {
    fn drop(&mut self) {
        self.0.iter().for_each(|d| wipe_dir(d));
    }
}

/// `fs_import` on mobile: plain paths go through as on desktop; URIs are copied (under their
/// provider's display name) into a fresh staging folder first. Staged copies (these, and the
/// ones `mobile_pick_photos` / `mobile_pick_files` made on iOS) are wiped when the job ends.
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
                    // each in its own folder: two picks may have the same name
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
        // wiped when the job has ended, or right away when it fails to start (the closure is
        // dropped then without running)
        let wipe = Staged(staged);
        r?;
        crate::jobs::fs_import_then(&s, &vault, parent, paths, move || drop(wipe))
    })
    .await
}

#[derive(Serialize)]
struct PickArgs {
    /// iOS: the fresh staging folder the picked files are copied into (Android returns URIs)
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
    // nothing picked (or it failed): no empty staging folder left behind
    if !r.as_ref().is_ok_and(|p| p.files.iter().any(|f| !f.contains("://"))) {
        wipe_dir(&dir);
    }
    Ok(r?.files)
}

// ---------------- camera roll (mobile_import_photos) ----------------

/// The last camera-roll pick, kept for "Riprova questi".
enum PickSrc {
    /// NvPlugin.swift keeps the PHPicker results of `session`
    Ios { session: i64, names: Vec<String> },
    /// content:// URIs of the Photo Picker and their display names
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
/// A camera-roll import is running (one at a time: the native side keeps one pick).
static PHOTOS_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Download progress (0..1) of the items being fetched, from NvPlugin.swift.
static FRACTIONS: std::sync::Mutex<Option<std::collections::HashMap<usize, f64>>> = std::sync::Mutex::new(None);

fn fractions() -> std::sync::MutexGuard<'static, Option<std::collections::HashMap<usize, f64>>> {
    FRACTIONS.lock().unwrap_or_else(|e| e.into_inner())
}

/// iOS: NvPlugin.swift reports the download of item `index` (NSItemProvider's Progress).
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
    /// Android: the picked URIs
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

/// The camera roll into a folder of a volume (Android Photo Picker, iOS PHPicker; photos and
/// videos, originals, no photo-library permission). Every picked item is fetched — iOS waits for
/// the iCloud download of a photo the phone keeps only as an optimised copy, Android for a
/// Google Photos one — four at a time, imported as soon as it arrives and its staged copy wiped
/// (pick_queue.rs). `mobile-pick-progress` reports "N di M" and, at the end, the items that could
/// not be imported with the reason; `retry` imports those again from the same pick. `None` when
/// nothing was picked.
#[tauri::command(rename_all = "snake_case")]
pub async fn mobile_import_photos(app: AppHandle, st: State<'_, Arc<AppState>>, vault: String, parent: NodeId, retry: Option<Vec<usize>>) -> ApiResult<Option<PhotoImport>> {
    let s = st.inner().clone();
    blocking(move || {
        use std::sync::atomic::Ordering;
        if PHOTOS_BUSY.load(Ordering::SeqCst) {
            return Err(ApiError::new("busy", "Photos are already being imported."));
        }
        // the target first: no picker over a locked volume
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
                // the Photo Picker's provider downloads a cloud item (Google Photos) while it is read
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
            // a full or locked volume: nothing more can go in
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
                    // the downloads in flight stop now, not when they are done
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

/// iOS: the document picker (Files, iCloud Drive, USB, providers), opened in place through
/// security-scoped URLs and copied into a staging folder under the file's real name; no copy
/// is left in the app's Inbox. Android uses the dialog plugin (SAF) and gets "unsupported".
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

/// A volume picked in the system picker (Files, a USB stick, a cloud drive): copied into the
/// app's Volumes folder (copy-in); returns the path to open. A plain
/// path is returned as it is.
#[tauri::command(rename_all = "snake_case")]
pub async fn mobile_volume_in(app: AppHandle, uri: String) -> ApiResult<String> {
    blocking(move || {
        let staged = staging_of(Path::new(&uri));
        if !uri.contains("://") && staged.is_none() {
            return Ok(uri);
        }
        let dir = PathBuf::from(std::env::var_os("NICEVAULT_VOLUME_DIR").ok_or_else(|| io("no volume folder"))?);
        if let Some(st) = staged {
            // iOS: picked into staging (mobile_pick_files): moved into Volumes
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

/// The file `n` of `v`, decrypted into `out` (1 MiB at a time through `buf`, zeroed after).
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
    /// went into the gallery
    saved: Vec<NodeId>,
    /// not pictures / videos (folders, documents, formats the gallery refuses): the UI exports
    /// these to Files as before
    others: Vec<NodeId>,
}

/// "Salva nella Galleria": pictures and videos of a volume into the phone's gallery. Android:
/// a MediaStore entry in Pictures/NiceVault or Movies/NiceVault (no storage permission from
/// Android 10; older ones answer `unsupported`), written straight from the volume through the
/// entry's file descriptor, nothing staged. iOS: PHPhotoLibrary, add-only access (asked once by
/// the system: `gallery_denied` when refused); a decrypted copy in `<cache>/tmp/nv-gallery`
/// that Photos moves into the library, the folder wiped right after either way.
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
                    // SAFETY: NvPlugin.kt detached this descriptor for us: we own it now
                    let mut f = unsafe { std::fs::File::from_raw_fd(o.fd) };
                    decrypt_to(&v, &n, &mut f, &mut buf).and_then(|()| f.sync_all().map_err(io))
                };
                #[cfg(not(unix))]
                let r: ApiResult<()> = Err(io(o.fd));
                // published, or the half-written entry removed
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

/// The share sheet for files of a volume: decrypted into the share folder (app cache, wiped at
/// the next start and ten minutes after), handed out read-only. Folders are skipped.
#[tauri::command(rename_all = "snake_case")]
pub async fn mobile_share(app: AppHandle, st: State<'_, Arc<AppState>>, vault: String, nodes: Vec<NodeId>, title: Option<String>) -> ApiResult<()> {
    let s = st.inner().clone();
    blocking(move || {
        let handle = bridge(&app)?;
        // copies older than ten minutes: the receiving app has had them
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

/// Where "Esporta" writes on a phone (there is no folder picker): Android's public
/// Download/NiceVault (apps may create files there by path since Android 11), iOS the app's
/// Documents folder (shown in the Files app).
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
