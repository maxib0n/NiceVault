//! Security tests of the Tauri app layer: PoC and property tests.
//!
//! Tests that demonstrate an open finding are `#[ignore = "..."]` so the suite stays
//! green; run them with `cargo test -p nicevault-app --test security -- --ignored`.
//! Tests that confirm a property holds run normally.

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

/// Portable layout in `dir` (nothing persisted on the host), no events.
pub fn state(dir: &Path) -> Arc<AppState> {
    let store = Store::new(Locations { exe_dir: Some(dir.to_path_buf()), removable: Some(vec![]), config_dir: None });
    AppState::new(store, Box::new(NoEvents))
}

/// A fresh volume (fast KDF) in `dir`; returns (state, vault id).
pub fn open_vault(dir: &Path) -> (Arc<AppState>, String) {
    let st = state(dir);
    let info = st
        .vault_create(dir.join("audit.nvault").to_str().unwrap(), SecretString::from("audit-pw".to_owned()), Some("fast"))
        .unwrap();
    (st, info.id)
}
