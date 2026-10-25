//! Self-update for the bare portable builds (Linux binary, single Windows .exe, macOS .app zip).
//!
//! tauri-plugin-updater only installs AppImage / NSIS / MSI / .app.tar.gz, so this is a small
//! custom one with the same trust model (minisign, Tauri's `latest.json`):
//!   1. GET latest.json (the one URL in tauri.conf.json `plugins.updater.endpoints`: an asset of
//!      the public GitHub release, any static HTTPS host works; plain GET, no token, no
//!      cookies, no telemetry);
//!   2. pick `platforms[<os>-<arch>]`, compare semver with this build;
//!   3. download the artefact (its URL must have the manifest's origin; redirects, e.g. GitHub's
//!      to its asset CDN, are followed but must stay HTTPS);
//!   4. verify the minisign signature of the file against the public key in tauri.conf.json;
//!   5. replace the running executable where it is (USB stick included): write `<name>.new`
//!      next to it, fsync, keep the current one as `<name>.old`, rename the new one in.
//!      macOS: the whole `X.app` bundle is swapped (`X.app.old` kept), files byte-identical to
//!      the signed zip so the ad-hoc signature stays valid). Running from an AppImage, the target
//!      is the .AppImage file itself ($APPIMAGE; the mounted binary is read-only) and the
//!      manifest key is `linux-x86_64-appimage`. When the location is not writable
//!      (installed system-wide) the verified artefact goes to the Downloads folder instead.
//!
//! Preferences (check at start on/off, last check): `updater.json` in the OS config dir when
//! installed; in memory only when portable (nothing on the stick).
//!
//! Test / dev overrides (environment): NICEVAULT_UPDATE_URL (manifest URL; https, or http on
//! 127.0.0.1 / localhost).

use crate::error::{ApiError, ApiResult};
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const PREFS_FILE: &str = "updater.json";
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const MAX_MANIFEST: u64 = 1 << 20;
const MAX_ARTIFACT: u64 = 1 << 30;

// ---------------------------------------------------------------------------------------------
// configuration (tauri.conf.json `plugins.updater`: the one place the project and key live)

#[derive(Deserialize)]
struct ConfFile {
    plugins: ConfPlugins,
}
#[derive(Deserialize)]
struct ConfPlugins {
    updater: ConfUpdater,
}
#[derive(Deserialize)]
struct ConfUpdater {
    pubkey: String,
    endpoints: Vec<String>,
}

/// (manifest URL, base64 public key) from the embedded tauri.conf.json.
pub fn embedded_config() -> (String, String) {
    let c: ConfFile = serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json plugins.updater");
    (c.plugins.updater.endpoints[0].clone(), c.plugins.updater.pubkey)
}

/// `latest.json`'s platform key for this build.
pub fn platform_key() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("windows", "x86_64") => "windows-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        ("macos", "aarch64") => "darwin-aarch64",
        ("macos", "x86_64") => "darwin-x86_64",
        _ => "unsupported",
    }
}

/// The `.AppImage` this process runs from ($APPIMAGE, set by the AppImage runtime), Linux only.
pub fn appimage_path() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_absolute() && p.is_file())
}

/// The file that is the app: the AppImage when running from one (current_exe() is then inside
/// its read-only mount), else the executable.
pub fn app_exe() -> Option<PathBuf> {
    appimage_path().or_else(|| std::env::current_exe().ok())
}

/// The manifest key for the way this process runs: Tauri's `<os>-<arch>-appimage` from an AppImage.
pub fn running_platform() -> String {
    match appimage_path() {
        Some(_) => format!("{}-appimage", platform_key()),
        None => platform_key().to_owned(),
    }
}

