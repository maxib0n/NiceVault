//! Hybrid X25519 + ML-KEM-768 recipient keys (`Identity` = secret, `Recipient` = public).

use crate::OpenError;
use crate::header::{Argon, KdfPreset, argon2_kek, new_password_kek};
use crate::keys::{Key32, random};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use ml_kem::kem::{Decapsulate, Encapsulate, KeyExport};
use ml_kem::{Ciphertext, MlKem768, Seed};
use nv_fs::Error;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::io::Write;
use std::path::Path;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

type Dk = ml_kem::DecapsulationKey<MlKem768>;
type Ek = ml_kem::EncapsulationKey<MlKem768>;

pub const EK_LEN: usize = 1184;
pub const CT_LEN: usize = 1088;
const PREFIX: &str = "nvpk1";
const KEY_MAGIC: [u8; 8] = *b"NVKEY001";

/// A recipient's secret key pair. Keep it in a password-protected `.nvkey` file.
pub struct Identity {
    x25519: Key32,
    mlkem_seed: Zeroizing<[u8; 64]>,
}

/// Public half of an [`Identity`]; can be added to a vault as a keyslot.
#[derive(Clone, PartialEq, Eq)]
pub struct Recipient {
    x25519: [u8; 32],
    mlkem_ek: Vec<u8>,
}

impl std::fmt::Debug for Recipient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Recipient({})", hex(&self.fingerprint()))
    }
}

pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl Identity {
    pub fn generate() -> Self {
        Identity { x25519: Key32::random(), mlkem_seed: Zeroizing::new(random()) }
    }

    fn dk(&self) -> Dk {
        Dk::from_seed(Seed::from(*self.mlkem_seed))
    }

    pub fn recipient(&self) -> Recipient {
        let x = PublicKey::from(&StaticSecret::from(*self.x25519.bytes()));
        Recipient { x25519: x.to_bytes(), mlkem_ek: self.dk().encapsulation_key().to_bytes().to_vec() }
    }

    /// Writes a password-protected key file (fails if `path` exists). Uses the Strong KDF.
    pub fn to_file(&self, path: &Path, pw: &SecretString) -> nv_fs::Result<()> {
        self.to_file_with(path, pw, KdfPreset::Strong)
    }

    pub fn to_file_with(&self, path: &Path, pw: &SecretString, kdf: KdfPreset) -> nv_fs::Result<()> {
        let (argon2, salt, kek) = new_password_kek(pw.expose_secret().as_bytes(), kdf);
        let mut secret = Zeroizing::new([0u8; 96]);
        secret[..32].copy_from_slice(self.x25519.bytes());
        secret[32..].copy_from_slice(&*self.mlkem_seed);
        let nonce: [u8; 24] = random();
        let wrapped = XChaCha20Poly1305::new(kek.bytes().into())
            .encrypt(XNonce::from_slice(&nonce), Payload { msg: &secret[..], aad: &KEY_MAGIC })
            .expect("encrypt");
        let f = KeyFile { magic: KEY_MAGIC, argon2, salt, nonce, wrapped };
        let bytes = postcard::to_allocvec(&f).expect("serialize");
        let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(io)?;
        out.write_all(&bytes).and_then(|_| out.sync_all()).map_err(io)
    }

    pub fn from_file(path: &Path, pw: &SecretString) -> Result<Self, OpenError> {
        let bytes = std::fs::read(path).map_err(|e| OpenError::Io(e.to_string()))?;
        let f: KeyFile = postcard::from_bytes(&bytes).map_err(|_| OpenError::NotAVault)?;
        if f.magic != KEY_MAGIC {
            return Err(OpenError::NotAVault);
        }
        let kek = argon2_kek(pw.expose_secret().as_bytes(), f.argon2, &f.salt).ok_or_else(|| OpenError::Corrupt("bad KDF parameters".into()))?;
        let secret = Zeroizing::new(
            XChaCha20Poly1305::new(kek.bytes().into())
                .decrypt(XNonce::from_slice(&f.nonce), Payload { msg: &f.wrapped, aad: &KEY_MAGIC })
                .map_err(|_| OpenError::WrongPassword)?,
        );
        if secret.len() != 96 {
            return Err(OpenError::Corrupt("bad key file".into()));
        }
        let mut seed = Zeroizing::new([0u8; 64]);
        seed.copy_from_slice(&secret[32..]);
        Ok(Identity { x25519: Key32::from_slice(&secret[..32]), mlkem_seed: seed })
    }

    /// KEK for a Recipient slot addressed to this identity.
    pub(crate) fn slot_kek(&self, volume_id: &[u8; 16], eph_pub: &[u8; 32], ct: &[u8]) -> Option<Key32> {
        if ct.len() != CT_LEN {
            return None;
        }
        let ct_arr = Ciphertext::<MlKem768>::try_from(ct).ok()?;
        let ss_x = StaticSecret::from(*self.x25519.bytes()).diffie_hellman(&PublicKey::from(*eph_pub));
        if !ss_x.was_contributory() {
            return None;
        }
        let ss_k = self.dk().decapsulate(&ct_arr);
        Some(hybrid_kek(ss_x.as_bytes(), ss_k.as_slice(), volume_id, eph_pub, ct))
    }
}

