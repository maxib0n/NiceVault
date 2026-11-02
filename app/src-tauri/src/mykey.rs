use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, SlotInfo, VaultInfo, lock_or_recover};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use nv_core::{Identity, KdfPreset, OpenError, Recipient, Vault};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FILE: &str = "my-key.json";
const RECIPIENTS_KEY: &str = "nicevault.recipients";
const MAX_PUB_FILE: u64 = 64 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub struct Stored {
    pub public: String,
    pub created_ms: u64,
    pub key_file: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct MyKey {
    pub public: Option<String>,
    pub fingerprint: Option<String>,
    pub created_ms: Option<u64>,
    pub persisted: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RecipientCheck {
    pub public: String,
    pub fingerprint: String,
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn invalid_recipient() -> ApiError {
    ApiError::new("invalid_recipient", "This is not a valid NiceVault public key (nvpk1…).")
}

fn secret_key_file() -> ApiError {
    ApiError::new("secret_key_file", "This is a secret key file (.nvkey): ask for the public key (nvpk1… or .nvpub) instead.")
}

fn no_key() -> ApiError {
    ApiError::new("no_key", "There is no personal key on this computer.")
}

pub fn parse_recipient(text: &str) -> ApiResult<Recipient> {
    let s: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if s.is_empty() {
        return Err(invalid_recipient());
    }
    Recipient::parse(&s).map_err(|_| invalid_recipient())
}

fn check(r: &Recipient) -> RecipientCheck {
    RecipientCheck { public: r.to_string(), fingerprint: hex(&r.fingerprint()) }
}

pub fn known_recipients(v: &Vault) -> Vec<(Recipient, Option<String>)> {
    let map = recipient_map(v);
    v.slots()
        .into_iter()
        .filter(|s| s.kind == "recipient")
        .filter_map(|s| {
            let r = map.get(s.fingerprint.as_deref()?).and_then(|t| Recipient::parse(t).ok())?;
            (hex(&r.fingerprint()) == s.fingerprint.clone()?).then_some((r, s.label))
        })
        .collect()
}

fn recipient_map(v: &Vault) -> BTreeMap<String, String> {
    v.meta_get(RECIPIENTS_KEY).ok().flatten().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn remember_recipient(v: &Vault, r: &Recipient) {
    let mut m = recipient_map(v);
    m.insert(hex(&r.fingerprint()), r.to_string());
    if let Ok(b) = serde_json::to_vec(&m) {
        let _ = v.meta_set(RECIPIENTS_KEY, Some(&b));
    }
}

pub fn forget_recipient(v: &Vault, fpr: &str) {
    let mut m = recipient_map(v);
    if m.remove(fpr).is_some() {
        let b = serde_json::to_vec(&m).ok();
        let _ = v.meta_set(RECIPIENTS_KEY, if m.is_empty() { None } else { b.as_deref() });
    }
}

fn kdf_of(kdf: Option<&str>) -> KdfPreset {
    match kdf {
        Some("fast") if cfg!(debug_assertions) => KdfPreset::Fast,
        _ => KdfPreset::Strong,
    }
}

fn write_new(dest: &Path, data: &[u8]) -> ApiResult<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    let mut f = o.open(dest).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => ApiError::new("exists", "A file with this name already exists."),
        _ => ApiError::io(e),
    })?;
    f.write_all(data).and_then(|_| f.sync_all()).map_err(ApiError::io)
}

impl AppState {
    fn my_key_file(&self) -> Option<PathBuf> {
        lock_or_recover(&self.store).locations().config_dir.as_ref().map(|d| d.join(FILE))
    }

    fn my_key_load(&self) -> ApiResult<Option<Stored>> {
        let Some(f) = self.my_key_file() else { return Ok(lock_or_recover(&self.my_key_mem).clone()) };
        match std::fs::read(&f) {
            Ok(b) => serde_json::from_slice::<Stored>(&b).map(Some).map_err(|_| ApiError::unsupported_version()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ApiError::io(e)),
        }
    }

    fn my_key_save(&self, s: Option<Stored>) -> ApiResult<()> {
        let Some(f) = self.my_key_file() else {
            *lock_or_recover(&self.my_key_mem) = s;
            return Ok(());
        };
        match s {
            Some(s) => {
                let b = serde_json::to_vec_pretty(&s).map_err(|e| ApiError::new("error", e.to_string()))?;
                crate::store::write_private(&f, &b).map_err(ApiError::io)
            }
            None => match std::fs::remove_file(&f) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(ApiError::io(e)),
                _ => Ok(()),
            },
        }
    }

    fn my_key_view(&self, s: Option<&Stored>) -> MyKey {
        let fpr = s.and_then(|s| Recipient::parse(&s.public).ok()).map(|r| hex(&r.fingerprint()));
        MyKey {
            public: s.map(|s| s.public.clone()),
            fingerprint: fpr,
            created_ms: s.map(|s| s.created_ms),
            persisted: self.my_key_file().is_some(),
        }
    }

    pub fn my_key_get(&self) -> ApiResult<MyKey> {
        Ok(self.my_key_view(self.my_key_load()?.as_ref()))
    }

    pub fn my_key_create(&self, password: SecretString, kdf: Option<&str>) -> ApiResult<MyKey> {
        if password.expose_secret().is_empty() {
            return Err(ApiError::new("invalid_password", "A password is needed."));
        }
        if self.my_key_load()?.is_some() {
            return Err(ApiError::new("exists", "There is already a personal key on this computer."));
        }
        let id = Identity::generate();
        let bytes = id.to_bytes_with(&password, kdf_of(kdf));
        drop(password);
        let s = Stored { public: id.recipient().to_string(), created_ms: crate::store::now_ms(), key_file: B64.encode(bytes), extra: Default::default() };
        self.my_key_save(Some(s.clone()))?;
        Ok(self.my_key_view(Some(&s)))
    }

    pub fn my_key_import(&self, path: &str, password: SecretString) -> ApiResult<MyKey> {
        let bytes = std::fs::read(path)?;
        if !Identity::is_key_file(&bytes) {
            return Err(ApiError::new("not_a_key", "This file is not a NiceVault key file (.nvkey)."));
        }
        let id = Identity::from_bytes(&bytes, &password).map_err(|e| match e {
            OpenError::WrongPassword => ApiError::new("wrong_password", "Wrong password for this key file."),
            OpenError::NotAVault | OpenError::Corrupt(_) => ApiError::new("not_a_key", "This file is not a NiceVault key file (.nvkey)."),
            e => e.into(),
        })?;
        drop(password);
        let public = id.recipient().to_string();
        if let Some(cur) = self.my_key_load()? {
            if cur.public == public {
                return Ok(self.my_key_view(Some(&cur)));
            }
            return Err(ApiError::new("exists", "There is already a different personal key on this computer."));
        }
        let s = Stored { public, created_ms: crate::store::now_ms(), key_file: B64.encode(&bytes), extra: Default::default() };
        self.my_key_save(Some(s.clone()))?;
        Ok(self.my_key_view(Some(&s)))
    }

    pub fn my_key_export(&self, dest: &str) -> ApiResult<()> {
        let s = self.my_key_load()?.ok_or_else(no_key)?;
        let bytes = B64.decode(&s.key_file).map_err(|_| ApiError::new("corrupt", "The stored key is damaged."))?;
        write_new(Path::new(dest), &bytes)
    }

    pub fn my_key_export_public(&self, dest: &str) -> ApiResult<()> {
        let s = self.my_key_load()?.ok_or_else(no_key)?;
        write_new(Path::new(dest), format!("{}\n", s.public).as_bytes())
    }

    pub fn my_key_remove(&self) -> ApiResult<MyKey> {
        self.my_key_save(None)?;
        Ok(self.my_key_view(None))
    }

    pub fn recipient_check(&self, text: &str) -> ApiResult<RecipientCheck> {
        if Identity::is_key_file(text.trim_start().as_bytes()) {
            return Err(secret_key_file());
        }
        Ok(check(&parse_recipient(text)?))
    }

    pub fn recipient_read_file(&self, path: &str) -> ApiResult<RecipientCheck> {
        let md = std::fs::metadata(path)?;
        if !md.is_file() {
            return Err(invalid_recipient());
        }
        let mut head = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(std::fs::File::open(path)?, MAX_PUB_FILE), &mut head)?;
        if Identity::is_key_file(&head) {
            return Err(secret_key_file());
        }
        let text = String::from_utf8_lossy(&head);
        let token = text.split(|c: char| c.is_whitespace() || c == '"' || c == '\'').find(|t| t.starts_with("nvpk1"));
        Ok(check(&parse_recipient(token.unwrap_or(&text))?))
    }

    pub fn slot_add_recipient(&self, id: &str, recipient: &str, label: Option<String>) -> ApiResult<Vec<SlotInfo>> {
        let r = parse_recipient(recipient)?;
        let v = self.vault(id)?;
        let fpr = hex(&r.fingerprint());
        if v.slots().iter().any(|s| s.fingerprint.as_deref() == Some(fpr.as_str())) {
            return Err(ApiError::new("recipient_exists", "This person can already open the volume."));
        }
        let label = label.map(|l| l.trim().to_string()).filter(|l| !l.is_empty());
        v.add_recipient(&r, label.as_deref())?;
        remember_recipient(&v, &r);
        self.slots_list(id)
    }

    pub fn vault_open_with_key(&self, path: &str, password: SecretString) -> ApiResult<VaultInfo> {
        let s = self.my_key_load()?.ok_or_else(no_key)?;
        let bytes = B64.decode(&s.key_file).map_err(|_| ApiError::new("corrupt", "The stored key is damaged."))?;
        self.vault_open_by(path, move |p| {
            let id = Identity::from_bytes(&bytes, &password).map_err(|e| match e {
                OpenError::WrongPassword => ApiError::new("wrong_password", "Wrong password for your key."),
                _ => ApiError::new("corrupt", "The stored key is damaged."),
            })?;
            drop(password);
            Vault::open_with_identity(p, &id).map_err(|e| match e {
                OpenError::WrongPassword => ApiError::new("not_recipient", "This volume is not shared with your key."),
                e => e.into(),
            })
        })
    }
}

