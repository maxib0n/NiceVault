//! Duress ("autodistruzione") slots.
//!
//! On the wire a duress slot IS a `Slot::Password` (same postcard tag, same Argon2id parameters
//! as an existing password slot, 48-byte wrap): someone holding a copy of the file cannot tell
//! it apart, so cannot avoid it. Two things make it one:
//! - its wrap uses AAD kind [`KIND`] (password = 0, recipient = 1) and seals a random key, not
//!   the master: at unlock, after the usual kind-0 attempt fails, a kind-[`KIND`] attempt with
//!   the same KEK succeeds only for the duress password;
//! - its nonce is `keyed BLAKE3(HKDF(master, "nv duress tag"), volume_id || index || salt)[..24]`
//!   instead of random: with the master (vault unlocked) the app can list it as "duress";
//!   without it the nonce looks random.
//!
//! Entering the duress password runs [`destroy`]: every slot's key material, every sealed
//! label and the header MAC are replaced by fresh random bytes of the same length, in both
//! copies, each fsynced. Argon2id parameters, slot count and holes, the appearance section and
//! the padding are kept, so the header still decodes and every later attempt (the real
//! passwords included) fails exactly like a wrong password, in the same time.

use crate::header::{self, COPY_LEN, Header, Slot, SlotEntry};
use crate::keys::{Key32, random};
use std::fs::File;

/// AAD kind byte of a duress wrap.
pub const KIND: u8 = 2;

/// The nonce that marks password-shaped slot `index` (with this `salt`) as a duress slot.
pub fn tag_nonce(master: &Key32, volume_id: &[u8; 16], index: usize, salt: &[u8; 32]) -> [u8; 24] {
    let k = master.derive(b"nv duress tag");
    let mut msg = volume_id.to_vec();
    msg.push(index as u8);
    msg.extend_from_slice(salt);
    let h = blake3::keyed_hash(k.bytes(), &msg);
    h.as_bytes()[..24].try_into().expect("24 bytes")
}

/// Whether slot `index` is a duress slot (needs the master, i.e. an unlocked volume).
pub fn is_duress(master: &Key32, volume_id: &[u8; 16], index: usize, slot: &Slot) -> bool {
    match slot {
        Slot::Password { salt, nonce, .. } => tag_nonce(master, volume_id, index, salt) == *nonce,
        _ => false,
    }
}

/// A duress slot at `index`: `kek` = Argon2id(duress password, `salt`) with parameters `argon2`.
pub fn new_slot(kek: &Key32, argon2: header::Argon, salt: [u8; 32], master: &Key32, volume_id: &[u8; 16], index: usize) -> Slot {
    let nonce = tag_nonce(master, volume_id, index, &salt);
    let wrapped = header::wrap_with_nonce(kek, &nonce, &Key32::random(), volume_id, index, KIND);
    Slot::Password { argon2, salt, nonce, wrapped }
}

fn fill(b: &mut [u8]) {
    getrandom::fill(b).expect("OS RNG unavailable");
}

/// Replaces all key material of an entry with random bytes of the same shape.
fn scrub(e: &mut SlotEntry) {
    match &mut e.slot {
        Slot::Password { salt, nonce, wrapped, .. } => {
            *salt = random();
            *nonce = random();
            fill(wrapped);
        }
        Slot::Recipient { x25519_eph_pub, mlkem_ct, recipient_fpr, nonce, wrapped } => {
            *x25519_eph_pub = random();
            fill(mlkem_ct);
            *recipient_fpr = random();
            *nonce = random();
            fill(wrapped);
        }
        Slot::Fido2(b) => fill(b),
    }
    if let Some(l) = &mut e.label {
        l.nonce = random();
        fill(&mut l.ct);
    }
}

/// Destroys the keys: writes a scrubbed header to both copies, each followed by fsync.
///
/// Order (crash safety): first the copy `read_headers` did NOT pick, keeping its seq, so a
/// crash after it leaves the intact copy the winner — the volume opens as before and the
/// duress slot is still armed. Then the picked copy with seq = max + 1. Once that write lands
/// no copy holds a key. `steps` < 2 stops early (tests simulate a crash with it).
pub fn destroy(f: &File, h: &Header, copy_seq: [u64; 2], steps: usize) -> std::io::Result<()> {
    // A wrong password writes nothing: keep the file's times as they were (ctime still moves).
    let times = f.metadata().and_then(|m| Ok(std::fs::FileTimes::new().set_modified(m.modified()?).set_accessed(m.accessed()?)));
    let r = scrub_copies(f, h, copy_seq, steps);
    if let Ok(t) = times {
        let _ = f.set_times(t);
    }
    r
}