// ---------------------------------------------------------------------------------------------
// manifest

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Notes {
    #[serde(default)]
    pub it: String,
    #[serde(default)]
    pub en: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct PlatformAsset {
    pub signature: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub notes_i18n: Option<Notes>,
    #[serde(default)]
    pub pub_date: Option<String>,
    pub platforms: BTreeMap<String, PlatformAsset>,
}

impl Manifest {
    pub fn notes(&self) -> Notes {
        let mut n = self.notes_i18n.clone().unwrap_or(Notes { it: String::new(), en: String::new() });
        if n.en.is_empty() {
            n.en = self.notes.clone();
        }
        if n.it.is_empty() {
            n.it = n.en.clone();
        }
        n
    }
}

pub fn parse_manifest(bytes: &[u8]) -> ApiResult<Manifest> {
    let m: Manifest = serde_json::from_slice(bytes).map_err(|e| invalid(format!("invalid latest.json ({e})")))?;
    semver::Version::parse(&m.version).map_err(|_| invalid(format!("invalid version: {}", m.version)))?;
    Ok(m)
}

/// `candidate` is strictly newer than `current` (semver, pre-releases ordered as usual).
pub fn is_newer(current: &str, candidate: &str) -> ApiResult<bool> {
    let cur = semver::Version::parse(current).map_err(|_| invalid(format!("invalid version: {current}")))?;
    let new = semver::Version::parse(candidate).map_err(|_| invalid(format!("invalid version: {candidate}")))?;
    Ok(new > cur)
}

// ---------------------------------------------------------------------------------------------
// signature (Tauri format: base64 of the minisign text files)

fn b64_text(s: &str) -> Option<String> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD.decode(s.trim()).ok()?;
    String::from_utf8(raw).ok()
}

fn bad_signature() -> ApiError {
    ApiError::new("update_signature", "The update's signature is not valid: it was not installed.")
}

/// Verifies `data` and returns the signed trusted comment.
pub fn verify_signature(pubkey_b64: &str, data: &[u8], signature_b64: &str) -> ApiResult<String> {
    let pk = b64_text(pubkey_b64).and_then(|t| PublicKey::decode(&t).ok()).ok_or_else(|| invalid("invalid public key"))?;
    let sig = b64_text(signature_b64).and_then(|t| Signature::decode(&t).ok()).ok_or_else(bad_signature)?;
    pk.verify(data, &sig, false).map_err(|_| bad_signature())?;
    Ok(sig.trusted_comment().to_owned())
}

/// The signed trusted comment (`... version:<v>\tplatform:<k1>,<k2>`, scripts/release.mjs) must
/// name `version` (the manifest's), our platform, and be newer than `current`: the unsigned
/// latest.json can no longer serve an old genuine build as an update.
pub fn check_binding(trusted: &str, version: &str, current: &str, platform: &str) -> ApiResult<()> {
    let field = |k: &str| trusted.split(['\t', ' ']).find_map(|f| f.strip_prefix(k));
    let signed = field("version:").ok_or_else(bad_signature)?;
    if signed != version || !is_newer(current, signed)? || !field("platform:").is_some_and(|p| p.split(',').any(|x| x == platform)) {
        return Err(bad_signature());
    }
    Ok(())
}

/// For "Crea dispositivo" (device/client.rs): the build of another platform, as the manifest
/// lists it. The signed trusted comment must name exactly the manifest's `version` and
/// `platform`, and that version must not be older than `min` (the running app): the unsigned
/// latest.json cannot put an old genuine build on a new device either.
pub fn check_binding_at_least(trusted: &str, version: &str, min: &str, platform: &str) -> ApiResult<()> {
    let field = |k: &str| trusted.split(['\t', ' ']).find_map(|f| f.strip_prefix(k));
    let signed = field("version:").ok_or_else(bad_signature)?;
    if signed != version || is_newer(signed, min)? || !field("platform:").is_some_and(|p| p.split(',').any(|x| x == platform)) {
        return Err(bad_signature());
    }
    Ok(())
}

/// Version in a signed trusted comment (`version:<v>`), if any.
pub fn signed_version(trusted: &str) -> Option<String> {
    trusted.split(['\t', ' ']).find_map(|f| f.strip_prefix("version:")).map(str::to_owned)
}

/// The published file name (the last segment of the URL).
pub fn artifact_name(url: &str) -> String {
    let name = url.rsplit('/').next().unwrap_or_default();
    if name.is_empty() || name.contains(['\\', ':']) || name == "." || name == ".." { "NiceVault-update".to_owned() } else { name.to_owned() }
}

