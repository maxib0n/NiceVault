use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const FILE: &str = "biometric.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub slot: usize,
    pub blob: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stale: bool,
}

pub type Records = BTreeMap<String, Record>;

pub fn tag(volume_id: &[u8; 16]) -> String {
    volume_id.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn alias(tag: &str) -> String {
    format!("nv-bio-{tag}")
}

pub fn load(dir: &Path) -> Records {
    std::fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(dir: &Path, r: &Records) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{FILE}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(r).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, dir.join(FILE))
}

#[cfg(mobile)]
pub use cmd::*;

#[cfg(mobile)]
mod cmd {
    use super::*;
    use crate::error::{ApiError, ApiResult};
    use crate::state::{AppState, VaultInfo};
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as B64;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tauri::{AppHandle, State};
    use zeroize::Zeroizing;

    static LOCK: Mutex<()> = Mutex::new(());

    fn dir() -> ApiResult<PathBuf> {
        std::env::var_os("NICEVAULT_CONFIG_DIR").map(PathBuf::from).ok_or_else(|| ApiError::new("io", "no config folder"))
    }

    fn update<T>(f: impl FnOnce(&mut Records) -> T) -> ApiResult<T> {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = dir()?;
        let mut r = load(&d);
        let out = f(&mut r);
        save(&d, &r)?;
        Ok(out)
    }

    fn get(tag: &str) -> Option<Record> {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        dir().ok().and_then(|d| load(&d).remove(tag))
    }

    #[derive(Serialize, Deserialize, Default)]
    pub struct BioStatus {
        #[serde(default)]
        available: bool,
        #[serde(default)]
        kind: String,
        #[serde(default)]
        reason: String,
        #[serde(default)]
        device: String,
        #[serde(default)]
        enabled: bool,
        #[serde(default)]
        stale: bool,
    }

    #[derive(Deserialize, Serialize, Clone)]
    pub struct Prompt {
        title: String,
        cancel: String,
    }

