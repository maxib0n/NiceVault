//! Clipboard handling of `pm_copy` (src/passwords.rs).

use super::open_vault;
use nicevault_app::passwords::Clipboard;
use nv_core::passwords::EntryPatch;
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

#[derive(Default)]
struct FakeClip(Mutex<Option<String>>);

impl Clipboard for FakeClip {
    fn read(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }
    fn write(&self, text: &str) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(text.to_owned());
        Ok(())
    }
    fn clear(&self) {
        *self.0.lock().unwrap() = Some(String::new());
    }
}

fn entry(st: &nicevault_app::AppState, vault: &str) -> String {
    let p = EntryPatch { title: Some("Bank".into()), password: Some(Zeroizing::new("Hunter2-Secret!".into())), ..Default::default() };
    st.pm_create(vault, p).unwrap().summary.id
}

/// Verified sound: the scheduled clear does not wipe something the user copied afterwards.
#[test]
fn clear_after_spares_a_newer_copy() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, id) = open_vault(tmp.path());
    let eid = entry(&st, &id);
    let clip = Arc::new(FakeClip::default());
    st.pm_copy(clip.clone(), &id, &eid, "password", Some(1)).unwrap();
    clip.write("something else").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1500));
    assert_eq!(clip.read().as_deref(), Some("something else"));
    // clear_after is capped
    assert_eq!(st.pm_copy(clip.clone(), &id, &eid, "password", Some(u64::MAX)).unwrap().clear_after, 600);
}

/// Locking the volume (user, idle, sleep, screen, removal, app exit) leaves a copied
/// password on the clipboard until the timer fires; at app exit the timer thread dies with
/// the process, so on Windows/macOS (and X11 with a clipboard manager) it stays there.
#[test]
fn lock_clears_a_copied_secret() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, id) = open_vault(tmp.path());
    let eid = entry(&st, &id);
    let clip = Arc::new(FakeClip::default());
    st.pm_copy(clip.clone(), &id, &eid, "password", Some(600)).unwrap();
    st.lock_all("user");
    assert_ne!(clip.read().as_deref(), Some("Hunter2-Secret!"), "password still on the clipboard after lock");
}

/// The real clipboard write uses `write_text`, which sets no "exclude from history"
/// hint (KDE `x-kde-passwordManagerHint`, Windows `ExcludeClipboardContentFromMonitorProcessing`
/// / `CanIncludeInClipboardHistory` / `CanUploadToCloudClipboard`, macOS
/// `org.nspasteboard.ConcealedType`). tauri-plugin-clipboard-manager 2.5 has `write_secret` for
/// exactly this. Source-level check (the plugin cannot be driven headless).
#[test]
fn clipboard_write_uses_secret_hints() {
    let src = include_str!("../../src/passwords.rs");
    let imp = src.split("impl Clipboard for TauriClipboard").nth(1).unwrap();
    let write = imp.split("fn write(").nth(1).unwrap().split("fn clear").next().unwrap();
    assert!(write.contains("write_secret"), "TauriClipboard::write = {write}");
}
