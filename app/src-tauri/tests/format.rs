use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, NoEvents};
use nv_fs::{Fs, Kind, ROOT};
use secrecy::SecretString;
use std::path::Path;
use std::sync::Arc;

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn portable(stick: &Path) -> Arc<AppState> {
    AppState::new(Store::new(Locations { exe_dir: Some(stick.to_path_buf()), removable: Some(vec![]), config_dir: None }), Box::new(NoEvents))
}

#[test]
fn upgrade_format_end_to_end() {
    let stick = tempfile::tempdir().unwrap();
    let st = portable(stick.path());
    let new = st.vault_create(stick.path().join("Nuovo.nvault").to_str().unwrap(), pw("pw"), Some("fast")).unwrap();
    assert!(new.single_file && !new.can_upgrade_format, "new volumes are single-file");
    st.vault_lock(&new.id, "user").unwrap();
    let path = stick.path().join("Lavoro.nvault");
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/nv-core/tests/fixtures/compat/2026-10-24/cifrato.nvault"), &path).unwrap();
    let info = st.vault_open(path.to_str().unwrap(), pw("cifrato-pw")).unwrap();
    assert!(!info.single_file && info.can_upgrade_format);
    let v = st.vault(&info.id).unwrap();
    let f = v.create(ROOT, "nota.txt", Kind::File).unwrap();
    v.write(f.id, 0, b"prima dell'aggiornamento").unwrap();
    drop(v);
    let up = st.vault_upgrade_format(&info.id).unwrap();
    assert!(up.single_file && !up.can_upgrade_format);
    assert_eq!(up.id, info.id);
    assert!(st.vault_list_open().iter().any(|i| i.id == info.id && i.single_file));
    assert!(st.fs_list(&info.id, ROOT).unwrap().iter().any(|n| n.name == "nota.txt"));
    assert!(st.vault_upgrade_format(&info.id).unwrap().single_file, "again: nothing to do");
    st.vault_lock(&info.id, "user").unwrap();
    let names: Vec<_> = std::fs::read_dir(stick.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("Lavoro")).collect();
    assert_eq!(names, vec!["Lavoro.nvault".to_owned()]);
    let again = st.vault_open(path.to_str().unwrap(), pw("cifrato-pw")).unwrap();
    assert!(again.single_file);
    let v = st.vault(&again.id).unwrap();
    let n = v.lookup(ROOT, "nota.txt").unwrap();
    let mut buf = vec![0u8; n.size as usize];
    v.read(n.id, 0, &mut buf).unwrap();
    assert_eq!(buf, b"prima dell'aggiornamento");
    drop(v);
    st.vault_lock(&again.id, "user").unwrap();

    let fixed = st.vault_create_sized(stick.path().join("Fisso.nvault").to_str().unwrap(), pw("pw"), Some("fast"), Some(8 << 20)).unwrap();
    assert!(fixed.single_file && !fixed.can_upgrade_format);
    st.vault_lock(&fixed.id, "user").unwrap();
    let old = stick.path().join("Chiavetta.nvault");
    nv_core::Vault::create_fixed_as(&old, &pw("pw"), nv_core::KdfPreset::Fast, 16 << 20, false).unwrap().close().unwrap();
    let info = st.vault_open(old.to_str().unwrap(), pw("pw")).unwrap();
    assert!(!info.single_file && info.can_upgrade_format);
    let up = st.vault_upgrade_format(&info.id).unwrap();
    assert!(up.single_file && !up.can_upgrade_format);
    st.vault_lock(&info.id, "user").unwrap();
    assert_eq!(std::fs::metadata(&old).unwrap().len(), 16 << 20);
    assert!(!stick.path().join("Chiavetta.nvault-journal").exists());

    let full = stick.path().join("Pieno.nvault");
    {
        let v = nv_core::Vault::create_fixed_as(&full, &pw("pw"), nv_core::KdfPreset::Fast, 8 << 20, false).unwrap();
        let f = v.create(ROOT, "pieno.bin", Kind::File).unwrap();
        v.write(f.id, 0, &vec![7u8; 7_000_000]).unwrap();
        v.close().unwrap();
    }
    let info = st.vault_open(full.to_str().unwrap(), pw("pw")).unwrap();
    assert!(!info.single_file && !info.can_upgrade_format);
    assert_eq!(st.vault_upgrade_format(&info.id).unwrap_err().code, "volume_full");
    st.vault(&info.id).unwrap().resize(16 << 20, &|_| {}).unwrap();
    assert!(st.vault_upgrade_format(&info.id).unwrap().single_file);
    st.vault_lock(&info.id, "user").unwrap();
}
