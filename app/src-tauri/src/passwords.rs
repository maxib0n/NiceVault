use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, Job, lock_or_recover, random_id};
use nv_core::passwords::generate::{self, GenError, GenOptions};
use nv_core::passwords::import::{self, ImportError};
use nv_core::passwords::totp::{Totp, TotpCode, TotpInfo};
use nv_core::passwords::{EntryDetail, EntryPatch, EntrySummary, HistoryItem, PmError, Secret};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub const DEFAULT_CLEAR_AFTER: u64 = 30;
pub const MAX_CLEAR_AFTER: u64 = 600;
const IMPORT_BATCH: usize = 25;

impl From<PmError> for ApiError {
    fn from(e: PmError) -> Self {
        match e {
            PmError::NotFound => ApiError::not_found("Voce"),
            PmError::Invalid(m) => ApiError::new("invalid_entry", format!("Invalid entry: {m}.")),
            PmError::Totp(e) => ApiError::new("invalid_totp", format!("Invalid 2FA code: {e}.")),
            PmError::Unsupported(m) => ApiError::new(
                "unsupported_version",
                format!("This part of the entry was saved by a newer version of NiceVault and cannot be changed here ({m})."),
            ),
            PmError::Fs(e) => e.into(),
        }
    }
}

impl From<ImportError> for ApiError {
    fn from(e: ImportError) -> Self {
        match e {
            ImportError::WrongKey => ApiError::new("wrong_password", "Password o file chiave del database KeePass errati."),
            ImportError::Encrypted => {
                ApiError::new("unsupported", "Encrypted exports are not supported: export unencrypted JSON.")
            }
            ImportError::Format(m) => ApiError::new("invalid_import", format!("Not recognised ({m}).")),
        }
    }
}

impl From<GenError> for ApiError {
    fn from(e: GenError) -> Self {
        ApiError::new("invalid_options", format!("Invalid options: {e}."))
    }
}

fn secret_of(field: &str) -> ApiResult<Secret> {
    Secret::parse(field).ok_or_else(|| ApiError::new("invalid_field", format!("Campo sconosciuto: {field}")))
}

pub trait Clipboard: Send + Sync {
    fn read(&self) -> Option<String>;
    fn write(&self, text: &str) -> Result<(), String>;
    fn clear(&self);
}

pub struct TauriClipboard(pub tauri::AppHandle);