/// The manifest URL in use (NICEVAULT_UPDATE_URL overrides the embedded one, tests).
pub fn manifest_url() -> String {
    std::env::var("NICEVAULT_UPDATE_URL").unwrap_or_else(|_| embedded_config().0)
}

/// Fetches and parses latest.json (HTTPS only, loopback HTTP for tests).
pub fn fetch_manifest(url: &str) -> ApiResult<Manifest> {
    check_manifest_url(url)?;
    parse_manifest(&fetch(url, MAX_MANIFEST, Duration::from_secs(30), &mut |_, _| {})?)
}

/// Downloads one artefact of the manifest and verifies it (origin, signature, binding).
pub fn fetch_verified(manifest_url: &str, m: &Manifest, platform: &str, pubkey: &str, min: &str, progress: &mut dyn FnMut(u64, Option<u64>)) -> ApiResult<Vec<u8>> {
    let a = m.platforms.get(platform).ok_or_else(|| invalid(format!("no {platform} build in latest.json")))?;
    check_asset_url(manifest_url, &a.url)?;
    let bytes = fetch(&a.url, MAX_ARTIFACT, Duration::from_secs(30 * 60), progress)?;
    let trusted = verify_signature(pubkey, &bytes, &a.signature)?;
    check_binding_at_least(&trusted, &m.version, min, platform)?;
    Ok(bytes)
}

// ---------------------------------------------------------------------------------------------
// network

pub(crate) fn invalid(m: impl Into<String>) -> ApiError {
    ApiError::new("update_invalid", format!("Invalid update: {}", m.into()))
}
pub(crate) fn network(e: impl std::fmt::Display) -> ApiError {
    ApiError::new("update_network", format!("Cannot reach the update server ({e})."))
}

/// (scheme, authority) of an absolute URL.
pub(crate) fn origin(url: &str) -> Option<(String, String)> {
    let u: ureq::http::Uri = url.parse().ok()?;
    Some((u.scheme_str()?.to_ascii_lowercase(), u.authority()?.as_str().to_ascii_lowercase()))
}

pub(crate) fn is_loopback(host_port: &str) -> bool {
    let host = host_port.rsplit_once(':').map_or(host_port, |(h, _)| h);
    host == "127.0.0.1" || host == "localhost" || host == "[::1]"
}

/// The manifest URL must be HTTPS (plain HTTP only on loopback, for tests).
pub fn check_manifest_url(url: &str) -> ApiResult<()> {
    match origin(url) {
        Some((s, _)) if s == "https" => Ok(()),
        Some((s, a)) if s == "http" && is_loopback(&a) => Ok(()),
        _ => Err(invalid(format!("address not allowed: {url}"))),
    }
}

/// Artefacts are fetched only from the manifest's own origin.
pub fn check_asset_url(manifest_url: &str, asset_url: &str) -> ApiResult<()> {
    match (origin(manifest_url), origin(asset_url)) {
        (Some(a), Some(b)) if a == b => Ok(()),
        _ => Err(invalid(format!("file address not allowed: {asset_url}"))),
    }
}

fn agent(https_only: bool, body_timeout: Duration) -> ureq::Agent {
    agent_with(https_only, body_timeout, 5, true)
}

/// `redirects` = 0: a 3xx is returned as is; `status_errors` = false: 4xx/5xx too (the GitHub API).
pub(crate) fn agent_with(https_only: bool, body_timeout: Duration, redirects: u32, status_errors: bool) -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .https_only(https_only)
        .http_status_as_error(status_errors)
        .max_redirects(redirects)
        .user_agent("NiceVault")
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(body_timeout))
        .build()
        .into()
}

/// GET `url`, at most `max` bytes, `progress(done, total)` while reading.
pub(crate) fn fetch(url: &str, max: u64, body_timeout: Duration, progress: &mut dyn FnMut(u64, Option<u64>)) -> ApiResult<Vec<u8>> {
    let https_only = origin(url).is_some_and(|(s, _)| s == "https");
    let resp = agent(https_only, body_timeout).get(url).call().map_err(network)?;
    read_body(resp, max, progress)
}