pub mod cmd {
    use super::{MyKey, RecipientCheck};
    use crate::error::{ApiError, ApiResult};
    use crate::state::{AppState, SlotInfo, VaultInfo};
    use secrecy::SecretString;
    use std::sync::Arc;
    use tauri::State;

    type St<'a> = State<'a, Arc<AppState>>;

    async fn blocking<T: Send + 'static>(st: &St<'_>, f: impl FnOnce(&AppState) -> ApiResult<T> + Send + 'static) -> ApiResult<T> {
        let s = st.inner().clone();
        tauri::async_runtime::spawn_blocking(move || f(&s)).await.map_err(|e| ApiError::new("error", format!("Internal error: {e}")))?
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn my_key_get(st: St<'_>) -> ApiResult<MyKey> {
        blocking(&st, |s| s.my_key_get()).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn my_key_create(st: St<'_>, password: String, kdf: Option<String>) -> ApiResult<MyKey> {
        let pw = SecretString::from(password);
        blocking(&st, move |s| s.my_key_create(pw, kdf.as_deref())).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn my_key_import(st: St<'_>, path: String, password: String) -> ApiResult<MyKey> {
        let pw = SecretString::from(password);
        blocking(&st, move |s| s.my_key_import(&path, pw)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn my_key_export(st: St<'_>, dest: String) -> ApiResult<()> {
        blocking(&st, move |s| s.my_key_export(&dest)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn my_key_export_public(st: St<'_>, dest: String) -> ApiResult<()> {
        blocking(&st, move |s| s.my_key_export_public(&dest)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn my_key_remove(st: St<'_>) -> ApiResult<MyKey> {
        blocking(&st, |s| s.my_key_remove()).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn recipient_check(st: St<'_>, text: String) -> ApiResult<RecipientCheck> {
        blocking(&st, move |s| s.recipient_check(&text)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn recipient_read_file(st: St<'_>, path: String) -> ApiResult<RecipientCheck> {
        blocking(&st, move |s| s.recipient_read_file(&path)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn slot_add_recipient(st: St<'_>, vault: String, recipient: String, label: Option<String>) -> ApiResult<Vec<SlotInfo>> {
        blocking(&st, move |s| s.slot_add_recipient(&vault, &recipient, label)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn vault_open_with_key(st: St<'_>, path: String, password: String) -> ApiResult<VaultInfo> {
        let pw = SecretString::from(password);
        blocking(&st, move |s| s.vault_open_with_key(&path, pw)).await
    }
}
