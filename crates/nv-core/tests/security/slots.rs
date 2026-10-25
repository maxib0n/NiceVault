//! Keyslots and the duress slot, attacked through the header.

use crate::common::*;
use nv_core::{KdfPreset, OpenError, Vault};
use std::time::{Duration, Instant};

/// Slot injection: the attacker adds a password slot of their own wrapping a master key of
/// their own, and re-MACs the header with it (the MAC key derives from whatever master the
/// slot yields). Their password then gets past the header MAC, but the database key derives
/// from the real master: never opens. The user's password no longer passes the MAC (DoS only).
#[test]
fn injected_slot_with_a_foreign_master_never_opens() {
    let t = tempfile::tempdir().unwrap();
    let (p, _, _) = sample(t.path(), "v.nvault");
    let mut h = read_copy(&p, 0);
    let evil_master = [0x42u8; 32];
    let salt = [9u8; 32];
    let nonce = [3u8; 24];
    let idx = h.body.slots.len() as u8;
    let kek = argon(b"evil", FAST, &salt);
    let wrapped = wrap(&kek, &nonce, &evil_master, &h.body.volume_id, idx, 0);
    h.body.slots.push(Some(SlotEntry { slot: Slot::Password { argon2: FAST, salt, nonce, wrapped }, label: None }));
    h.body.seq += 100;
    h.header_mac = header_mac(&evil_master, &h.body);
    write_copy(&p, 0, &h);
    write_copy(&p, 1, &h);
    assert!(matches!(Vault::open(&p, &pw("evil")), Err(OpenError::Corrupt(_))), "foreign master must not open the database");
    assert!(Vault::open(&p, &pw("pw")).is_err());
}

/// KDF downgrade: lowering a slot's Argon2id cost changes the KEK, so the unwrap fails.
/// There is no way to weaken a slot without the password.
#[test]
fn kdf_downgrade_fails_closed() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("v.nvault");
    let v = Vault::create(&p, &pw("pw"), KdfPreset::Fast).unwrap();
    v.close().unwrap();
    for c in 0..2 {
        let mut h = read_copy(&p, c);
        if let Some(Some(SlotEntry { slot: Slot::Password { argon2, .. }, .. })) = h.body.slots.get_mut(0) {
            *argon2 = Argon { m_kib: 8, t: 1, p: 1 };
        }
        write_copy(&p, c, &h);
    }
    assert_eq!(Vault::open(&p, &pw("pw")).err(), Some(OpenError::WrongPassword));
}

/// Slots moved to another position, or a slot copied from another volume with the same
/// password: the AAD (volume_id || index || kind) refuses both.
#[test]
fn slot_move_and_cross_volume_copy_fail() {
    let t = tempfile::tempdir().unwrap();
    let (a, _, _) = sample(t.path(), "a.nvault");
    let (b, _, _) = sample(t.path(), "b.nvault");
    let hb = read_copy(&b, 0);
    for c in 0..2 {
        let mut h = read_copy(&a, c);
        h.body.slots[0] = hb.body.slots[0].clone();
        write_copy(&a, c, &h);
    }
    assert_eq!(Vault::open(&a, &pw("pw")).err(), Some(OpenError::WrongPassword), "slot from another volume");
    let (m, _, _) = sample(t.path(), "m.nvault");
    for c in 0..2 {
        let mut h = read_copy(&m, c);
        let s = h.body.slots[0].take();
        h.body.slots.push(s);
        write_copy(&m, c, &h);
    }
    assert_eq!(Vault::open(&m, &pw("pw")).err(), Some(OpenError::WrongPassword), "slot moved to index 1");
}

