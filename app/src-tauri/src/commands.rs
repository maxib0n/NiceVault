use crate::appearance::{AppearanceState, Settings, SettingsPatch, Visibility};
use crate::emblem::{Emblem, EmblemState};
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, BackendInfo, Mounted, SlotInfo, VaultInfo, VaultStats};
use crate::store::RecentEntry;
use nv_fs::{Node, NodeId};
use secrecy::SecretString;
use std::sync::Arc;
use tauri::State;

type St<'a> = State<'a, Arc<AppState>>;

async fn blocking<T: Send + 'static>(st: &St<'_>, f: impl FnOnce(&AppState) -> ApiResult<T> + Send + 'static) -> ApiResult<T> {
    let s = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || f(&s))
        .await
        .map_err(|e| ApiError::new("error", format!("Internal error: {e}")))?
}

fn secret(s: String) -> SecretString {
    SecretString::from(s)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn vault_create(st: St<'_>, path: String, password: String, kdf: Option<String>, fixed_size: Option<u64>) -> ApiResult<VaultInfo> {
    let pw = secret(password);
    blocking(&st, move |s| s.vault_create_sized(&path, pw, kdf.as_deref(), fixed_size)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn vault_open(st: St<'_>, path: String, password: String) -> ApiResult<VaultInfo> {
    let pw = secret(password);
    blocking(&st, move |s| s.vault_open(&path, pw)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn vault_lock(st: St<'_>, vault: String, reason: Option<String>) -> ApiResult<()> {
    blocking(&st, move |s| s.vault_lock(&vault, reason.as_deref().unwrap_or("user"))).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn vault_list_open(st: St<'_>) -> ApiResult<Vec<VaultInfo>> {
    blocking(&st, |s| Ok(s.vault_list_open())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn vault_stats(st: St<'_>, vault: String) -> ApiResult<VaultStats> {
    blocking(&st, move |s| s.vault_stats(&vault)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn vault_emblem_get(st: St<'_>, vault: String) -> ApiResult<EmblemState> {
    blocking(&st, move |s| s.vault_emblem_get(&vault)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn vault_emblem_set(
    st: St<'_>,
    vault: String,
    emblem: Emblem,
    show_when_locked: Option<bool>,
    locked_image_base64: Option<String>,
    emblem_frame: Option<String>,
) -> ApiResult<EmblemState> {
    blocking(&st, move |s| s.vault_emblem_set(&vault, emblem, show_when_locked.unwrap_or(true), locked_image_base64, emblem_frame)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn vault_appearance_get(st: St<'_>, vault: String) -> ApiResult<AppearanceState> {
    blocking(&st, move |s| s.vault_appearance_get(&vault)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn vault_appearance_set(
    st: St<'_>,
    vault: String,
    accent: Option<String>,
    emblem: Option<Emblem>,
    visibility: Option<Visibility>,
    locked_image_base64: Option<String>,
    emblem_frame: Option<String>,
) -> ApiResult<AppearanceState> {
    blocking(&st, move |s| s.vault_appearance_set(&vault, accent, emblem, visibility, locked_image_base64, emblem_frame)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn recent_list(st: St<'_>) -> ApiResult<Vec<RecentEntry>> {
    blocking(&st, |s| Ok(s.recent_list())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn recent_forget(st: St<'_>, path: String) -> ApiResult<Vec<RecentEntry>> {
    blocking(&st, move |s| s.recent_forget(&path)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn default_volume_dir(st: St<'_>) -> ApiResult<String> {
    blocking(&st, |s| Ok(s.default_volume_dir())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_get(st: St<'_>, vault: Option<String>) -> ApiResult<Settings> {
    blocking(&st, move |s| s.settings_get(vault.as_deref())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_set(st: St<'_>, settings: SettingsPatch, vault: Option<String>) -> ApiResult<Settings> {
    blocking(&st, move |s| s.settings_set(settings, vault.as_deref())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn fs_mkdir(st: St<'_>, vault: String, parent: NodeId, name: String) -> ApiResult<Node> {
    blocking(&st, move |s| s.fs_mkdir(&vault, parent, &name)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn fs_rename(st: St<'_>, vault: String, node: NodeId, new_parent: NodeId, new_name: String) -> ApiResult<()> {
    blocking(&st, move |s| s.fs_rename(&vault, node, new_parent, &new_name)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn fs_delete(st: St<'_>, vault: String, nodes: Vec<NodeId>) -> ApiResult<String> {
    blocking(&st, move |s| crate::delete::fs_delete(s, &vault, nodes)).await
}

#[cfg(desktop)]
#[tauri::command(rename_all = "snake_case")]
pub async fn fs_import(st: St<'_>, vault: String, parent: NodeId, host_paths: Vec<String>) -> ApiResult<String> {
    blocking(&st, move |s| crate::jobs::fs_import(s, &vault, parent, host_paths)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn import_size(paths: Vec<String>) -> ApiResult<u64> {
    tauri::async_runtime::spawn_blocking(move || crate::jobs::import_size(&paths)).await.map_err(|e| ApiError::new("error", format!("Internal error: {e}")))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn fs_export(st: St<'_>, vault: String, nodes: Vec<NodeId>, dest_dir: String) -> ApiResult<String> {
    blocking(&st, move |s| crate::jobs::fs_export(s, &vault, nodes, &dest_dir)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn fs_open_external(st: St<'_>, vault: String, node: NodeId) -> ApiResult<crate::state::OpenedExternal> {
    blocking(&st, move |s| s.fs_open_external(&vault, node)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn job_cancel(st: St<'_>, job: String) -> ApiResult<()> {
    st.job_cancel(&job);
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mount_available(st: St<'_>) -> ApiResult<Vec<BackendInfo>> {
    blocking(&st, |s| Ok(s.mount_available())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mount(st: St<'_>, vault: String) -> ApiResult<Mounted> {
    blocking(&st, move |s| s.mount(&vault)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn unmount(st: St<'_>, vault: String) -> ApiResult<()> {
    blocking(&st, move |s| s.unmount(&vault)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn slots_list(st: St<'_>, vault: String) -> ApiResult<Vec<SlotInfo>> {
    blocking(&st, move |s| s.slots_list(&vault)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn slot_add_password(st: St<'_>, vault: String, password: String, label: Option<String>) -> ApiResult<Vec<SlotInfo>> {
    let pw = secret(password);
    blocking(&st, move |s| s.slot_add_password(&vault, pw, label)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn slot_change_password(
    st: St<'_>,
    vault: String,
    slot: usize,
    old_password: String,
    new_password: String,
    label: Option<String>,
) -> ApiResult<Vec<SlotInfo>> {
    let (old, new) = (secret(old_password), secret(new_password));
    blocking(&st, move |s| s.slot_change_password(&vault, slot, old, new, label)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn slot_remove(st: St<'_>, vault: String, slot: usize) -> ApiResult<Vec<SlotInfo>> {
    blocking(&st, move |s| s.slot_remove(&vault, slot)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn slot_rotate_key(st: St<'_>, vault: String, slot: usize, password: String) -> ApiResult<Vec<SlotInfo>> {
    let password = secret(password);
    blocking(&st, move |s| s.slot_rotate_key(&vault, slot, password)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn vault_upgrade_format(st: St<'_>, vault: String) -> ApiResult<VaultInfo> {
    blocking(&st, move |s| s.vault_upgrade_format(&vault)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn vault_accept_rollback(st: St<'_>, vault: String) -> ApiResult<VaultInfo> {
    blocking(&st, move |s| s.vault_accept_rollback(&vault)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn vault_touch(st: St<'_>, vault: String) -> ApiResult<()> {
    blocking(&st, move |s| s.vault_touch(&vault)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn autolock_status(st: St<'_>) -> ApiResult<Vec<crate::autolock::AutoLockStatus>> {
    blocking(&st, |s| Ok(s.autolock_status())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn slot_add_duress(st: St<'_>, vault: String, password: String, label: Option<String>) -> ApiResult<Vec<SlotInfo>> {
    let pw = secret(password);
    blocking(&st, move |s| s.slot_add_duress(&vault, pw, label)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn app_settings_get(st: St<'_>) -> ApiResult<crate::global::AppSettings> {
    blocking(&st, |s| Ok(s.app_settings_get())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn app_settings_set(
    st: St<'_>,
    app: tauri::AppHandle,
    webview: tauri::WebviewWindow,
    patch: crate::global::AppSettingsPatch,
) -> ApiResult<crate::global::AppSettings> {
    let rescale = patch.ui_scale.is_some() && cfg!(desktop);
    let (tray_mode, relabel) = (patch.start_minimized.is_some() || patch.close_to_tray.is_some(), patch.language.is_some());
    let unforce = patch.start_minimized.is_some();
    let out = blocking(&st, move |s| s.app_settings_set(patch)).await?;
    if tray_mode {
        if unforce && let Some(t) = tauri::Manager::try_state::<crate::tray::Tray>(&app) {
            t.unforce();
        }
        crate::tray::sync(&app);
    } else if relabel {
        crate::tray::refresh(&app);
    }
    if rescale {
        let _ = webview.set_zoom(f64::from(out.ui_scale) / 100.0);
    }
    Ok(out)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn app_info(st: St<'_>) -> ApiResult<crate::global::AppInfo> {
    blocking(&st, |s| Ok(s.app_info())).await
}
