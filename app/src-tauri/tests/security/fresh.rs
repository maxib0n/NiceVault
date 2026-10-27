use nicevault_app::AppState;
use nicevault_app::store::{Locations, Store};
use nicevault_app::NoEvents;
use nv_fs::{Fs, Kind, ROOT};
use secrecy::SecretString;
use std::path::Path;
use std::sync::Arc;

fn installed(dir: &Path) -> Arc<AppState> {
    let store = Store::new(Locations { exe_dir: Some(dir.to_path_buf()), removable: Some(vec![]), config_dir: Some(dir.join("config")) });
    AppState::new(store, Box::new(NoEvents))
}

fn pw() -> SecretString {
    SecretString::from("audit-pw".to_owned())
}

#[test]
fn an_older_copy_of_the_volume_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("v.nvault");
    let p = path.to_str().unwrap();
    let st = installed(tmp.path());
    let info = st.vault_create(p, pw(), Some("fast")).unwrap();
    assert!(info.rolled_back.is_none());
    st.vault_lock(&info.id, "user").unwrap();
    let old = std::fs::read(&path).unwrap();

    let info = st.vault_open(p, pw()).unwrap();
    assert!(info.rolled_back.is_none(), "same file: not older");
    let v = st.vault(&info.id).unwrap();
    let f = v.create(ROOT, "after-the-backup", Kind::File).unwrap();
    v.write(f.id, 0, b"new").unwrap();
    drop(v);
    st.vault_lock(&info.id, "user").unwrap();
    st.lock_all("user");
    drop(st);

    std::fs::write(&path, &old).unwrap();
    let st = installed(tmp.path());
    let info = st.vault_open(p, pw()).unwrap();
    let rb = info.rolled_back.expect("an older copy opened without a warning");
    assert!(rb.now < rb.seen);
    st.vault_lock(&info.id, "user").unwrap();
    let info = st.vault_open(p, pw()).unwrap();
    assert!(info.rolled_back.is_some());
    let info = st.vault_accept_rollback(&info.id).unwrap();
    assert!(info.rolled_back.is_none());
    st.vault_lock(&info.id, "user").unwrap();
    let info = st.vault_open(p, pw()).unwrap();
    assert!(info.rolled_back.is_none(), "accepted: the anchor follows the kept copy");
    st.lock_all("user");
}
