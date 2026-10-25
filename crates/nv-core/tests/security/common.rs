//! Attacker's toolkit: a mirror of the on-disk header so tests can decode,
//! edit and re-checksum copies exactly as someone holding the file could.
#![allow(dead_code)]

use nv_core::{Appearance, KdfPreset, Vault};
use nv_fs::{Fs, Kind, NodeId, ROOT};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const COPY_LEN: usize = 64 * 1024;
pub const DB_OFFSET: usize = 128 * 1024;
pub const PAGE: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argon {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Slot {
    Password { argon2: Argon, salt: [u8; 32], nonce: [u8; 24], wrapped: Vec<u8> },
    Recipient { x25519_eph_pub: [u8; 32], mlkem_ct: Vec<u8>, recipient_fpr: [u8; 16], nonce: [u8; 24], wrapped: Vec<u8> },
    Fido2(Vec<u8>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SealedLabel {
    pub nonce: [u8; 24],
    pub ct: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlotEntry {
    pub slot: Slot,
    pub label: Option<SealedLabel>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Body {
    pub magic: [u8; 8],
    pub version: u16,
    pub volume_id: [u8; 16],
    pub seq: u64,
    pub slots: Vec<Option<SlotEntry>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedAppearance {
    pub appearance: Appearance,
    pub mac: [u8; 32],
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Layout {
    pub page_size: u32,
    pub chunk_size: u32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Fixed {
    pub capacity: u64,
}

/// Single-file volume (format v2 `journal` section; every volume `Vault::create` makes now).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct JournalRegion {
    pub region: u64,
    pub moved_to: u64,
    pub split: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Header {
    pub body: Body,
    pub header_mac: [u8; 32],
    pub appearance: Option<SignedAppearance>,
    pub layout: Option<Layout>,
    pub fixed: Option<Fixed>,
    pub journal: Option<JournalRegion>,
}

pub fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

pub fn read_copy(path: &Path, which: usize) -> Header {
    let raw = std::fs::read(path).unwrap();
    postcard::take_from_bytes::<Header>(&raw[which * COPY_LEN..(which + 1) * COPY_LEN - 32]).unwrap().0
}

pub fn encode(h: &Header) -> Vec<u8> {
    let mut buf = vec![0u8; COPY_LEN];
    postcard::to_slice(h, &mut buf[..COPY_LEN - 32]).unwrap();
    let sum = blake3::hash(&buf[..COPY_LEN - 32]);
    buf[COPY_LEN - 32..].copy_from_slice(sum.as_bytes());
    buf
}

/// Writes `h` as copy `which` with a valid (unkeyed) checksum: anyone can do this.
pub fn write_copy(path: &Path, which: usize, h: &Header) {
    use std::os::unix::fs::FileExt;
    let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.write_all_at(&encode(h), (which * COPY_LEN) as u64).unwrap();
}

pub fn write_at(path: &Path, off: usize, bytes: &[u8]) {
    use std::os::unix::fs::FileExt;
    let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.write_all_at(bytes, off as u64).unwrap();
}

/// What the attacker computes with a master key of their own (HKDF-SHA256, no salt).
pub fn derive(master: &[u8; 32], info: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 32];
    hkdf::Hkdf::<sha2::Sha256>::new(None, master).expand(info, &mut k).unwrap();
    k
}

pub fn header_mac(master: &[u8; 32], body: &Body) -> [u8; 32] {
    *blake3::keyed_hash(&derive(master, b"nv header mac"), &postcard::to_allocvec(body).unwrap()).as_bytes()
}

pub fn argon(pw: &[u8], a: Argon, salt: &[u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, argon2::Params::new(a.m_kib, a.t, a.p, Some(32)).unwrap())
        .hash_password_into(pw, salt, &mut out)
        .unwrap();
    out
}

pub fn wrap(kek: &[u8; 32], nonce: &[u8; 24], key: &[u8; 32], vid: &[u8; 16], index: u8, kind: u8) -> Vec<u8> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    let mut aad = vid.to_vec();
    aad.extend([index, kind]);
    chacha20poly1305::XChaCha20Poly1305::new(kek.into())
        .encrypt(chacha20poly1305::XNonce::from_slice(nonce), Payload { msg: key, aad: &aad })
        .unwrap()
}

pub const FAST: Argon = Argon { m_kib: 8 * 1024, t: 1, p: 1 };

/// A small volume: files `a` (4 KiB) and `big` (600 KiB, three chunks), password "pw".
pub fn sample(dir: &Path, name: &str) -> (PathBuf, NodeId, NodeId) {
    let p = dir.join(name);
    let v = Vault::create(&p, &pw("pw"), KdfPreset::Fast).unwrap();
    let a = v.create(ROOT, "a", Kind::File).unwrap().id;
    v.write(a, 0, &[b'A'; 4096]).unwrap();
    let big = v.create(ROOT, "big", Kind::File).unwrap().id;
    v.write(big, 0, &pat(7, 600 * 1024)).unwrap();
    v.close().unwrap();
    (p, a, big)
}

pub fn pat(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E3779B97F4A7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

pub fn read_all(v: &Vault, id: NodeId) -> nv_fs::Result<Vec<u8>> {
    let size = v.getattr(id)?.size as usize;
    let mut out = vec![0u8; size];
    let mut off = 0;
    while off < size {
        let n = v.read(id, off as u64, &mut out[off..])?;
        if n == 0 {
            break;
        }
        off += n;
    }
    Ok(out)
}

/// Every file of the tree, by path, or the first error met.
pub fn dump(v: &Vault) -> nv_fs::Result<Vec<(String, Vec<u8>)>> {
    fn walk(v: &Vault, dir: NodeId, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) -> nv_fs::Result<()> {
        for n in v.readdir(dir)? {
            let path = format!("{prefix}/{}", n.name);
            if n.kind == Kind::Dir {
                out.push((path.clone() + "/", vec![]));
                walk(v, n.id, &path, out)?;
            } else {
                out.push((path, read_all(v, n.id)?));
            }
        }
        Ok(())
    }
    let mut out = vec![];
    walk(v, ROOT, "", &mut out)?;
    Ok(out)
}

/// What the app sees: open with `password`, then browse and read everything.
/// `Err` = the tampering was noticed (open or a read failed).
pub fn open_dump(path: &Path, password: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let v = Vault::open(path, &pw(password)).map_err(|e| format!("open: {e}"))?;
    dump(&v).map_err(|e| format!("read: {e}"))
}

/// Whether SQLite's own integrity check passes (the app never runs it at open).
pub fn integrity_ok(path: &Path, password: &str) -> bool {
    Vault::open(path, &pw(password)).is_ok_and(|v| v.check_integrity().is_ok())
}
