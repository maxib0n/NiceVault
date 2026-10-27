#[path = "security/clipboard.rs"]
mod clipboard;
#[path = "security/external.rs"]
mod external;
#[path = "security/fresh.rs"]
mod fresh;
#[path = "security/media.rs"]
mod media;
#[path = "security/names.rs"]
mod names;
#[path = "security/preview.rs"]
mod preview;

use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, NoEvents};
use secrecy::SecretString;
use std::path::Path;
use std::sync::Arc;

pub fn state(dir: &Path) -> Arc<AppState> {
    let store = Store::new(Locations { exe_dir: Some(dir.to_path_buf()), removable: Some(vec![]), config_dir: None });
    AppState::new(store, Box::new(NoEvents))
}

pub fn open_vault(dir: &Path) -> (Arc<AppState>, String) {
    let st = state(dir);
    let info = st
        .vault_create(dir.join("audit.nvault").to_str().unwrap(), SecretString::from("audit-pw".to_owned()), Some("fast"))
        .unwrap();
    (st, info.id)
}