/// The body of `resp`, at most `max` bytes, `progress(done, total)` while reading.
pub(crate) fn read_body(mut resp: ureq::http::Response<ureq::Body>, max: u64, progress: &mut dyn FnMut(u64, Option<u64>)) -> ApiResult<Vec<u8>> {
    let total = resp.body().content_length();
    if total.is_some_and(|t| t > max) {
        return Err(invalid("file too large"));
    }
    let mut reader = resp.body_mut().as_reader();
    let mut out = Vec::with_capacity(total.unwrap_or(0).min(max) as usize);
    let mut buf = vec![0u8; 64 * 1024];
    let mut last = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(network)?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() as u64 > max {
            return Err(invalid("file too large"));
        }
        let done = out.len() as u64;
        if done - last >= 256 * 1024 {
            last = done;
            progress(done, total);
        }
    }
    progress(out.len() as u64, total);
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// in-place replacement

/// What gets replaced: the executable itself, or (macOS) the `.app` bundle around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    File(PathBuf),
    Bundle(PathBuf),
}

/// `zip` artefacts are macOS bundles: the `X.app` that contains `exe` (X.app/Contents/MacOS/bin).
pub fn target_for(exe: &Path, artifact_is_zip: bool) -> ApiResult<Target> {
    if !artifact_is_zip {
        return Ok(Target::File(exe.to_path_buf()));
    }
    let bundle = exe.parent().and_then(Path::parent).and_then(Path::parent);
    match bundle {
        Some(b) if b.extension().is_some_and(|e| e == "app") && exe.parent().is_some_and(|p| p.ends_with("Contents/MacOS")) => {
            Ok(Target::Bundle(b.to_path_buf()))
        }
        _ => Err(invalid("the app is not inside a .app bundle")),
    }
}

impl Target {
    pub fn path(&self) -> &Path {
        match self {
            Target::File(p) | Target::Bundle(p) => p,
        }
    }
    /// The folder the swap happens in can be written.
    pub fn writable(&self) -> bool {
        self.path().parent().is_some_and(crate::store::dir_writable)
    }
}

/// `<path>` + suffix (`NiceVault.exe` -> `NiceVault.exe.old`).
pub fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn remove_any(p: &Path) {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => {
            let _ = std::fs::remove_dir_all(p);
        }
        Ok(_) => {
            let _ = std::fs::remove_file(p);
        }
        Err(_) => {}
    }
}

fn sync_dir(_dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(_dir) {
        let _ = d.sync_all();
    }
}

/// Puts `staged` at `target`, keeping the current one as `<target>.old`. On failure the
/// original is back at `target`. Works on a running executable on every OS: Windows lets a
/// running exe be renamed (not overwritten), Unix keeps the old inode mapped.
pub fn swap_in(target: &Path, staged: &Path) -> std::io::Result<()> {
    let old = sibling(target, ".old");
    remove_any(&old);
    // Unix files: hard-link the current one to .old, then one atomic rename over it.
    #[cfg(unix)]
    if std::fs::symlink_metadata(target).is_ok_and(|m| m.is_file()) && std::fs::hard_link(target, &old).is_ok() {
        let r = std::fs::rename(staged, target);
        if r.is_err() {
            let _ = std::fs::remove_file(&old);
        }
        if let Some(d) = target.parent() {
            sync_dir(d);
        }
        return r;
    }
    // Everywhere else (Windows, FAT sticks, bundles): move the current one away, then the new one in.
    std::fs::rename(target, &old)?;
    if let Err(e) = std::fs::rename(staged, target) {
        let _ = std::fs::rename(&old, target);
        return Err(e);
    }
    if let Some(d) = target.parent() {
        sync_dir(d);
    }
    Ok(())
}

/// Writes `bytes` as the new executable next to `exe` and swaps it in.
pub fn replace_file(exe: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let staged = sibling(exe, ".new");
    remove_any(&staged);
    let res = (|| {
        let mut f = std::fs::File::create(&staged)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(exe).map(|m| m.permissions().mode()).unwrap_or(0o755) | 0o755;
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(mode & 0o7777))?;
        }
        swap_in(exe, &staged)
    })();
    if res.is_err() {
        remove_any(&staged);
    }
    res
}

