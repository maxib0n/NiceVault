use crate::header::{self, COPY_LEN, Header, Slot, SlotEntry};
use crate::keys::{Key32, random};
use std::fs::File;

pub const KIND: u8 = 2;

pub fn tag_nonce(master: &Key32, volume_id: &[u8; 16], index: usize, salt: &[u8; 32]) -> [u8; 24] {
    let k = master.derive(b"nv duress tag");
    let mut msg = volume_id.to_vec();
    msg.push(index as u8);
    msg.extend_from_slice(salt);
    let h = blake3::keyed_hash(k.bytes(), &msg);
    h.as_bytes()[..24].try_into().expect("24 bytes")
}

pub fn is_duress(master: &Key32, volume_id: &[u8; 16], index: usize, slot: &Slot) -> bool {
    match slot {
        Slot::Password { salt, nonce, .. } => tag_nonce(master, volume_id, index, salt) == *nonce,
        _ => false,
    }
}

pub fn new_slot(kek: &Key32, argon2: header::Argon, salt: [u8; 32], master: &Key32, volume_id: &[u8; 16], index: usize) -> Slot {
    let nonce = tag_nonce(master, volume_id, index, &salt);
    let wrapped = header::wrap_with_nonce(kek, &nonce, &Key32::random(), volume_id, index, KIND);
    Slot::Password { argon2, salt, nonce, wrapped }
}

fn fill(b: &mut [u8]) {
    getrandom::fill(b).expect("OS RNG unavailable");
}

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

pub fn destroy(f: &File, h: &Header, copy_seq: [u64; 2], steps: usize) -> std::io::Result<()> {
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
        assert!(matches!(v.remove_slot(0), Err(SlotError::LastSlot)));
        assert!(matches!(v.change_password(i, &pw("x"), KdfPreset::Fast, None), Err(SlotError::NotPassword)));
        assert!(!v.verify_password(i, &pw("panico")));
        assert!(matches!(v.add_password(&pw("panico"), KdfPreset::Fast, None), Err(SlotError::DuressConflict)));
        assert!(matches!(v.change_password(0, &pw("panico"), KdfPreset::Fast, None), Err(SlotError::DuressConflict)));
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
        for p_ in ["real", "riserva", "panico", "altro"] {
            assert_eq!(Vault::open(&p, &pw(p_)).err(), Some(OpenError::WrongPassword), "{p_}");
        }
        let pk = crate::peek(&p).unwrap();
        assert_eq!((pk.password_kdf, pk.recipient_slots), (peek_before.password_kdf, peek_before.recipient_slots));
    }

    #[test]
    fn crash_after_first_copy_leaves_volume_intact_and_armed() {
        let d = tempfile::tempdir().unwrap();
        let p = armed(d.path(), "v.nvault");
        let f = std::fs::OpenOptions::new().read(true).write(true).open(&p).unwrap();
        let (h, seqs, _) = crate::read_headers(&f).unwrap();
        super::destroy(&f, &h, seqs, 1).unwrap();
        drop(f);
        let (_, c) = copies(&p);
        assert!(c[0].is_some() && c[1].is_some());
        let v = Vault::open(&p, &pw("real")).unwrap();
        assert_eq!(v.slots().iter().filter(|s| s.kind == "duress").count(), 1);
        v.close().unwrap();
        assert_eq!(Vault::open(&p, &pw("panico")).err(), Some(OpenError::WrongPassword));
        assert_eq!(Vault::open(&p, &pw("real")).err(), Some(OpenError::WrongPassword));
        let p2 = armed(d.path(), "w.nvault");
        let f = std::fs::OpenOptions::new().read(true).write(true).open(&p2).unwrap();
        let (h, seqs, _) = crate::read_headers(&f).unwrap();
        super::destroy(&f, &h, seqs, 1).unwrap();
        let picked = if seqs[1] > seqs[0] { 1 } else { 0 };
        crate::write_at(&f, (picked * COPY_LEN + 100) as u64, &[0xAA; 512]).unwrap();
        drop(f);
        for p_ in ["real", "riserva", "panico"] {
            assert_eq!(Vault::open(&p2, &pw(p_)).err(), Some(OpenError::WrongPassword), "{p_}");
        }
    }

    fn median(mut v: Vec<Duration>) -> Duration {
        v.sort();
        v[v.len() / 2]
    }

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
