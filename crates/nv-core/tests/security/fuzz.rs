//! Cheap mutation fuzzing of the header decoder (no new dependency): random byte edits with a
//! re-computed checksum, so the mutations reach postcard decoding instead of stopping at the
//! checksum. `peek` and `open` must answer an error, never panic or allocate wildly.

use crate::common::*;
use nv_core::Vault;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn mutate(rng: &mut Rng, copy: &mut [u8], used: usize) {
    for _ in 0..1 + rng.below(4) {
        let at = rng.below(used + 8);
        copy[at] = match rng.below(4) {
            0 => copy[at] ^ (1 << rng.below(8)),
            1 => 0xff,
            2 => 0,
            _ => rng.next() as u8,
        };
    }
    let sum = blake3::hash(&copy[..COPY_LEN - 32]);
    copy[COPY_LEN - 32..].copy_from_slice(sum.as_bytes());
}

#[test]
fn header_decode_survives_mutations() {
    let t = tempfile::tempdir().unwrap();
    let (p, _, _) = sample(t.path(), "v.nvault");
    let v = Vault::open(&p, &pw("pw")).unwrap();
    v.add_recipient(&nv_core::Identity::generate().recipient(), Some("r")).unwrap();
    v.close().unwrap();
    let raw = std::fs::read(&p).unwrap();
    let used = postcard::to_allocvec(&read_copy(&p, 0)).unwrap().len();
    let mut rng = Rng(0x5eed_1234_abcd_ef01);
    let q = t.path().join("fuzz.nvault");
    let mut panics = vec![];
    for i in 0..4000 {
        let mut m = raw.clone();
        for c in 0..2 {
            let mut copy = m[c * COPY_LEN..(c + 1) * COPY_LEN].to_vec();
            mutate(&mut rng, &mut copy, used);
            m[c * COPY_LEN..(c + 1) * COPY_LEN].copy_from_slice(&copy);
        }
        std::fs::write(&q, &m).unwrap();
        // Every 20th case also goes through a full unlock attempt (Argon2id runs: keep it rare).
        // Mutated KDF parameters may ask for a huge Argon2id: skip those unlocks.
        let full = i % 20 == 0
            && nv_core::peek(&q).is_ok_and(|pk| pk.password_kdf.iter().all(|k| k.m_kib <= 64 * 1024 && k.t <= 4 && k.p <= 8));
        let r = std::panic::catch_unwind(|| {
            let _ = nv_core::peek(&q);
            if full {
                let _ = Vault::open(&q, &pw("pw"));
            }
        });
        if r.is_err() {
            panics.push(i);
        }
        let _ = std::fs::remove_file(t.path().join("fuzz.nvault-journal"));
    }
    assert!(panics.is_empty(), "panicked on cases {panics:?}");
}
