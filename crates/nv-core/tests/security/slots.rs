use crate::common::*;
use nv_core::{KdfPreset, OpenError, Vault};
use std::time::{Duration, Instant};

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
        let old = bytes(b);
        let new = bytes(a);
        let survivors = old.windows(16).filter(|w| w.iter().any(|&x| x != 0) && new.windows(16).any(|n| n == *w)).count();
        assert!(survivors < 4, "{survivors} old 16-byte windows survive (only Argon2 params / tags may)");
    }
}