fn scrub_copies(f: &File, h: &Header, copy_seq: [u64; 2], steps: usize) -> std::io::Result<()> {
    let mut body = h.body.clone();
    body.slots.iter_mut().flatten().for_each(scrub);
    let picked = if copy_seq[1] > copy_seq[0] { 1 } else { 0 };
    let other = 1 - picked;
    let plan = [(other, copy_seq[other]), (picked, copy_seq[0].max(copy_seq[1]) + 1)];
    for &(i, seq) in plan.iter().take(steps) {
        body.seq = seq;
        let copy = Header { body: body.clone(), header_mac: random(), appearance: h.appearance.clone(), layout: h.layout, fixed: h.fixed, journal: h.journal };
        crate::write_at(f, (i * COPY_LEN) as u64, &header::encode_copy(&copy))?;
        f.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::header::{self, COPY_LEN, Slot};
    use crate::{KdfPreset, OpenError, SlotError, Vault};
    use secrecy::SecretString;
    use std::path::Path;
    use std::time::{Duration, Instant};

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    fn copies(path: &Path) -> (Vec<u8>, [Option<header::Header>; 2]) {
        let raw = std::fs::read(path).unwrap();
        let a = header::decode_copy(&raw[..COPY_LEN]).ok();
        let b = header::decode_copy(&raw[COPY_LEN..2 * COPY_LEN]).ok();
        (raw, [a, b])
    }

    /// Every secret byte string of every slot of every valid copy.
    fn secrets(h: &[Option<header::Header>; 2]) -> Vec<Vec<u8>> {
        let mut out = vec![];
        for e in h.iter().flatten().flat_map(|h| h.body.slots.iter().flatten()) {
            match &e.slot {
                Slot::Password { salt, nonce, wrapped, .. } => out.extend([salt.to_vec(), nonce.to_vec(), wrapped.clone()]),
                Slot::Recipient { mlkem_ct, wrapped, nonce, x25519_eph_pub, .. } => {
                    out.extend([mlkem_ct.clone(), wrapped.clone(), nonce.to_vec(), x25519_eph_pub.to_vec()])
                }
                Slot::Fido2(b) => out.push(b.clone()),
            }
            if let Some(l) = &e.label {
                out.push(l.ct.clone());
            }
        }
        out
    }

    /// A volume with "real" (slot 0), "riserva" (slot 1, labelled), a recipient and "panico" (duress).
    fn armed(dir: &Path, name: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        let v = Vault::create(&p, &pw("real"), KdfPreset::Fast).unwrap();
        v.add_password(&pw("riserva"), KdfPreset::Fast, Some("Busta")).unwrap();
        v.add_recipient(&crate::Identity::generate().recipient(), Some("Anna")).unwrap();
        assert_eq!(v.add_duress(&pw("panico"), KdfPreset::Fast, Some("Rapina")).unwrap(), 3);
        v.close().unwrap();
        p
    }

    #[test]
    fn duress_slot_rules_and_listing() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("v.nvault");
        let v = Vault::create(&p, &pw("real"), KdfPreset::Fast).unwrap();
        assert!(matches!(v.add_duress(&pw("real"), KdfPreset::Fast, None), Err(SlotError::DuressConflict)));
        let i = v.add_duress(&pw("panico"), KdfPreset::Fast, Some("Rapina")).unwrap();
        let slots = v.slots();
        assert_eq!(slots[i].kind, "duress");
        assert_eq!(slots[i].label.as_deref(), Some("Rapina"));
        assert_eq!(slots[i].kdf, slots[0].kdf, "same Argon2id parameters as the password slot");
        assert!(!slots[i].in_use);
        // the duress slot can never be the only one left
        assert!(matches!(v.remove_slot(0), Err(SlotError::LastSlot)));
        // and is no password to change, nor verifies as one
        assert!(matches!(v.change_password(i, &pw("x"), KdfPreset::Fast, None), Err(SlotError::NotPassword)));
        assert!(!v.verify_password(i, &pw("panico")));
        // a new password equal to the duress one is refused
        assert!(matches!(v.add_password(&pw("panico"), KdfPreset::Fast, None), Err(SlotError::DuressConflict)));
        assert!(matches!(v.change_password(0, &pw("panico"), KdfPreset::Fast, None), Err(SlotError::DuressConflict)));
        // wire-identical to a password slot: peek sees two password slots with equal parameters
        v.close().unwrap();
        let pk = crate::peek(&p).unwrap();
        assert_eq!(pk.password_kdf.len(), 2);
        assert_eq!(pk.password_kdf[0], pk.password_kdf[1]);
        let v = Vault::open(&p, &pw("real")).unwrap();
        v.remove_slot(i).unwrap();
        assert_eq!(v.slots().len(), 1);
    }

    #[test]
    fn duress_destroys_every_slot_in_both_copies() {
        let d = tempfile::tempdir().unwrap();
        let p = armed(d.path(), "v.nvault");
        let peek_before = crate::peek(&p).unwrap();
        let (raw_before, before) = copies(&p);
        let old = secrets(&before);
        assert!(!old.is_empty());
        let size = raw_before.len();

        assert_eq!(Vault::open(&p, &pw("panico")).err(), Some(OpenError::WrongPassword));

        let (raw, after) = copies(&p);
        assert!(after[0].is_some() && after[1].is_some(), "both copies still decode");
        assert_eq!(raw.len(), size);
        for s in &old {
            assert!(!raw.windows(s.len()).any(|w| w == &s[..]), "old key material left in the file");
        }
        for h in after.iter().flatten() {
            assert_eq!(h.body.slots.len(), before[0].as_ref().unwrap().body.slots.len());
            assert_eq!(h.appearance, before[0].as_ref().unwrap().appearance);
        }
        // Nothing opens any more, every error is the wrong-password one; the duress password too.
        for p_ in ["real", "riserva", "panico", "altro"] {
            assert_eq!(Vault::open(&p, &pw(p_)).err(), Some(OpenError::WrongPassword), "{p_}");
        }
        // From outside it looks the same: same KDF costs (same unlock time), same slot count.
        let pk = crate::peek(&p).unwrap();
        assert_eq!((pk.password_kdf, pk.recipient_slots), (peek_before.password_kdf, peek_before.recipient_slots));
    }

    #[test]
    fn crash_after_first_copy_leaves_volume_intact_and_armed() {
        let d = tempfile::tempdir().unwrap();
        let p = armed(d.path(), "v.nvault");
        let f = std::fs::OpenOptions::new().read(true).write(true).open(&p).unwrap();
        let (h, seqs, _) = crate::read_headers(&f).unwrap();
        super::destroy(&f, &h, seqs, 1).unwrap(); // "crash" after the first copy
        drop(f);
        let (_, c) = copies(&p);
        assert!(c[0].is_some() && c[1].is_some());
        // The intact copy still wins: real password opens (and repairs both copies) ...
        let v = Vault::open(&p, &pw("real")).unwrap();
        assert_eq!(v.slots().iter().filter(|s| s.kind == "duress").count(), 1);
        v.close().unwrap();
        // ... and the duress password still destroys.
        assert_eq!(Vault::open(&p, &pw("panico")).err(), Some(OpenError::WrongPassword));
        assert_eq!(Vault::open(&p, &pw("real")).err(), Some(OpenError::WrongPassword));
        // A crash after the second copy is the finished destruction (tested above); a torn
        // second write fails its checksum, so the already-scrubbed first copy is used.
        let p2 = armed(d.path(), "w.nvault");
        let f = std::fs::OpenOptions::new().read(true).write(true).open(&p2).unwrap();
        let (h, seqs, _) = crate::read_headers(&f).unwrap();
        super::destroy(&f, &h, seqs, 1).unwrap();
        let picked = if seqs[1] > seqs[0] { 1 } else { 0 };
        crate::write_at(&f, (picked * COPY_LEN + 100) as u64, &[0xAA; 512]).unwrap(); // torn
        drop(f);
        for p_ in ["real", "riserva", "panico"] {
            assert_eq!(Vault::open(&p2, &pw(p_)).err(), Some(OpenError::WrongPassword), "{p_}");
        }
    }

    fn median(mut v: Vec<Duration>) -> Duration {
        v.sort();
        v[v.len() / 2]
    }

    /// A duress attempt takes about as long as a wrong-password attempt: both run every
    /// slot's Argon2id; the duress one adds two 64 KiB writes + fsync.
    #[test]
    fn duress_timing_matches_wrong_password() {
        let d = tempfile::tempdir().unwrap();
        const N: usize = 5;
        let paths: Vec<_> = (0..N).map(|i| armed(d.path(), &format!("t{i}.nvault"))).collect();
        let mut wrong = vec![];
        let mut duress = vec![];
        for p in &paths {
            let t = Instant::now();
            assert_eq!(Vault::open(p, &pw("sbagliata")).err(), Some(OpenError::WrongPassword));
            wrong.push(t.elapsed());
            let t = Instant::now();
            assert_eq!(Vault::open(p, &pw("panico")).err(), Some(OpenError::WrongPassword));
            duress.push(t.elapsed());
        }
        let (w, du) = (median(wrong), median(duress));
        println!("median wrong = {w:?}, duress = {du:?}");
        let tol = w / 2 + Duration::from_millis(40);
        assert!(du <= w + tol && w <= du + tol, "wrong {w:?} vs duress {du:?}");
    }
}
