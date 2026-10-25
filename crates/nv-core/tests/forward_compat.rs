//! Downgrade safety: a volume last written by a NEWER build (unknown header version, slot kind,
//! emblem kind or header field) must make this build answer `UnsupportedVersion` — not
//! "corrupt", not "wrong password" — and leave every byte of the file as it was.
//!

use nv_core::{Appearance, KdfPreset, LockedEmblem, OpenError, Vault};
use secrecy::SecretString;
use std::path::Path;

const COPY_LEN: usize = 64 * 1024;

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn volume(dir: &Path, appearance: Option<Appearance>) -> std::path::PathBuf {
    let p = dir.join("v.nvault");
    let v = Vault::create(&p, &pw("pw"), KdfPreset::Fast).unwrap();
    v.set_appearance(appearance.as_ref()).unwrap();
    v.close().unwrap();
    p
}

/// Edits the checksummed part of header copy `which` (0, 1) and fixes its checksum, as a newer
/// build writing its own format would.
fn edit_copy(path: &Path, which: usize, f: impl Fn(&mut [u8])) {
    let mut raw = std::fs::read(path).unwrap();
    let c = &mut raw[which * COPY_LEN..(which + 1) * COPY_LEN];
    f(&mut c[..COPY_LEN - 32]);
    let sum = blake3::hash(&c[..COPY_LEN - 32]);
    c[COPY_LEN - 32..].copy_from_slice(sum.as_bytes());
    std::fs::write(path, raw).unwrap();
}

fn files(path: &Path) -> Vec<Option<Vec<u8>>> {
    ["", "-wal", "-shm"]
        .iter()
        .map(|s| {
            let mut p = path.as_os_str().to_owned();
            p.push(s);
            std::fs::read(p).ok()
        })
        .collect()
}

/// Open (password), peek and peek_appearance all say "newer version"; nothing on disk changed.
fn refused_untouched(path: &Path) {
    let before = files(path);
    for _ in 0..2 {
        assert!(matches!(Vault::open(path, &pw("pw")), Err(OpenError::UnsupportedVersion(_))));
        assert!(matches!(Vault::open(path, &pw("wrong")), Err(OpenError::UnsupportedVersion(_))));
        assert!(matches!(nv_core::peek(path), Err(OpenError::UnsupportedVersion(_))));
        assert!(matches!(nv_core::peek_appearance(path), Err(OpenError::UnsupportedVersion(_))));
    }
    assert!(before == files(path), "the volume was written to");
    let e = Vault::open(path, &pw("pw")).err().unwrap();
    assert_eq!(serde_json::to_value(&e).unwrap()["code"], "unsupported_version");
}

#[test]
fn newer_header_version_is_unsupported() {
    let t = tempfile::tempdir().unwrap();
    let p = volume(t.path(), None);
    for c in 0..2 {
        edit_copy(&p, c, |b| b[8] = 3); // version varint right after the magic (new volumes are 2)
    }
    refused_untouched(&p);
}

#[test]
fn unknown_slot_kind_is_unsupported() {
    let t = tempfile::tempdir().unwrap();
    let p = volume(t.path(), None);
    // magic 8 | version 1 | volume_id 16 | seq 1 (< 128) | slot count 1 | Some 1 | Slot tag
    for c in 0..2 {
        edit_copy(&p, c, |b| {
            assert_eq!(b[28], 0, "Slot::Password tag");
            b[28] = 7;
        });
    }
    refused_untouched(&p);
}

#[test]
fn unknown_locked_emblem_kind_is_unsupported() {
    let t = tempfile::tempdir().unwrap();
    let id = "zz-unique-icon";
    let p = volume(t.path(), Some(Appearance { accent: None, emblem: Some(LockedEmblem::Icon(id.into())) }));
    for c in 0..2 {
        edit_copy(&p, c, |b| {
            let at = b.windows(id.len()).position(|w| w == id.as_bytes()).unwrap();
            assert_eq!((b[at - 2], b[at - 1]), (1, id.len() as u8), "Icon tag, string length");
            b[at - 2] = 9;
        });
    }
    refused_untouched(&p);
}

#[test]
fn unknown_trailing_header_field_is_unsupported() {
    let t = tempfile::tempdir().unwrap();
    let p = volume(t.path(), Some(Appearance { accent: Some("prussia".into()), emblem: None }));
    for c in 0..2 {
        edit_copy(&p, c, |b| b[COPY_LEN - 33] = 1); // in the padding after every known field
    }
    refused_untouched(&p);
}

/// Only the newest copy is in the newer format (the other still holds the previous, older
/// header): falling back to the older copy and "repairing" would lose what the newer build
/// wrote, so the volume is refused as well.
#[test]
fn one_newer_copy_is_enough_to_refuse() {
    let t = tempfile::tempdir().unwrap();
    let p = volume(t.path(), None);
    edit_copy(&p, 1, |b| b[COPY_LEN - 33] = 1);
    refused_untouched(&p);
}

/// Control: a torn copy (checksum mismatch) is still damage, repaired from the other copy.
#[test]
fn torn_copy_still_opens() {
    let t = tempfile::tempdir().unwrap();
    let p = volume(t.path(), None);
    let mut raw = std::fs::read(&p).unwrap();
    raw[COPY_LEN + 40] ^= 0xff;
    std::fs::write(&p, raw).unwrap();
    Vault::open(&p, &pw("pw")).unwrap().close().unwrap();
}