/// Unpacks the zip's single top-level `*.app` into `dest` (which becomes that bundle).
pub fn extract_bundle(zip_bytes: &[u8], dest: &Path) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    let bad = |m: &str| Error::new(ErrorKind::InvalidData, m.to_owned());
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).map_err(|e| bad(&e.to_string()))?;
    std::fs::create_dir_all(dest)?;
    let mut seen_app = false;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| bad(&e.to_string()))?;
        let name = entry.enclosed_name().ok_or_else(|| bad("unsafe path in zip"))?;
        let mut parts = name.components();
        let top = parts.next().ok_or_else(|| bad("empty path in zip"))?;
        if !Path::new(top.as_os_str()).extension().is_some_and(|e| e == "app") {
            return Err(bad("zip does not contain an .app"));
        }
        seen_app = true;
        let rel: PathBuf = parts.collect();
        // Nothing is ever written THROUGH a symlink: every folder on the way to
        // `out` that already exists must be a real folder, and files are created new and not followed.
        let mut at = dest.to_path_buf();
        for c in rel.parent().into_iter().flat_map(Path::components) {
            at.push(c);
            if std::fs::symlink_metadata(&at).is_ok_and(|m| !m.is_dir()) {
                return Err(bad("zip writes through a symlink"));
            }
        }
        let out = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        if entry.is_symlink() {
            let mut link = String::new();
            entry.take(4096).read_to_string(&mut link)?;
            // Only links that point down from their own folder (`Versions/Current -> A`,
            // `Foo -> Versions/Current/Foo`): a chain of such links stays inside the bundle,
            // whatever order the entries come in. No `..`, no absolute target.
            let target = Path::new(&link);
            if link.is_empty() || !target.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
                return Err(bad("symlink leaves its folder"));
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, &out)?;
            continue;
        }
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.custom_flags(libc::O_NOFOLLOW);
        }
        let mut f = o.open(&out)?;
        std::io::copy(&mut entry, &mut f)?;
        f.sync_all()?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o7777))?;
        }
    }
    if !seen_app {
        return Err(bad("empty zip"));
    }
    Ok(())
}

/// Swaps the `.app` at `bundle` for the one in `zip_bytes` (bundle name kept, `<X.app>.old` kept).
pub fn replace_bundle(bundle: &Path, zip_bytes: &[u8]) -> std::io::Result<()> {
    let staged = sibling(bundle, ".new");
    remove_any(&staged);
    let res = extract_bundle(zip_bytes, &staged).and_then(|()| swap_in(bundle, &staged));
    if res.is_err() {
        remove_any(&staged);
    }
    res
}