impl Clipboard for TauriClipboard {
    fn read(&self) -> Option<String> {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        self.0.clipboard().read_text().ok()
    }
    fn write(&self, text: &str) -> Result<(), String> {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        let c = self.0.clipboard();
        c.write_secret(text.to_owned()).or_else(|_| c.write_text(text.to_owned())).map_err(|e| e.to_string())
    }
    fn clear(&self) {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        if self.0.clipboard().clear().is_err() {
            let _ = self.0.clipboard().write_text(String::new());
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Copied {
    pub clear_after: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct GeneratedPw {
    pub value: String,
    pub entropy_bits: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TotpCheck {
    #[serde(flatten)]
    pub info: TotpInfo,
    #[serde(flatten)]
    pub code: TotpCode,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    Bitwarden,
    Keepass,
    Csv,
}

fn schedule_clear(clip: Arc<dyn Clipboard>, value: Zeroizing<String>, after: Duration) {
    std::thread::Builder::new()
        .name("nv-clipboard".into())
        .spawn(move || {
            std::thread::sleep(after);
            if let Some(now) = clip.read().map(Zeroizing::new)
                && *now == *value
            {
                clip.clear();
            }
        })
        .expect("spawn clipboard thread");
}

fn read_capped(p: &str, cap: usize) -> ApiResult<Vec<u8>> {
    use std::io::Read;
    let mut b = Vec::new();
    std::fs::File::open(p)?.take(cap as u64 + 1).read_to_end(&mut b)?;
    if b.len() > cap {
        return Err(ApiError::new("invalid_import", "The file is too large."));
    }
    Ok(b)
}

pub(crate) type LastCopy = Option<(String, Zeroizing<String>, Arc<dyn Clipboard>)>;

impl AppState {
    pub(crate) fn clear_copied(&self, vault: Option<&str>) {
        let mut g = lock_or_recover(&self.last_copy);
        if let Some((v, value, clip)) = g.as_ref()
            && vault.is_none_or(|x| x == v)
        {
            if clip.read().map(Zeroizing::new).is_some_and(|now| *now == **value) {
                clip.clear();
            }
            *g = None;
        }
    }

    pub fn pm_list(&self, vault: &str) -> ApiResult<Vec<EntrySummary>> {
        Ok(self.vault(vault)?.pm_list()?)
    }

    pub fn pm_search(&self, vault: &str, query: &str) -> ApiResult<Vec<EntrySummary>> {
        Ok(self.vault(vault)?.pm_search(query)?)
    }

    pub fn pm_get(&self, vault: &str, id: &str) -> ApiResult<EntryDetail> {
        Ok(self.vault(vault)?.pm_get(id)?)
    }

    pub fn pm_create(&self, vault: &str, entry: EntryPatch) -> ApiResult<EntryDetail> {
        Ok(self.vault(vault)?.pm_create(entry)?)
    }

    pub fn pm_update(&self, vault: &str, id: &str, entry: EntryPatch) -> ApiResult<EntryDetail> {
        Ok(self.vault(vault)?.pm_update(id, entry)?)
    }

    pub fn pm_delete(&self, vault: &str, id: &str) -> ApiResult<()> {
        Ok(self.vault(vault)?.pm_delete(id)?)
    }

    pub fn pm_move(&self, vault: &str, ids: &[String], folder: &str) -> ApiResult<usize> {
        Ok(self.vault(vault)?.pm_move(ids, folder)?)
    }

    pub fn pm_folder_rename(&self, vault: &str, from: &str, to: &str) -> ApiResult<usize> {
        Ok(self.vault(vault)?.pm_folder_rename(from, to)?)
    }

    pub fn pm_folder_delete(&self, vault: &str, folder: &str) -> ApiResult<usize> {
        Ok(self.vault(vault)?.pm_folder_delete(folder)?)
    }

    pub fn pm_repair(&self, vault: &str) -> ApiResult<usize> {
        Ok(self.vault(vault)?.pm_repair_imported()?)
    }

    pub fn pm_history(&self, vault: &str, id: &str) -> ApiResult<Vec<HistoryItem>> {
        Ok(self.vault(vault)?.pm_history(id)?)
    }

    pub fn pm_reveal(&self, vault: &str, id: &str, field: &str) -> ApiResult<Zeroizing<String>> {
        Ok(self.vault(vault)?.pm_reveal(id, secret_of(field)?)?)
    }

    pub fn pm_copy(&self, clip: Arc<dyn Clipboard>, vault: &str, id: &str, field: &str, clear_after: Option<u64>) -> ApiResult<Copied> {
        let value = self.pm_reveal(vault, id, field)?;
        clip.write(&value).map_err(|e| ApiError::new("clipboard", format!("The clipboard is not available: {e}")))?;
        *lock_or_recover(&self.last_copy) = Some((vault.to_owned(), value.clone(), clip.clone()));
        let after = clear_after.unwrap_or(DEFAULT_CLEAR_AFTER).min(MAX_CLEAR_AFTER);
        if after > 0 {
            schedule_clear(clip, value, Duration::from_secs(after));
        }
        Ok(Copied { clear_after: after })
    }

    pub fn pm_totp(&self, vault: &str, id: &str) -> ApiResult<TotpCode> {
        Ok(self.vault(vault)?.pm_totp(id)?)
    }

    pub fn pm_totp_check(&self, totp: &str) -> ApiResult<TotpCheck> {
        let t = Totp::parse(totp).map_err(PmError::from)?;
        Ok(TotpCheck { info: t.info(), code: t.now() })
    }

    pub fn pm_generate(&self, opts: GenOptions) -> ApiResult<GeneratedPw> {
        let g = generate::generate(&opts)?;
        Ok(GeneratedPw { value: g.value.to_string(), entropy_bits: g.entropy_bits })
    }

    pub fn pm_import(
        &self,
        vault: &str,
        kind: ImportKind,
        path: Option<String>,
        data: Option<String>,
        master_password: Option<Zeroizing<String>>,
        keyfile: Option<String>,
    ) -> ApiResult<String> {
        let v = self.vault(vault)?;
        let bytes: Zeroizing<Vec<u8>> = Zeroizing::new(match (path, data) {
            (Some(p), _) if !p.trim().is_empty() => read_capped(&p, import::MAX_KDBX)?,
            (_, Some(d)) if kind == ImportKind::Keepass => {
                use base64::Engine;
                let d = d.split_once("base64,").map_or(d.as_str(), |(_, b)| b);
                base64::engine::general_purpose::STANDARD
                    .decode(d.trim())
                    .map_err(|_| ApiError::new("invalid_import", "Invalid KeePass data (base64 expected)."))?
            }
            (_, Some(d)) => d.into_bytes(),
            _ => return Err(ApiError::new("invalid_import", "No file to import.")),
        });
        let keyfile: Option<Zeroizing<Vec<u8>>> = match keyfile.filter(|k| !k.trim().is_empty()) {
            Some(k) => Some(Zeroizing::new(read_capped(&k, import::MAX_KEYFILE)?)),
            None => None,
        };
        let job = random_id();
        let cancel = Arc::new(AtomicBool::new(false));
        lock_or_recover(&self.jobs).insert(job.clone(), Job { vault: vault.to_owned(), cancel: cancel.clone() });
        let st = self.arc();
        let id = job.clone();
        std::thread::Builder::new()
            .name("nv-pm-import".into())
            .spawn(move || {
                let mut imported = 0usize;
                let r = (|| -> ApiResult<()> {
                    st.emit("job-progress", json!({ "job": id, "done_bytes": 0, "total_bytes": 0, "current": "" }));
                    let entries = match kind {
                        ImportKind::Bitwarden => import::bitwarden_json(&bytes)?,
                        ImportKind::Csv => import::csv(&bytes)?,
                        ImportKind::Keepass => {
                            import::keepass(&bytes, master_password.as_deref().map(String::as_str), keyfile.as_deref().map(Vec::as_slice))?
                        }
                    };
                    let total = entries.len();
                    let mut last = Instant::now();
                    let mut it = entries.into_iter().peekable();
                    while it.peek().is_some() {
                        if cancel.load(Ordering::Relaxed) {
                            return Err(ApiError::cancelled());
                        }
                        let batch: Vec<_> = it.by_ref().take(IMPORT_BATCH).collect();
                        let current = batch.last().and_then(|e| e.entry.title.clone()).unwrap_or_default();
                        imported += v.pm_insert_many(batch)?.len();
                        if last.elapsed() >= Duration::from_millis(100) || imported == total {
                            last = Instant::now();
                            st.emit("job-progress", json!({ "job": id, "done_bytes": imported, "total_bytes": total, "current": current }));
                        }
                    }
                    if total == 0 {
                        st.emit("job-progress", json!({ "job": id, "done_bytes": 0, "total_bytes": 0, "current": "" }));
                    }
                    Ok(())
                })();
                lock_or_recover(&st.jobs).remove(&id);
                st.emit("job-done", json!({ "job": id, "error": r.err(), "imported": imported }));
            })
            .expect("spawn import thread");
        Ok(job)
    }
}

pub mod cmd {
    use super::*;
    use tauri::State;

    type St<'a> = State<'a, Arc<AppState>>;

    async fn blocking<T: Send + 'static>(st: &St<'_>, f: impl FnOnce(&AppState) -> ApiResult<T> + Send + 'static) -> ApiResult<T> {
        let s = st.inner().clone();
        tauri::async_runtime::spawn_blocking(move || f(&s))
            .await
            .map_err(|e| ApiError::new("error", format!("Internal error: {e}")))?
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_list(st: St<'_>, vault: String) -> ApiResult<Vec<EntrySummary>> {
        blocking(&st, move |s| s.pm_list(&vault)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_search(st: St<'_>, vault: String, query: String) -> ApiResult<Vec<EntrySummary>> {
        blocking(&st, move |s| s.pm_search(&vault, &query)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_get(st: St<'_>, vault: String, id: String) -> ApiResult<EntryDetail> {
        blocking(&st, move |s| s.pm_get(&vault, &id)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_create(st: St<'_>, vault: String, entry: EntryPatch) -> ApiResult<EntryDetail> {
        blocking(&st, move |s| s.pm_create(&vault, entry)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_update(st: St<'_>, vault: String, id: String, entry: EntryPatch) -> ApiResult<EntryDetail> {
        blocking(&st, move |s| s.pm_update(&vault, &id, entry)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_delete(st: St<'_>, vault: String, id: String) -> ApiResult<()> {
        blocking(&st, move |s| s.pm_delete(&vault, &id)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_move(st: St<'_>, vault: String, ids: Vec<String>, folder: String) -> ApiResult<usize> {
        blocking(&st, move |s| s.pm_move(&vault, &ids, &folder)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_folder_rename(st: St<'_>, vault: String, from: String, to: String) -> ApiResult<usize> {
        blocking(&st, move |s| s.pm_folder_rename(&vault, &from, &to)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_folder_delete(st: St<'_>, vault: String, folder: String) -> ApiResult<usize> {
        blocking(&st, move |s| s.pm_folder_delete(&vault, &folder)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_repair(st: St<'_>, vault: String) -> ApiResult<usize> {
        blocking(&st, move |s| s.pm_repair(&vault)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_icons_refresh(st: St<'_>, vault: String) -> ApiResult<usize> {
        blocking(&st, move |s| s.pm_icons_refresh(&vault)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_history(st: St<'_>, vault: String, id: String) -> ApiResult<Vec<HistoryItem>> {
        blocking(&st, move |s| s.pm_history(&vault, &id)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_reveal(st: St<'_>, vault: String, id: String, field: String) -> ApiResult<String> {
        blocking(&st, move |s| s.pm_reveal(&vault, &id, &field).map(|z| z.to_string())).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_copy(
        app: tauri::AppHandle,
        st: St<'_>,
        vault: String,
        id: String,
        field: String,
        clear_after: Option<u64>,
    ) -> ApiResult<Copied> {
        let clip: Arc<dyn Clipboard> = Arc::new(TauriClipboard(app));
        blocking(&st, move |s| s.pm_copy(clip, &vault, &id, &field, clear_after)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_totp(st: St<'_>, vault: String, id: String) -> ApiResult<TotpCode> {
        blocking(&st, move |s| s.pm_totp(&vault, &id)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_totp_check(st: St<'_>, totp: String) -> ApiResult<TotpCheck> {
        let totp = Zeroizing::new(totp);
        blocking(&st, move |s| s.pm_totp_check(&totp)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_generate(st: St<'_>, opts: Option<GenOptions>) -> ApiResult<GeneratedPw> {
        blocking(&st, move |s| s.pm_generate(opts.unwrap_or_default())).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn pm_import(
        st: St<'_>,
        vault: String,
        kind: ImportKind,
        path: Option<String>,
        data: Option<String>,
        master_password: Option<String>,
        keyfile: Option<String>,
    ) -> ApiResult<String> {
        let master_password = master_password.map(Zeroizing::new);
        blocking(&st, move |s| s.pm_import(&vault, kind, path, data, master_password, keyfile)).await
    }
}