#[derive(Serialize, Deserialize)]
struct KeyFile {
    magic: [u8; 8],
    argon2: Argon,
    salt: [u8; 32],
    nonce: [u8; 24],
    wrapped: Vec<u8>,
}

fn io(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::AlreadyExists { Error::Exists } else { Error::Io(e.to_string()) }
}

fn hybrid_kek(ss_x: &[u8], ss_k: &[u8], volume_id: &[u8; 16], eph_pub: &[u8; 32], ct: &[u8]) -> Key32 {
    let mut ikm = Zeroizing::new([0u8; 64]);
    ikm[..32].copy_from_slice(ss_x);
    ikm[32..].copy_from_slice(ss_k);
    let mut info = b"nv hybrid kek".to_vec();
    info.extend_from_slice(volume_id);
    info.extend_from_slice(eph_pub);
    info.extend_from_slice(ct);
    let mut out = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(None, &ikm[..]).expand(&info, &mut out[..]).expect("hkdf");
    Key32::from_slice(&out[..])
}

impl Recipient {
    pub fn fingerprint(&self) -> [u8; 16] {
        let mut h = blake3::Hasher::new();
        h.update(&self.x25519).update(&self.mlkem_ek);
        h.finalize().as_bytes()[..16].try_into().unwrap()
    }

    /// `nvpk1` + base64url(x25519 pub || ML-KEM-768 ek || 4-byte BLAKE3 checksum).
    #[allow(clippy::inherent_to_string_shadow_display)]
    pub fn to_string(&self) -> String {
        let mut b = self.x25519.to_vec();
        b.extend_from_slice(&self.mlkem_ek);
        let sum = blake3::hash(&b);
        b.extend_from_slice(&sum.as_bytes()[..4]);
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(b))
    }

    pub fn parse(s: &str) -> nv_fs::Result<Self> {
        let bad = |m: &str| Error::Corrupt(format!("invalid recipient: {m}"));
        let body = s.trim().strip_prefix(PREFIX).ok_or_else(|| bad("missing nvpk1 prefix"))?;
        let b = URL_SAFE_NO_PAD.decode(body).map_err(|_| bad("not base64"))?;
        if b.len() != 32 + EK_LEN + 4 {
            return Err(bad("wrong length"));
        }
        let (key, sum) = b.split_at(32 + EK_LEN);
        if &blake3::hash(key).as_bytes()[..4] != sum {
            return Err(bad("checksum mismatch"));
        }
        let r = Recipient { x25519: key[..32].try_into().unwrap(), mlkem_ek: key[32..].to_vec() };
        r.ek().ok_or_else(|| bad("invalid ML-KEM key"))?;
        Ok(r)
    }

    fn ek(&self) -> Option<Ek> {
        Ek::new(&self.mlkem_ek[..].try_into().ok()?).ok()
    }

    /// Fresh ephemeral encapsulation: (eph_pub, mlkem_ct, kek).
    pub(crate) fn seal(&self, volume_id: &[u8; 16]) -> (([u8; 32], Vec<u8>), Key32) {
        let eph = StaticSecret::from(random::<32>());
        let eph_pub = PublicKey::from(&eph).to_bytes();
        let ss_x = eph.diffie_hellman(&PublicKey::from(self.x25519));
        let (ct, ss_k) = self.ek().expect("validated in parse").encapsulate();
        let ct = ct.to_vec();
        let kek = hybrid_kek(ss_x.as_bytes(), ss_k.as_slice(), volume_id, &eph_pub, &ct);
        ((eph_pub, ct), kek)
    }
}

impl std::fmt::Display for Recipient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&Recipient::to_string(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_and_string_roundtrip() {
        let id = Identity::generate();
        let r = id.recipient();
        let s = r.to_string();
        assert!(s.starts_with("nvpk1"));
        let r2 = Recipient::parse(&s).unwrap();
        assert_eq!(r, r2);
        assert_eq!(r.fingerprint(), r2.fingerprint());
        let ((eph, ct), kek) = r2.seal(&[3; 16]);
        assert_eq!(ct.len(), CT_LEN);
        assert_eq!(id.slot_kek(&[3; 16], &eph, &ct).unwrap().bytes(), kek.bytes());
        assert_ne!(Identity::generate().slot_kek(&[3; 16], &eph, &ct).unwrap().bytes(), kek.bytes());
        let mut broken = s.clone();
        broken.replace_range(10..11, if &s[10..11] == "A" { "B" } else { "A" });
        assert!(Recipient::parse(&broken).is_err());
    }
}
