//! "Open with the default app" temp copies (src/external.rs).

use super::open_vault;
use nv_fs::{Fs, Kind, ROOT};

/// Verified sound (Unix): the per-run folder and the per-file folder are 0700, and the
/// copy is wiped (and its folder removed) when the volume locks.
#[test]
#[cfg(unix)]
fn temp_copy_is_private_and_wiped_on_lock() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let (st, id) = open_vault(tmp.path());
    let v = st.vault(&id).unwrap();
    let n = v.create(ROOT, "tax.pdf", Kind::File).unwrap();
    v.write(n.id, 0, b"%PDF plaintext").unwrap();
    drop(v);
    let p = st.extract_external(&id, n.id).unwrap();
    let dir = p.parent().unwrap();
    let base = dir.parent().unwrap();
    assert_eq!(std::fs::metadata(dir).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(std::fs::metadata(base).unwrap().permissions().mode() & 0o777, 0o700);
    assert!(base.file_name().unwrap().to_str().unwrap().starts_with("nicevault-"));
    st.vault_lock(&id, "user").unwrap();
    assert!(!p.exists() && !dir.exists(), "copy left after lock");
    st.lock_all("user");
}

/// The write-back follows a symlink the external side puts in place of the copy, so
/// any host file the user can read is pulled into the volume (and a later preview/export
/// shows it). Same-user only.
#[test]
#[cfg(unix)]
fn reimport_does_not_follow_symlinks() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, id) = open_vault(tmp.path());
    let v = st.vault(&id).unwrap();
    let n = v.create(ROOT, "notes.txt", Kind::File).unwrap();
    v.write(n.id, 0, b"original").unwrap();
    let p = st.extract_external(&id, n.id).unwrap();
    let host = tmp.path().join("host-secret.txt");
    std::fs::write(&host, b"HOST FILE CONTENT").unwrap();
    std::fs::remove_file(&p).unwrap();
    std::os::unix::fs::symlink(&host, &p).unwrap();
    st.poll_external();
    st.poll_external();
    let mut buf = vec![0u8; 64];
    let k = v.read(n.id, 0, &mut buf).unwrap();
    drop(v);
    st.lock_all("user");
    assert_eq!(&buf[..k], b"original", "the vault node now holds the symlink target's bytes");
}

/// "Open with" refuses programs and scripts on every OS.
#[test]
fn executables_are_not_opened() {
    use nicevault_app::state::runs_code;
    for n in ["Fattura.pdf.exe", "x.BAT", "a.js", "run.ps1", "s.lnk", "y.Msi"] {
        assert!(runs_code(n), "{n:?}");
    }
    for n in ["Fattura.pdf", "notes.txt", "exe", "photo.jpeg", "archive.zip"] {
        assert!(!runs_code(n), "{n:?}");
    }
    let tmp = tempfile::tempdir().unwrap();
    let (st, id) = open_vault(tmp.path());
    let n = st.vault(&id).unwrap().create(ROOT, "Fattura.pdf.exe", Kind::File).unwrap();
    assert_eq!(st.fs_open_external(&id, n.id).unwrap_err().code, "unsafe_type");
    st.lock_all("user");
}
