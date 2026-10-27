use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

pub const PAIR_INFO: &[u8] = b"nv-browser-pair-v1";
pub const AAD: &[u8] = b"nv1";

pub type Key = Zeroizing<[u8; 32]>;

pub fn sas(epk_e: &[u8; 32], epk_a: &[u8; 32]) -> String {
    let mut h = Sha256::new();
    h.update(b"nv-pair");
    h.update(epk_e);
    h.update(epk_a);
    let d = h.finalize();
    let n = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) % 1_000_000;
    format!("{n:06}")
}

pub fn hkdf(ikm: &[u8], salt: &[u8], info: &[u8]) -> Key {
    let mut out = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(salt), ikm).expand(info, out.as_mut()).expect("32 bytes is a valid HKDF length");
    out
}

pub struct PairOffer {
    pub epk_a: [u8; 32],
    pub sas: String,
    pub key: Key,
}

pub fn pair_commit(epk_e: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"nv-pair-commit");
    h.update(epk_e);
    h.finalize().into()
}

pub fn pair_secret() -> (StaticSecret, [u8; 32]) {
    let secret = StaticSecret::random_from_rng(rand_core::OsRng);
    let epk_a = PublicKey::from(&secret).to_bytes();
    (secret, epk_a)
}

pub fn pair_with(secret: &StaticSecret, epk_e: &[u8; 32]) -> Option<PairOffer> {
    let epk_a = PublicKey::from(secret).to_bytes();
    let shared = secret.diffie_hellman(&PublicKey::from(*epk_e));
    if !shared.was_contributory() {
        return None;
    }
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(epk_e);
    salt[32..].copy_from_slice(&epk_a);
    let key = hkdf(shared.as_bytes(), &salt, PAIR_INFO);
    Some(PairOffer { epk_a, sas: sas(epk_e, &epk_a), key })
}

pub fn alias(key: &[u8; 32], what: &str) -> String {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("any key length");
    m.update(b"nv-alias\0");
    m.update(what.as_bytes());
    m.finalize().into_bytes()[..10].iter().map(|b| format!("{b:02x}")).collect()
}

pub struct Half {
    cipher: Aes256Gcm,
    ctr: u64,
}

impl Half {
    fn new(k: &Key) -> Half {
        Half { cipher: Aes256Gcm::new_from_slice(k.as_ref()).expect("32-byte key"), ctr: 0 }
    }

    fn nonce(&self) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[4..].copy_from_slice(&self.ctr.to_be_bytes());
        n
    }

    pub fn seal(&mut self, plain: &[u8]) -> Vec<u8> {
        let n = self.nonce();
        self.ctr += 1;
        self.cipher.encrypt(Nonce::from_slice(&n), Payload { msg: plain, aad: AAD }).expect("AES-GCM encrypt")
    }

    pub fn open(&mut self, sealed: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
        let n = self.nonce();
        let p = self.cipher.decrypt(Nonce::from_slice(&n), Payload { msg: sealed, aad: AAD }).ok()?;
        self.ctr += 1;
        Some(Zeroizing::new(p))
    }
}

pub fn session(k: &[u8; 32], n_e: &[u8], n_a: &[u8]) -> (Half, Half) {
    let salt = [n_e, n_a].concat();
    (Half::new(&hkdf(k, &salt, b"nv-session-v1 e2a")), Half::new(&hkdf(k, &salt, b"nv-session-v1 a2e")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAIR_COMMIT_OF_ONES: &str = "8fc259ebc18ccfd1c3ec58d6e636035165c06478c63a6af56290c1c77f1f7aa0";

    #[test]
    fn both_sides_derive_the_same_key_and_code() {
        let esk = x25519_dalek::EphemeralSecret::random_from_rng(rand_core::OsRng);
        let epk_e = PublicKey::from(&esk).to_bytes();
        let (secret, epk_a) = pair_secret();
        let offer = pair_with(&secret, &epk_e).unwrap();
        assert_eq!(offer.epk_a, epk_a);
        let shared = esk.diffie_hellman(&PublicKey::from(offer.epk_a));
        let salt = [epk_e, offer.epk_a].concat();
        assert_eq!(*hkdf(shared.as_bytes(), &salt, PAIR_INFO), *offer.key);
        assert_eq!(sas(&epk_e, &offer.epk_a), offer.sas);
        assert_eq!(offer.sas.len(), 6);
        assert!(pair_with(&secret, &[0u8; 32]).is_none(), "low-order point refused");
    }

    #[test]
    fn known_vectors() {
        let e = [1u8; 32];
        let a = [2u8; 32];
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(sas(&e, &a), "865608");
        assert_eq!(hex(&pair_commit(&e)), PAIR_COMMIT_OF_ONES);
        assert_eq!(alias(&[7u8; 32], "local:/v/Personale.nvault"), "2814035647ffbdba0b95");
        assert_eq!(hex(hkdf(&[5u8; 32], &[e, a].concat(), PAIR_INFO).as_ref()), "1ac778692ad09bc098c4b65cc3a1e61f168bc001b6cf4e4ba6ae4d75ee6673f0");
        let (mut e2a, _) = session(&[9u8; 32], &[3u8; 16], &[4u8; 16]);
        assert_eq!(hex(&e2a.seal(br#"{"id":1}"#)), "2bd9518e99be3319d469c9e58fa1b33040df96a0f022738c");
    }

    #[test]
    fn frames_open_only_in_order_once() {
        let k = [5u8; 32];
        let (mut a_send, _) = session(&k, b"ne", b"na");
        let (mut b_recv, _) = session(&k, b"ne", b"na");
        let f1 = a_send.seal(b"one");
        let f2 = a_send.seal(b"two");
        assert!(b_recv.open(&f2).is_none(), "out of order");
        assert_eq!(&**b_recv.open(&f1).unwrap(), b"one");
        assert!(b_recv.open(&f1).is_none(), "replay");
        let mut forged = f2.clone();
        forged[0] ^= 1;
        assert!(b_recv.open(&forged).is_none(), "forged");
        assert_eq!(&**b_recv.open(&f2).unwrap(), b"two");
        let (_, mut a2e) = session(&k, b"ne", b"na");
        assert!(a2e.open(&a_send.seal(b"x")).is_none());
    }
}
