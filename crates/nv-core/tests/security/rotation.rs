//! Key rotation. After `rotate_master`, an old copy of the header and its password no
//! longer decrypt anything written since, even with the attacker's own tools.

use crate::common::*;
use nv_core::{Identity, KdfPreset, OpenError, Vault};
use nv_core::passwords::{EntryPatch, Secret};
use nv_fs::{Fs, Kind, ROOT};
use zeroize::Zeroizing;

#[test]
fn rotation_revokes_old_headers_and_keeps_the_data() {
    let t = tempfile::tempdir().unwrap();
    let (p, a, big) = sample(t.path(), "v.nvault");
    let v = Vault::open(&p, &pw("pw")).unwrap();
    v.add_password(&pw("other"), KdfPreset::Fast, Some("second")).unwrap();
    v.add_recipient(&Identity::generate().recipient(), None).unwrap();
    v.add_duress(&pw("panic"), KdfPreset::Fast, None).unwrap();
    let entry = EntryPatch { title: Some("bank".into()), password: Some(Zeroizing::new("s3cret".into())), ..Default::default() };
    let pm = v.pm_create(entry).unwrap();
    v.close().unwrap();
    let want = open_dump(&p, "pw").unwrap();
    let old_header = std::fs::read(&p).unwrap()[..DB_OFFSET].to_vec();

    let v = Vault::open(&p, &pw("pw")).unwrap();
    assert!(matches!(v.rotate_master(0, &pw("wrong")), Err(nv_core::SlotError::WrongPassword)));
    let dropped = v.rotate_master(0, &pw("pw")).unwrap();
    assert_eq!(dropped, vec![1, 2, 3]);
    // The session keeps working on the new key, mid-write included.
    assert_eq!(read_all(&v, a).unwrap(), vec![b'A'; 4096]);
    let f = v.create(ROOT, "after-rotation", Kind::File).unwrap();
    v.write(f.id, 0, b"fresh").unwrap();
    assert_eq!(v.slots().len(), 1);
    assert_eq!(v.pm_reveal(&pm.summary.id, Secret::Password).unwrap().as_str(), "s3cret");
    v.close().unwrap();
    assert!(!rekey_leftover(&p));

    let mut got = open_dump(&p, "pw").unwrap();
    got.retain(|(n, _)| n != "/after-rotation");
    assert_eq!(got, want, "same content after rotation");
    for gone in ["other", "panic"] {
        assert_eq!(Vault::open(&p, &pw(gone)).err(), Some(OpenError::WrongPassword), "{gone}");
    }
    assert!(Vault::open(&p, &pw("pw")).is_ok(), "duress slot was dropped, not triggered");

    // The attacker's old header: even its genuine master no longer opens the database.
    write_at(&p, 0, &old_header);
    let r = Vault::open(&p, &pw("pw"));
    assert!(matches!(r, Err(OpenError::Corrupt(_))), "{:?}", r.err());
    let _ = big;
}

fn rekey_leftover(p: &std::path::Path) -> bool {
    let mut s = p.as_os_str().to_owned();
    s.push("-rekey");
    std::path::Path::new(&s).exists()
}

/// A crash before the rename leaves the old volume intact; its leftover goes at the next open.
#[test]
fn rotation_leftover_is_harmless() {
    let t = tempfile::tempdir().unwrap();
    let (p, _, _) = sample(t.path(), "v.nvault");
    let want = open_dump(&p, "pw").unwrap();
    let mut s = p.as_os_str().to_owned();
    s.push("-rekey");
    std::fs::write(&s, b"half a rotated volume").unwrap();
    assert_eq!(open_dump(&p, "pw").unwrap(), want);
    assert!(!rekey_leftover(&p));
}