// ---------------------------------------------------------------------------------------------
// the updater

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Prefs {
    pub check_on_start: bool,
    /// Unix ms of the last successful check.
    pub last_check: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PrefsView {
    pub check_on_start: bool,
    pub last_check: Option<u64>,
    /// true = kept in memory only (running from a stick).
    pub portable: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UpdateStatus {
    /// Automatic check not due (disabled, or done in the last 24 hours).
    Skipped,
    UpToDate { current: String },
    Available { current: String, version: String, notes: Notes, pub_date: Option<String> },
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum InstallOutcome {
    /// Replaced in place: restart to use it.
    Replaced { path: String },
    /// The app's location is not writable: the verified file is here.
    Downloaded { path: String },
}

pub struct UpdaterConfig {
    pub manifest_url: String,
    pub pubkey: String,
    pub current_version: String,
    pub platform: String,
    /// The running executable, captured at start (Linux renames make current_exe() lie later).
    pub exe: PathBuf,
    /// `None` = portable: preferences in memory only.
    pub config_dir: Option<PathBuf>,
    pub downloads_dir: Option<PathBuf>,
    /// false (dev builds) = never touch the executable, always go to Downloads.
    pub in_place: bool,
}

struct Staged {
    version: String,
    file_name: String,
    bytes: Vec<u8>,
}

pub struct Updater {
    cfg: UpdaterConfig,
    prefs: Mutex<Prefs>,
    available: Mutex<Option<(String, PlatformAsset)>>,
    staged: Mutex<Option<Staged>>,
    installed: Mutex<bool>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Updater {
    /// The real app: config from tauri.conf.json, preferences from `config_dir` (None = portable).
    pub fn for_app(config_dir: Option<PathBuf>) -> Updater {
        let (url, pubkey) = embedded_config();
        Updater::new(UpdaterConfig {
            manifest_url: std::env::var("NICEVAULT_UPDATE_URL").unwrap_or(url),
            pubkey,
            current_version: env!("CARGO_PKG_VERSION").to_owned(),
            platform: running_platform(),
            exe: app_exe().unwrap_or_default(),
            config_dir,
            downloads_dir: dirs::download_dir(),
            in_place: !cfg!(debug_assertions),
        })
    }

    pub fn new(cfg: UpdaterConfig) -> Updater {
        let prefs = cfg
            .config_dir
            .as_ref()
            .and_then(|d| std::fs::read(d.join(PREFS_FILE)).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Prefs { check_on_start: true, last_check: None });
        Updater { cfg, prefs: Mutex::new(prefs), available: Mutex::new(None), staged: Mutex::new(None), installed: Mutex::new(false) }
    }

    fn save_prefs(&self, p: &Prefs) {
        let Some(dir) = &self.cfg.config_dir else { return };
        if let Ok(json) = serde_json::to_vec_pretty(p) {
            let _ = crate::store::write_private(&dir.join(PREFS_FILE), &json);
        }
    }

    pub fn prefs(&self) -> PrefsView {
        let p = lock(&self.prefs).clone();
        PrefsView { check_on_start: p.check_on_start, last_check: p.last_check, portable: self.cfg.config_dir.is_none() }
    }

    pub fn set_check_on_start(&self, on: bool) -> PrefsView {
        {
            let mut p = lock(&self.prefs);
            p.check_on_start = on;
            self.save_prefs(&p);
        }
        self.prefs()
    }

    /// `auto`: the check at start, skipped when disabled or done less than a day ago.
    pub fn check(&self, auto: bool, now_ms: u64) -> ApiResult<UpdateStatus> {
        if auto {
            let p = lock(&self.prefs).clone();
            if !p.check_on_start || p.last_check.is_some_and(|t| now_ms.saturating_sub(t) < DAY_MS) {
                return Ok(UpdateStatus::Skipped);
            }
        }
        check_manifest_url(&self.cfg.manifest_url)?;
        let bytes = fetch(&self.cfg.manifest_url, MAX_MANIFEST, Duration::from_secs(30), &mut |_, _| {})?;
        let m = parse_manifest(&bytes)?;
        {
            let mut p = lock(&self.prefs);
            p.last_check = Some(now_ms);
            self.save_prefs(&p);
        }
        let current = self.cfg.current_version.clone();
        let asset = m.platforms.get(&self.cfg.platform).cloned();
        match asset {
            Some(a) if is_newer(&current, &m.version)? => {
                check_asset_url(&self.cfg.manifest_url, &a.url)?;
                *lock(&self.available) = Some((m.version.clone(), a));
                Ok(UpdateStatus::Available { current, notes: m.notes(), version: m.version, pub_date: m.pub_date })
            }
            _ => {
                *lock(&self.available) = None;
                Ok(UpdateStatus::UpToDate { current })
            }
        }
    }

    /// Downloads and verifies the update found by the last `check`. Nothing is written yet.
    pub fn download(&self, progress: &mut dyn FnMut(u64, Option<u64>)) -> ApiResult<()> {
        let (version, asset) =
            lock(&self.available).clone().ok_or_else(|| ApiError::new("update_none", "No update to download."))?;
        let bytes = fetch(&asset.url, MAX_ARTIFACT, Duration::from_secs(30 * 60), progress)?;
        let trusted = verify_signature(&self.cfg.pubkey, &bytes, &asset.signature)?;
        check_binding(&trusted, &version, &self.cfg.current_version, &self.cfg.platform)?;
        let file_name = artifact_name(&asset.url);
        *lock(&self.staged) = Some(Staged { version, file_name, bytes });
        Ok(())
    }

    /// Puts the downloaded update in place (or in Downloads when that is not possible).
    pub fn install(&self) -> ApiResult<InstallOutcome> {
        let staged = lock(&self.staged).take().ok_or_else(|| ApiError::new("update_none", "Download the update first."))?;
        let is_zip = staged.file_name.to_ascii_lowercase().ends_with(".zip");
        let target = target_for(&self.cfg.exe, is_zip);
        if self.cfg.in_place
            && let Ok(t) = &target
            && t.writable()
        {
            let r = match t {
                Target::File(p) => replace_file(p, &staged.bytes),
                Target::Bundle(p) => replace_bundle(p, &staged.bytes),
            };
            return match r {
                Ok(()) => {
                    *lock(&self.installed) = true;
                    Ok(InstallOutcome::Replaced { path: t.path().to_string_lossy().into_owned() })
                }
                Err(e) => {
                    *lock(&self.staged) = Some(staged);
                    Err(ApiError::new("update_install", format!("Update not installed, the app is unchanged ({e}).")))
                }
            };
        }
        self.save_to_downloads(&staged)
    }

    fn save_to_downloads(&self, s: &Staged) -> ApiResult<InstallOutcome> {
        let dir = self.cfg.downloads_dir.clone().ok_or_else(|| ApiError::new("update_install", "Downloads folder not found."))?;
        std::fs::create_dir_all(&dir).map_err(ApiError::io)?;
        let name = match s.file_name.rsplit_once('.') {
            Some((stem, ext)) => format!("{stem}-{}.{ext}", s.version),
            None => format!("{}-{}", s.file_name, s.version),
        };
        let dest = dir.join(name);
        let tmp = sibling(&dest, ".part");
        std::fs::write(&tmp, &s.bytes).and_then(|()| std::fs::rename(&tmp, &dest)).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            ApiError::io(e)
        })?;
        #[cfg(unix)]
        if !dest.extension().is_some_and(|e| e == "zip") {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
        }
        Ok(InstallOutcome::Downloaded { path: dest.to_string_lossy().into_owned() })
    }

    /// Starts the updated app (same path as this one). The caller then exits.
    pub fn spawn_updated(&self) -> ApiResult<()> {
        if !*lock(&self.installed) {
            return Err(ApiError::new("update_none", "No update installed."));
        }
        std::process::Command::new(&self.cfg.exe)
            .spawn()
            .map(|_| ())
            .map_err(|e| ApiError::new("update_restart", format!("Restart failed: open NiceVault again by hand ({e}).")))
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Tauri commands (contract: app/ui/src/lib/api.ts)

pub mod cmd {
    use super::*;
    use std::sync::Arc;
    use tauri::{Emitter, State};

    type St<'a> = State<'a, Arc<Updater>>;

    async fn blocking<T: Send + 'static>(st: &St<'_>, f: impl FnOnce(&Updater) -> ApiResult<T> + Send + 'static) -> ApiResult<T> {
        let u = st.inner().clone();
        tauri::async_runtime::spawn_blocking(move || f(&u))
            .await
            .map_err(|e| ApiError::new("error", format!("Internal error: {e}")))?
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn update_check(st: St<'_>, auto: Option<bool>) -> ApiResult<UpdateStatus> {
        let auto = auto.unwrap_or(false);
        if auto && cfg!(debug_assertions) && std::env::var_os("NICEVAULT_UPDATE_URL").is_none() {
            return Ok(UpdateStatus::Skipped);
        }
        blocking(&st, move |u| u.check(auto, now_ms())).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn update_download(app: tauri::AppHandle, st: St<'_>) -> ApiResult<()> {
        blocking(&st, move |u| {
            u.download(&mut |downloaded, total| {
                let _ = app.emit("update-progress", serde_json::json!({ "downloaded": downloaded, "total": total }));
            })
        })
        .await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn update_install(st: St<'_>) -> ApiResult<InstallOutcome> {
        blocking(&st, |u| u.install()).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn update_restart(app: tauri::AppHandle, st: St<'_>) -> ApiResult<()> {
        blocking(&st, |u| u.spawn_updated()).await?;
        // RunEvent::Exit locks every volume before the process ends.
        app.exit(0);
        Ok(())
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn update_prefs_get(st: St<'_>) -> ApiResult<PrefsView> {
        Ok(st.prefs())
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn update_prefs_set(st: St<'_>, check_on_start: bool) -> ApiResult<PrefsView> {
        blocking(&st, move |u| Ok(u.set_check_on_start(check_on_start))).await
    }
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_config_is_https() {
        let (url, key) = embedded_config();
        assert!(url.starts_with("https://"), "{url}");
        assert!(url.ends_with("/latest.json"));
        check_manifest_url(&url).unwrap();
        let pk = b64_text(&key).unwrap();
        PublicKey::decode(&pk).unwrap();
    }

    #[test]
    fn platform_key_is_known_here() {
        assert_ne!(platform_key(), "unsupported");
    }

    #[test]
    fn semver_compare() {
        assert!(is_newer("0.1.0", "0.1.1").unwrap());
        assert!(is_newer("0.9.9", "1.0.0").unwrap());
        assert!(is_newer("1.0.0-beta.1", "1.0.0-beta.2").unwrap());
        assert!(is_newer("1.0.0-rc.1", "1.0.0").unwrap());
        assert!(!is_newer("1.0.0", "1.0.0").unwrap());
        assert!(!is_newer("1.0.0", "1.0.0-rc.1").unwrap());
        assert!(!is_newer("0.10.0", "0.9.0").unwrap());
        assert!(is_newer("1.0.0", "x").is_err());
    }

    #[test]
    fn manifest_parsing() {
        let json = br#"{"version":"0.2.0","notes":"news","notes_i18n":{"it":"novita","en":"news"},
            "pub_date":"2026-10-24T00:00:00Z","platforms":{
            "linux-x86_64":{"signature":"S1","url":"https://h/a"},
            "darwin-aarch64":{"signature":"S2","url":"https://h/m.zip"}}}"#;
        let m = parse_manifest(json).unwrap();
        assert_eq!(m.version, "0.2.0");
        assert_eq!(m.notes(), Notes { it: "novita".into(), en: "news".into() });
        assert_eq!(m.platforms["darwin-aarch64"].url, "https://h/m.zip");
        // notes only in English: Italian falls back to it
        let m = parse_manifest(br#"{"version":"1.0.0","notes":"only en","platforms":{}}"#).unwrap();
        assert_eq!(m.notes().it, "only en");
        assert!(parse_manifest(br#"{"version":"one","platforms":{}}"#).is_err());
        assert!(parse_manifest(br#"{"version":"1.0.0"}"#).is_err());
        assert!(parse_manifest(b"<html>").is_err());
    }

    #[test]
    fn url_rules() {
        check_manifest_url("https://gitlab.com/x/latest.json").unwrap();
        check_manifest_url("http://127.0.0.1:8080/latest.json").unwrap();
        assert!(check_manifest_url("http://gitlab.com/x").is_err());
        assert!(check_manifest_url("file:///etc/passwd").is_err());
        check_asset_url("https://gitlab.com/a/latest.json", "https://gitlab.com/b/bin").unwrap();
        assert!(check_asset_url("https://gitlab.com/a", "https://evil.example/bin").is_err());
        assert!(check_asset_url("https://gitlab.com/a", "http://gitlab.com/bin").is_err());
    }

    #[test]
    fn artifact_names() {
        assert_eq!(artifact_name("https://h/v1/NiceVault-macos-universal.zip"), "NiceVault-macos-universal.zip");
        assert_eq!(artifact_name("https://h/v1/NiceVault-linux-x86_64.AppImage"), "NiceVault-linux-x86_64.AppImage");
        assert_eq!(artifact_name("https://h/v1/NiceVault-linux-x86_64"), "NiceVault-linux-x86_64");
        assert_eq!(artifact_name("https://h/v1/"), "NiceVault-update");
        assert_eq!(artifact_name("https://h/a:b"), "NiceVault-update");
    }

    #[test]
    fn bundle_target_needs_app_layout() {
        let exe = Path::new("/Volumes/STICK/My NiceVault.app/Contents/MacOS/nicevault");
        assert_eq!(target_for(exe, true).unwrap(), Target::Bundle(PathBuf::from("/Volumes/STICK/My NiceVault.app")));
        assert_eq!(target_for(exe, false).unwrap(), Target::File(exe.to_path_buf()));
        assert!(target_for(Path::new("/home/u/NiceVault-linux-x86_64"), true).is_err());
    }
}