    #[derive(Serialize)]
    struct NativeArgs<'a> {
        alias: &'a str,
        data: &'a str,
        title: &'a str,
        cancel: &'a str,
    }

    #[derive(Deserialize)]
    struct NativeData {
        data: String,
    }

    fn native(app: &AppHandle, cmd: &str, alias: &str, data: &str, p: Option<&Prompt>) -> ApiResult<Zeroizing<String>> {
        let args = NativeArgs { alias, data, title: p.map_or("", |p| &p.title), cancel: p.map_or("", |p| &p.cancel) };
        let r: NativeData = crate::mobile::bridge(app)?.run_mobile_plugin(cmd, args).map_err(crate::mobile::native_err)?;
        Ok(Zeroizing::new(r.data))
    }

    fn native_delete(app: &AppHandle, tag: &str) {
        if let Ok(h) = crate::mobile::bridge(app) {
            let _ = h.run_mobile_plugin::<serde_json::Value>("bioDelete", NativeArgs { alias: &alias(tag), data: "", title: "", cancel: "" });
        }
    }

    fn volume_id(path: &str) -> ApiResult<[u8; 16]> {
        Ok(nv_core::peek(Path::new(path))?.volume_id)
    }

    fn open_vault(st: &AppState, path: &str) -> Option<Arc<nv_core::Vault>> {
        let id = st.vault_list_open().into_iter().find(|i| i.path == path)?.id;
        st.vault(&id).ok()
    }

    fn is_device_slot(v: &nv_core::Vault, slot: usize) -> bool {
        v.slots().iter().any(|s| s.index == slot && s.kind == "device")
    }

    fn status(app: &AppHandle, st: &AppState, path: &str) -> ApiResult<BioStatus> {
        let mut s: BioStatus = crate::mobile::bridge(app)?.run_mobile_plugin("bioStatus", serde_json::json!({})).map_err(crate::mobile::native_err)?;
        let tag = tag(&volume_id(path)?);
        let mut rec = get(&tag);
        if let (Some(r), Some(v)) = (&rec, open_vault(st, path))
            && !r.stale
            && !is_device_slot(&v, r.slot)
        {
            native_delete(app, &tag);
            update(|m| m.remove(&tag))?;
            rec = None;
        }
        s.enabled = rec.as_ref().is_some_and(|r| !r.stale);
        s.stale = rec.is_some_and(|r| r.stale);
        Ok(s)
    }

    fn mark_stale(app: &AppHandle, tag: &str) {
        native_delete(app, tag);
        let _ = update(|m| {
            if let Some(r) = m.get_mut(tag) {
                r.stale = true;
            }
        });
    }

    async fn blocking<T: Send + 'static>(f: impl FnOnce() -> ApiResult<T> + Send + 'static) -> ApiResult<T> {
        tauri::async_runtime::spawn_blocking(f).await.map_err(|e| ApiError::new("error", format!("Internal error: {e}")))?
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn bio_status(app: AppHandle, st: State<'_, Arc<AppState>>, path: String) -> ApiResult<BioStatus> {
        let st = st.inner().clone();
        blocking(move || status(&app, &st, &path)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn bio_enable(app: AppHandle, st: State<'_, Arc<AppState>>, vault: String, label: String, prompt: Prompt) -> ApiResult<BioStatus> {
        let st = st.inner().clone();
        blocking(move || {
            let v = st.vault(&vault)?;
            let path = st.vault_list_open().into_iter().find(|i| i.id == vault).ok_or_else(ApiError::locked)?.path;
            let tag = tag(&v.volume_id().ok_or_else(ApiError::locked)?);
            if let Some(old) = get(&tag) {
                if is_device_slot(&v, old.slot) {
                    let _ = v.remove_slot(old.slot);
                }
                native_delete(&app, &tag);
                update(|m| m.remove(&tag))?;
            }
            let (slot, kek) = v.add_device(Some(&label))?;
            let data = Zeroizing::new(B64.encode(*kek));
            drop(kek);
            match native(&app, "bioWrap", &alias(&tag), &data, Some(&prompt)) {
                Ok(blob) => update(|m| m.insert(tag, Record { slot, blob: blob.to_string(), stale: false }))?,
                Err(e) => {
                    let _ = v.remove_slot(slot);
                    return Err(e);
                }
            };
            status(&app, &st, &path)
        })
        .await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn bio_disable(app: AppHandle, st: State<'_, Arc<AppState>>, vault: String) -> ApiResult<BioStatus> {
        let st = st.inner().clone();
        blocking(move || {
            let v = st.vault(&vault)?;
            let path = st.vault_list_open().into_iter().find(|i| i.id == vault).ok_or_else(ApiError::locked)?.path;
            let tag = tag(&v.volume_id().ok_or_else(ApiError::locked)?);
            if let Some(r) = get(&tag)
                && is_device_slot(&v, r.slot)
            {
                v.remove_slot(r.slot)?;
            }
            native_delete(&app, &tag);
            update(|m| m.remove(&tag))?;
            status(&app, &st, &path)
        })
        .await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn bio_unlock(app: AppHandle, st: State<'_, Arc<AppState>>, path: String, prompt: Prompt) -> ApiResult<VaultInfo> {
        let st = st.inner().clone();
        blocking(move || {
            let tag = tag(&volume_id(&path)?);
            let rec = get(&tag).filter(|r| !r.stale).ok_or_else(|| ApiError::new("bio_off", "Biometric unlock is off for this volume."))?;
            let data = match native(&app, "bioUnwrap", &alias(&tag), &rec.blob, Some(&prompt)) {
                Ok(d) => d,
                Err(e) => {
                    if e.code == "bio_invalidated" {
                        mark_stale(&app, &tag);
                    }
                    return Err(e);
                }
            };
            let bytes = Zeroizing::new(B64.decode(data.as_bytes()).unwrap_or_default());
            drop(data);
            let kek: Zeroizing<[u8; 32]> = Zeroizing::new(bytes.as_slice().try_into().map_err(|_| ApiError::new("bio_failed", "bad key"))?);
            match st.vault_open_by(&path, |p| Ok(nv_core::Vault::open_with_device(p, rec.slot, &kek)?)) {
                Err(e) if e.code == "wrong_password" => {
                    mark_stale(&app, &tag);
                    Err(ApiError::new("bio_revoked", "This phone's key was removed from the volume."))
                }
                r => r,
            }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip_and_bad_files_read_as_none() {
        let t = tempfile::tempdir().unwrap();
        assert!(load(t.path()).is_empty());
        let mut r = Records::new();
        let id = [0xab; 16];
        r.insert(tag(&id), Record { slot: 2, blob: "AAAA".into(), stale: false });
        r.insert(tag(&[1; 16]), Record { slot: 1, blob: "BBBB".into(), stale: true });
        save(t.path(), &r).unwrap();
        assert_eq!(load(t.path()), r);
        let raw = std::fs::read_to_string(t.path().join(FILE)).unwrap();
        assert_eq!(raw.matches("stale").count(), 1, "stale is written only when true");
        assert_eq!(alias(&tag(&id)), format!("nv-bio-{}", "ab".repeat(16)));
        std::fs::write(t.path().join(FILE), b"{ not json").unwrap();
        assert!(load(t.path()).is_empty());
    }
}