/// Argon2id parameters come from the unauthenticated header and are not bounded:
/// a crafted file (t = 2^32-1, or m_kib = 4 TiB) hangs or aborts the app on the first unlock
/// attempt. Desired: refuse parameters outside a sane envelope before running Argon2id.
#[test]
fn absurd_kdf_parameters_are_refused_quickly() {
    let t = tempfile::tempdir().unwrap();
    let (p, _, _) = sample(t.path(), "v.nvault");
    for c in 0..2 {
        let mut h = read_copy(&p, c);
        if let Some(Some(SlotEntry { slot: Slot::Password { argon2, .. }, .. })) = h.body.slots.get_mut(0) {
            *argon2 = Argon { m_kib: 8, t: u32::MAX, p: 1 };
        }
        write_copy(&p, c, &h);
    }
    assert_eq!(nv_core::peek(&p).unwrap().password_kdf[0].t, u32::MAX, "peek hands it to the UI as is");
    let (tx, rx) = std::sync::mpsc::channel();
    let q = p.clone();
    std::thread::spawn(move || {
        let t0 = Instant::now();
        let _ = Vault::open(&q, &pw("pw"));
        let _ = tx.send(t0.elapsed());
    });
    assert!(rx.recv_timeout(Duration::from_secs(10)).is_ok(), "unlock still running after 10 s");
}

/// A duress attempt rewrites the header (two writes + fsync) while a wrong password
/// writes nothing. Whoever watches the file (mtime, a hash, a sync client, the stick's LED)
/// tells them apart. Desired at least: the file's mtime is the same after either attempt.
#[test]
fn duress_attempt_leaves_the_same_mtime_as_a_wrong_password() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("v.nvault");
    let v = Vault::create(&p, &pw("real"), KdfPreset::Fast).unwrap();
    v.add_duress(&pw("panic"), KdfPreset::Fast, None).unwrap();
    v.close().unwrap();
    let mtime = || std::fs::metadata(&p).unwrap().modified().unwrap();
    let before = mtime();
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(Vault::open(&p, &pw("wrong")).err(), Some(OpenError::WrongPassword));
    assert_eq!(mtime(), before, "wrong password leaves the file alone");
    assert_eq!(Vault::open(&p, &pw("panic")).err(), Some(OpenError::WrongPassword));
    assert_eq!(mtime(), before, "duress attempt changed the file's mtime");
}

/// After a duress attempt no copy holds anything that unwraps with any of the passwords,
/// the file keeps its size, and peek still shows the same slot count and KDF parameters.
#[test]
fn duress_destroys_every_slot_and_keeps_the_shape() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("v.nvault");
    let v = Vault::create(&p, &pw("real"), KdfPreset::Fast).unwrap();
    v.add_password(&pw("second"), KdfPreset::Fast, Some("label")).unwrap();
    v.add_recipient(&nv_core::Identity::generate().recipient(), None).unwrap();
    v.add_duress(&pw("panic"), KdfPreset::Fast, None).unwrap();
    v.close().unwrap();
    let (size, peek) = (std::fs::metadata(&p).unwrap().len(), nv_core::peek(&p).unwrap());
    let before = [read_copy(&p, 0), read_copy(&p, 1)];
    assert_eq!(Vault::open(&p, &pw("panic")).err(), Some(OpenError::WrongPassword));
    let after = [read_copy(&p, 0), read_copy(&p, 1)];
    for pw_ in ["real", "second", "panic"] {
        assert_eq!(Vault::open(&p, &pw(pw_)).err(), Some(OpenError::WrongPassword), "{pw_}");
    }
    assert_eq!(std::fs::metadata(&p).unwrap().len(), size);
    let p2 = nv_core::peek(&p).unwrap();
    assert_eq!((p2.password_kdf, p2.recipient_slots), (peek.password_kdf, peek.recipient_slots));
    let bytes = |h: &Header| postcard::to_allocvec(&h.body.slots).unwrap();
    for (b, a) in before.iter().zip(&after) {
        assert_eq!(bytes(b).len(), bytes(a).len(), "same encoded size");
        // No 16-byte window of old slot material survives in the new copy.
        let old = bytes(b);
        let new = bytes(a);
        let survivors = old.windows(16).filter(|w| w.iter().any(|&x| x != 0) && new.windows(16).any(|n| n == *w)).count();
        assert!(survivors < 4, "{survivors} old 16-byte windows survive (only Argon2 params / tags may)");
    }
}
