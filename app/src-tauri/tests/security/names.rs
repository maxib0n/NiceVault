use super::open_vault;
use nv_fs::{Fs, Kind, ROOT};

#[test]
fn unix_separators_are_rejected() {
    for n in ["", ".", "..", "a/b", "/abs", "a\0b"] {
        assert!(!nv_fs::valid_name(n), "{n:?} accepted");
    }
}

#[test]
fn windows_path_syntax_is_rejected() {
    for n in [r"..\..\evil.bat", r"C:\Users\Public\x.exe", "C:evil", "NUL", "con.txt", "a:stream", "trailing. ", "file."] {
        assert!(!nv_fs::valid_name(n), "{n:?} accepted");
    }
}

#[test]
fn backslash_names_are_refused_and_host_joins_stay_in_dest() {
    let tmp = tempfile::tempdir().unwrap();
    let (st, id) = open_vault(tmp.path());
    let v = st.vault(&id).unwrap();
    let dir = v.create(ROOT, "share", Kind::Dir).unwrap();
    assert!(matches!(v.create(dir.id, r"..\..\escaped.txt", Kind::File), Err(nv_fs::Error::InvalidName)));
    drop(v);
    st.lock_all("user");
    let dest = tmp.path().join("out");
    for n in [r"..\..\escaped.txt", r"C:\Users\x.exe", "..", "NUL", "a:b", "x. "] {
        let p = nicevault_app::jobs::host_join(&dest, n).unwrap();
        assert_eq!(p.parent(), Some(dest.as_path()), "{n:?} -> {p:?}");
        assert!(nv_fs::valid_name(p.file_name().unwrap().to_str().unwrap()), "{n:?} -> {p:?}");
    }
}
