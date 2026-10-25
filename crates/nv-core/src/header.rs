//! Volume header (two 64 KiB copies), keyslots and the key-wrapping crypto.

use crate::appearance::Appearance;
use crate::keys::{Key32, random};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub const MAGIC: [u8; 8] = *b"NICEVLT1";
/// Format v1: no layout section; the store uses [`Layout::LEGACY`].
pub const VERSION: u16 = 1;
/// Format v2: same as v1 plus a [`Layout`] section (authenticated by the header MAC).
pub const VERSION_LAYOUT: u16 = 2;
pub const COPY_LEN: usize = 64 * 1024;
pub const MAX_SLOTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argon {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

/// Wire format of a keyslot. Variant order is part of the format (postcard tags).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Slot {
    Password { argon2: Argon, salt: [u8; 32], nonce: [u8; 24], wrapped: Vec<u8> },
    Recipient {
        x25519_eph_pub: [u8; 32],
        mlkem_ct: Vec<u8>,
        recipient_fpr: [u8; 16],
        nonce: [u8; 24],
        wrapped: Vec<u8>,
    },
    /// Reserved for phase 2; never unlocks in this version.
    Fido2(Vec<u8>),
}

/// A label sealed under `HKDF(master, "nv slot label")`, AAD = `volume_id || slot_index`.
/// Labels are user text, so they are encrypted like everything else that is not needed
/// before unlock (the volume must stay opaque).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SealedLabel {
    pub nonce: [u8; 24],
    pub ct: Vec<u8>,
}

/// One occupied keyslot position: the key material plus an optional sealed label.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlotEntry {
    pub slot: Slot,
    pub label: Option<SealedLabel>,
}

impl SlotEntry {
    pub fn new(slot: Slot, label: Option<SealedLabel>) -> Self {
        Self { slot, label }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeaderBody {
    pub magic: [u8; 8],
    pub version: u16,
    pub volume_id: [u8; 16],
    pub seq: u64,
    /// Fixed positions: the index is bound into each slot's AAD, so removing a slot
    /// leaves a `None` hole instead of shifting the others.
    pub slots: Vec<Option<SlotEntry>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Header {
    pub body: HeaderBody,
    pub header_mac: [u8; 32],
    /// Plaintext appearance section. Trailing on purpose: in a copy written before it
    /// existed the zero padding decodes as `None`.
    pub appearance: Option<SignedAppearance>,
    /// Store layout (format v2). Trailing like `appearance`: a v1 copy's zero padding decodes
    /// as `None` = [`Layout::LEGACY`]. Covered by `header_mac` when present.
    pub layout: Option<Layout>,
    /// Fixed-size volume (format v2 only, after `layout`): the file was preallocated to
    /// `capacity` bytes at creation and never grows or shrinks. Trailing like the others, so a
    /// copy without it decodes as `None`, and a build that does not know it refuses the copy
    /// (non-zero bytes after its last field) instead of growing the file. Covered by `header_mac`.
    pub fixed: Option<Fixed>,
    /// Single-file journal (format v2 only, after `fixed`): the rollback journal lives inside
    /// the `.nvault` file (single.rs), so a volume is always exactly one file. Trailing like the
    /// others: a copy without it decodes as `None` (side-file journal), and every build before it
    /// refuses the copy (unknown version, or non-zero bytes after its last field). Covered by `header_mac`.
    pub journal: Option<JournalRegion>,
}

/// Where the database and its in-file journal sit in a single-file volume. `region` bytes at the start of the store area (right after the header copies)
/// hold the journal records and the journal itself; the database's first `region` bytes were
/// moved to `moved_to` to make room. Database byte `l` is at:
/// `l < region` -> `moved_to + l`; `region <= l < split` -> [`STORE_AT`]` + l` (where it always was);
/// `l >= split` -> `moved_to + region + (l - split)`. A volume made single-file at creation has
/// `split = region`, `moved_to = STORE_AT + region` (= one contiguous database after the region).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalRegion {
    pub region: u64,
    pub moved_to: u64,
    pub split: u64,
}

/// Start of the store area (after the two header copies).
pub const STORE_AT: u64 = 2 * COPY_LEN as u64;

impl JournalRegion {
    /// Smallest / largest region this build accepts (64 KiB records + at least 64 KiB of journal).
    pub const MIN: u64 = 128 << 10;
    pub const MAX: u64 = 64 << 20;

    /// The layout of a volume made single-file at creation.
    pub fn fresh(region: u64) -> Self {
        JournalRegion { region, moved_to: STORE_AT + region, split: region }
    }

    /// The layout of a **fixed-size** volume made single-file at creation (its database starts
    /// at [`FIXED_DB_AT`]: only `region - 64 KiB` of it are displaced, see [`Self::base`]).
    pub fn fresh_fixed(region: u64) -> Self {
        JournalRegion { region, moved_to: STORE_AT + region, split: region - (FIXED_DB_AT - STORE_AT) }
    }

    /// Where the database started before the region took its place: [`STORE_AT`], or
    /// [`FIXED_DB_AT`] in a fixed-size volume (whose 64 KiB footer region became the records area).
    pub fn base(fixed: bool) -> u64 {
        if fixed { FIXED_DB_AT } else { STORE_AT }
    }

    /// Database bytes the region displaced (they live at `moved_to`): `region`, or `region - 64 KiB`
    /// in a fixed-size volume. Database byte `l` is at `moved_to + l` below it,
    /// `base + l` below `split`, `moved_to + displaced + (l - split)` from there on.
    pub fn displaced(&self, fixed: bool) -> u64 {
        self.region - (Self::base(fixed) - STORE_AT)
    }

    pub fn is_valid(&self, page_size: u32) -> bool {
        self.is_valid_in(page_size, None)
    }

    /// [`Self::is_valid`] for a volume that may be fixed-size (whose moved bytes start inside
    /// its capacity; the shim keeps every write inside it).
    pub fn is_valid_in(&self, page_size: u32, fixed: Option<Fixed>) -> bool {
        let p = page_size as u64;
        let d = self.displaced(fixed.is_some());
        (Self::MIN..=Self::MAX).contains(&self.region)
            && self.region.is_multiple_of(64 << 10)
            && self.split >= d
            && self.split.is_multiple_of(p)
            && self.moved_to.is_multiple_of(4096)
            && self.moved_to >= Self::base(fixed.is_some()) + self.split
            && self.moved_to.checked_add(self.region).is_some()
            && fixed.is_none_or(|f| self.moved_to < f.capacity)
    }
}

/// Where a fixed-size volume's database starts when it has no journal region (after the header
/// copies and its 64 KiB footer region; = `vfs::FIXED_DB_OFFSET`).
pub const FIXED_DB_AT: u64 = STORE_AT + (64 << 10);

/// A preallocated volume: the whole `.nvault` file is `capacity` bytes, the database lives in
/// it and a footer at its end records how much of it is in use (vfs.rs, "fixed" mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fixed {
    pub capacity: u64,
}

impl Fixed {
    /// Smallest fixed volume: the header copies, a few store pages and the footer.
    pub const MIN: u64 = 4 << 20;
    /// Largest one this build makes (16 TiB, well under every file system limit it targets).
    pub const MAX: u64 = 16 << 40;

    pub fn is_valid(&self) -> bool {
        (Self::MIN..=Self::MAX).contains(&self.capacity) && self.capacity.is_multiple_of(4096)
    }
}

/// How the encrypted store is laid out. Fixed at creation: a volume is never re-laid-out
/// implicitly (an old volume keeps [`Layout::LEGACY`] forever).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    /// SQLCipher page size (`cipher_page_size`), bytes.
    pub page_size: u32,
    /// File contents are stored in chunks of this many bytes (the last one may be shorter).
    pub chunk_size: u32,
}

impl Layout {
    /// What every volume made before layouts were stored uses (format v1).
    pub const LEGACY: Layout = Layout { page_size: 16 * 1024, chunk_size: 256 * 1024 };

    pub fn is_valid(&self) -> bool {
        let p = self.page_size;
        let c = self.chunk_size;
        p.is_power_of_two() && (4096..=65536).contains(&p) && c.is_power_of_two() && (64 * 1024..=16 << 20).contains(&c)
    }
}

/// The appearance plus its own keyed MAC (`HKDF(master, "nv appearance mac")`, over
/// `volume_id || postcard(appearance)`). Separate from `header_mac` so a tampered appearance
/// is detected and dropped on unlock without refusing an otherwise intact volume.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedAppearance {
    pub appearance: Appearance,
    pub mac: [u8; 32],
}

/// Keyed BLAKE3 over `postcard(body)`, followed by `postcard(layout)` when there is one and
/// `postcard(fixed)` when there is one, then `"journal" || postcard(journal)` when there is one (so
/// a v1 header's MAC is unchanged, and a v2 layout, a fixed capacity or a journal region cannot be
/// stripped or edited).
pub fn mac(mac_key: &Key32, body: &HeaderBody, layout: Option<&Layout>, fixed: Option<&Fixed>, journal: Option<&JournalRegion>) -> [u8; 32] {
    let mut bytes = Zeroizing::new(postcard::to_allocvec(body).expect("serialize header"));
    if let Some(l) = layout {
        bytes.extend_from_slice(&postcard::to_allocvec(l).expect("serialize layout"));
    }
    if let Some(f) = fixed {
        bytes.extend_from_slice(b"fixed");
        bytes.extend_from_slice(&postcard::to_allocvec(f).expect("serialize fixed"));
    }
    if let Some(j) = journal {
        bytes.extend_from_slice(b"journal");
        bytes.extend_from_slice(&postcard::to_allocvec(j).expect("serialize journal"));
    }
    *blake3::keyed_hash(mac_key.bytes(), &bytes).as_bytes()
}

pub fn mac_ok(mac_key: &Key32, h: &Header) -> bool {
    // blake3::Hash's PartialEq is constant-time.
    blake3::Hash::from_bytes(mac(mac_key, &h.body, h.layout.as_ref(), h.fixed.as_ref(), h.journal.as_ref())) == blake3::Hash::from_bytes(h.header_mac)
}

fn appearance_mac(master: &Key32, volume_id: &[u8; 16], a: &Appearance) -> [u8; 32] {
    let k = master.derive(b"nv appearance mac");
    let mut bytes = volume_id.to_vec();
    bytes.extend_from_slice(&postcard::to_allocvec(a).expect("serialize appearance"));
    *blake3::keyed_hash(k.bytes(), &bytes).as_bytes()
}

pub fn sign_appearance(master: &Key32, volume_id: &[u8; 16], a: Appearance) -> SignedAppearance {
    SignedAppearance { mac: appearance_mac(master, volume_id, &a), appearance: a }
}

pub fn appearance_ok(master: &Key32, volume_id: &[u8; 16], s: &SignedAppearance) -> bool {
    blake3::Hash::from_bytes(appearance_mac(master, volume_id, &s.appearance)) == blake3::Hash::from_bytes(s.mac)
}

/// Encoded size of a header copy before padding (must stay <= `COPY_LEN - 32`).
#[cfg(test)]
pub fn encoded_len(h: &Header) -> usize {
    postcard::experimental::serialized_size(h).expect("serialize header")
}

pub fn encode_copy(h: &Header) -> Vec<u8> {
    let mut buf = vec![0u8; COPY_LEN];
    // Caps (MAX_SLOTS, MAX_LABEL, appearance limits) keep the worst case well inside a copy;
    // see the `worst_case_header_fits` test.
    let used = postcard::to_slice(h, &mut buf[..COPY_LEN - 32]).expect("header fits in 64 KiB").len();
    debug_assert!(used <= COPY_LEN - 32);
    let sum = blake3::hash(&buf[..COPY_LEN - 32]);
    buf[COPY_LEN - 32..].copy_from_slice(sum.as_bytes());
    buf
}

pub enum CopyError {
    /// Doesn't even start with the magic: probably not a NiceVault file.
    NoMagic,
    /// Has the magic but is torn / corrupt (checksum mismatch).
    Bad(String),
    /// Intact (checksum ok) but not understood by this build: a newer version, an unknown slot /
    /// emblem kind (postcard tag) or header bytes after the fields this build knows. Written on
    /// purpose by a newer build, so the volume must be left alone (never "repaired").
    Unsupported(String),
}

pub fn decode_copy(buf: &[u8]) -> Result<Header, CopyError> {
    if buf.len() < COPY_LEN || buf[..8] != MAGIC {
        return Err(CopyError::NoMagic);
    }
    let body = &buf[..COPY_LEN - 32];
    let sum = blake3::hash(body);
    if sum != blake3::Hash::from_bytes(buf[COPY_LEN - 32..COPY_LEN].try_into().unwrap()) {
        return Err(CopyError::Bad("header checksum mismatch".into()));
    }
    // The checksum holds, so whatever follows was written deliberately: anything this build
    // cannot read is a newer format, not damage.
    let unsupported = |m: String| CopyError::Unsupported(m);
    // The version sits right after the 8-byte magic (postcard varint u16): check it before the
    // rest, whose layout a newer version may have changed.
    let version: u16 = postcard::take_from_bytes::<u16>(&body[8..]).map(|(v, _)| v).map_err(|e| unsupported(format!("header version: {e}")))?;
    if version != VERSION && version != VERSION_LAYOUT {
        return Err(unsupported(format!("header version {version}")));
    }
    let (h, rest): (Header, &[u8]) = postcard::take_from_bytes(body).map_err(|e| unsupported(format!("header decode: {e}")))?;
    // This build writes zero padding after its last field; non-zero bytes there are fields
    // added by a newer build, which a rewrite here would silently drop.
    if rest.iter().any(|&b| b != 0) {
        return Err(unsupported("header fields unknown to this version".into()));
    }
    // v1 = no layout section, v2 = a layout this build can use (a newer build may allow more).
    match (h.body.version, &h.layout) {
        (VERSION, None) => {}
        (VERSION_LAYOUT, Some(l)) if l.is_valid() => {}
        (_, l) => return Err(unsupported(format!("store layout {l:?} with header version {}", h.body.version))),
    }
    // A fixed capacity only comes with a layout section (v2), and only in sizes this build makes.
    if let Some(f) = h.fixed
        && (h.layout.is_none() || !f.is_valid())
    {
        return Err(unsupported(format!("fixed volume {f:?} with header version {}", h.body.version)));
    }
    // An in-file journal only comes with a layout section (v2); in a fixed-size volume it lies
    // inside the capacity (the region takes the footer region's place, see JournalRegion::base).
    if let Some(j) = h.journal
        && !h.layout.is_some_and(|l| j.is_valid_in(l.page_size, h.fixed))
    {
        return Err(unsupported(format!("journal region {j:?} with header version {}", h.body.version)));
    }
    if h.body.slots.is_empty() || h.body.slots.len() > MAX_SLOTS {
        return Err(unsupported("bad slot count".into()));
    }
    Ok(h)
}

fn aad(volume_id: &[u8; 16], index: usize, kind: u8) -> [u8; 18] {
    let mut a = [0u8; 18];
    a[..16].copy_from_slice(volume_id);
    a[16] = index as u8;
    a[17] = kind;
    a
}

pub fn wrap(kek: &Key32, master: &Key32, volume_id: &[u8; 16], index: usize, kind: u8) -> ([u8; 24], Vec<u8>) {
    let nonce: [u8; 24] = random();
    (nonce, wrap_with_nonce(kek, &nonce, master, volume_id, index, kind))
}

/// [`wrap`] with a caller-chosen nonce (duress slots derive theirs, see duress.rs).
pub fn wrap_with_nonce(kek: &Key32, nonce: &[u8; 24], key: &Key32, volume_id: &[u8; 16], index: usize, kind: u8) -> Vec<u8> {
    XChaCha20Poly1305::new(kek.bytes().into())
        .encrypt(XNonce::from_slice(nonce), Payload { msg: key.bytes(), aad: &aad(volume_id, index, kind) })
        .expect("encrypt")
}

pub fn unwrap(kek: &Key32, nonce: &[u8; 24], wrapped: &[u8], volume_id: &[u8; 16], index: usize, kind: u8) -> Option<Key32> {
    if wrapped.len() != 48 {
        return None;
    }
    let pt = Zeroizing::new(
        XChaCha20Poly1305::new(kek.bytes().into())
            .decrypt(XNonce::from_slice(nonce), Payload { msg: wrapped, aad: &aad(volume_id, index, kind) })
            .ok()?,
    );
    Some(Key32::from_slice(&pt))
}

/// Longest slot label, in UTF-8 bytes (keeps 8 slots well inside a 64 KiB header copy).
pub const MAX_LABEL: usize = 128;

fn label_aad(volume_id: &[u8; 16], index: usize) -> [u8; 17] {
    let mut a = [0u8; 17];
    a[..16].copy_from_slice(volume_id);
    a[16] = index as u8;
    a
}

pub fn seal_label(master: &Key32, volume_id: &[u8; 16], index: usize, label: &str) -> SealedLabel {
    let k = master.derive(b"nv slot label");
    let nonce: [u8; 24] = random();
    let ct = XChaCha20Poly1305::new(k.bytes().into())
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: label.as_bytes(), aad: &label_aad(volume_id, index) })
        .expect("encrypt");
    SealedLabel { nonce, ct }
}

/// `None` if the label does not authenticate (never expected: the header MAC covers it).
pub fn open_label(master: &Key32, volume_id: &[u8; 16], index: usize, l: &SealedLabel) -> Option<String> {
    let k = master.derive(b"nv slot label");
    let pt = XChaCha20Poly1305::new(k.bytes().into())
        .decrypt(XNonce::from_slice(&l.nonce), Payload { msg: &l.ct, aad: &label_aad(volume_id, index) })
        .ok()?;
    String::from_utf8(pt).ok()
}

// ---------------- password KDF ----------------

/// Argon2id cost presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KdfPreset {
    /// For tests only: m = 8 MiB, t = 1, p = 1.
    Fast,
    /// Calibrated to ~0.7 s on this machine; 512 MiB when RAM allows (else less memory, more passes).
    Strong,
}

/// Largest Argon2id cost accepted from a header or key file (no NiceVault build writes more:
/// Strong is <= 512 MiB, t <= 64, p = 4). The parameters are read before anything is
/// authenticated, so without a cap a crafted file hangs or aborts the app.
pub const MAX_ARGON: Argon = Argon { m_kib: 1 << 20, t: 64, p: 16 };

pub fn argon2_kek(pw: &[u8], a: Argon, salt: &[u8; 32]) -> Option<Key32> {
    if a.m_kib > MAX_ARGON.m_kib || a.t > MAX_ARGON.t || a.p > MAX_ARGON.p {
        return None;
    }
    let params = argon2::Params::new(a.m_kib, a.t, a.p, Some(32)).ok()?;
    let mut out = Zeroizing::new([0u8; 32]);
    argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
        .hash_password_into(pw, salt, &mut *out)
        .ok()?;
    Some(Key32::from_slice(&*out))
}

const TARGET: Duration = Duration::from_millis(700);

/// Strong preset memory: 512 MiB if the machine has room for it, else a third of the
/// available memory (floor 64 MiB).
fn strong_m_kib() -> u32 {
    let avail_kib = available_memory() / 1024;
    let want = 512 * 1024u64;
    if avail_kib == 0 || avail_kib >= want * 3 { want as u32 } else { (avail_kib / 3).max(64 * 1024) as u32 }
}

#[cfg(not(target_os = "ios"))]
fn available_memory() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.available_memory()
}

/// iOS: what this process may still allocate before jetsam (libSystem, iOS 13+).
#[cfg(target_os = "ios")]
fn available_memory() -> u64 {
    unsafe extern "C" {
        fn os_proc_available_memory() -> usize;
    }
    // SAFETY: a libSystem query without arguments.
    unsafe { os_proc_available_memory() as u64 }
}

/// Fresh salt and the KEK for fixed parameters `a` (a duress slot copies an existing slot's).
pub fn password_kek_with(pw: &[u8], a: Argon) -> ([u8; 32], Key32) {
    let salt: [u8; 32] = random();
    let kek = argon2_kek(pw, a, &salt).expect("argon2");
    (salt, kek)
}

/// Fresh salt, parameters for `kdf`, and the resulting KEK.
pub fn new_password_kek(pw: &[u8], kdf: KdfPreset) -> (Argon, [u8; 32], Key32) {
    let salt: [u8; 32] = random();
    match kdf {
        KdfPreset::Fast => {
            let a = Argon { m_kib: 8 * 1024, t: 1, p: 1 };
            (a, salt, argon2_kek(pw, a, &salt).expect("argon2"))
        }
        KdfPreset::Strong => {
            // Calibrate with one t=1 pass. If that alone is already >= half the target, keep it
            // (its output IS the KEK, so creation costs one pass); otherwise pick t to land on
            // ~TARGET and run once more.
            let mut a = Argon { m_kib: strong_m_kib(), t: 1, p: 4 };
            loop {
                let t0 = Instant::now();
                let kek = argon2_kek(pw, a, &salt).expect("argon2");
                let d = t0.elapsed();
                if d > TARGET * 2 && a.m_kib > 64 * 1024 {
                    a.m_kib /= 2; // very slow machine: trade memory for time
                    continue;
                }
                if d * 2 >= TARGET {
                    return (a, salt, kek);
                }
                a.t = ((TARGET.as_secs_f64() / d.as_secs_f64()).round() as u32).clamp(2, 64);
                return (a, salt, argon2_kek(pw, a, &salt).expect("argon2"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Header {
        let body = HeaderBody { magic: MAGIC, version: VERSION, volume_id: [7; 16], seq: 3, slots: vec![None, Some(SlotEntry::new(Slot::Fido2(vec![1]), None))] };
        Header { body, header_mac: [9; 32], appearance: None, layout: None, fixed: None, journal: None }
    }

    #[test]
    fn copy_roundtrip_and_corruption() {
        let enc = encode_copy(&sample());
        assert_eq!(enc.len(), COPY_LEN);
        assert_eq!(decode_copy(&enc).ok().unwrap().body.seq, 3);
        let mut bad = enc.clone();
        bad[20] ^= 1;
        assert!(matches!(decode_copy(&bad), Err(CopyError::Bad(_))));
        assert!(matches!(decode_copy(&[0u8; COPY_LEN]), Err(CopyError::NoMagic)));
    }

    /// Header size budget: 8 recipient slots (the largest kind) with max-length labels plus
    /// the largest appearance must fit one copy. Prints the numbers (`--nocapture`).
    #[test]
    fn worst_case_header_fits() {
        use crate::appearance::*;
        let slot = || {
            let s = Slot::Recipient { x25519_eph_pub: [1; 32], mlkem_ct: vec![2; 1088], recipient_fpr: [3; 16], nonce: [4; 24], wrapped: vec![5; 48] };
            Some(SlotEntry::new(s, Some(SealedLabel { nonce: [6; 24], ct: vec![7; MAX_LABEL + 16] })))
        };
        let body = HeaderBody { magic: MAGIC, version: VERSION, volume_id: [7; 16], seq: u64::MAX, slots: (0..MAX_SLOTS).map(|_| slot()).collect() };
        let bare = Header { body, header_mac: [9; 32], appearance: None, layout: None, fixed: None, journal: None };
        let mut img = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x80\0\0\0\x80".to_vec();
        img.resize(MAX_LOCKED_IMAGE, 0xff);
        let a = Appearance { accent: Some("a".repeat(MAX_ACCENT_ID)), emblem: Some(LockedEmblem::Image(img)) };
        a.validate().unwrap();
        let mut full = Header {
            appearance: Some(sign_appearance(&Key32::random(), &[7; 16], a)),
            layout: Some(Layout { page_size: 65536, chunk_size: 16 << 20 }),
            fixed: Some(Fixed { capacity: Fixed::MAX }),
            journal: Some(JournalRegion { region: JournalRegion::MAX, moved_to: Fixed::MAX - (1 << 30), split: Fixed::MAX >> 1 }),
            ..bare.clone()
        };
        full.body.version = VERSION_LAYOUT;
        let (b, f) = (encoded_len(&bare), encoded_len(&full));
        println!("header budget: 8 recipient slots = {b} B, + max appearance = {f} B, copy capacity = {} B", COPY_LEN - 32);
        assert!(f <= COPY_LEN - 32);
        assert_eq!(decode_copy(&encode_copy(&full)).ok().unwrap().appearance, full.appearance);
    }

    #[test]
    fn old_copy_without_appearance_still_decodes() {
        // A copy written before the appearance section: body + mac, then zero padding.
        #[derive(Serialize)]
        struct Old<'a> {
            body: &'a HeaderBody,
            header_mac: [u8; 32],
        }
        let h = sample();
        let mut buf = vec![0u8; COPY_LEN];
        postcard::to_slice(&Old { body: &h.body, header_mac: h.header_mac }, &mut buf[..COPY_LEN - 32]).unwrap();
        let sum = blake3::hash(&buf[..COPY_LEN - 32]);
        buf[COPY_LEN - 32..].copy_from_slice(sum.as_bytes());
        let d = decode_copy(&buf).ok().unwrap();
        assert!(d.appearance.is_none());
        assert_eq!(d.header_mac, h.header_mac);
    }

    #[test]
    fn layout_section_rules() {
        let l = Layout { page_size: 65536, chunk_size: 1 << 20 };
        let mut v2 = sample();
        v2.body.version = VERSION_LAYOUT;
        v2.layout = Some(l);
        assert_eq!(decode_copy(&encode_copy(&v2)).ok().unwrap().layout, Some(l));
        // v1 copies decode without a layout (zero padding), as every existing volume does.
        assert_eq!(decode_copy(&encode_copy(&sample())).ok().unwrap().layout, None);
        let bad = |h: &Header| matches!(decode_copy(&encode_copy(h)), Err(CopyError::Unsupported(_)));
        let mut v1_with = sample();
        v1_with.layout = Some(l);
        assert!(bad(&v1_with), "v1 never carries a layout");
        let mut v2_without = v2.clone();
        v2_without.layout = None;
        assert!(bad(&v2_without), "v2 always carries one");
        let mut silly = v2.clone();
        silly.layout = Some(Layout { page_size: 1000, chunk_size: 1 << 20 });
        assert!(bad(&silly));
        let mut v3 = v2.clone();
        v3.body.version = 3;
        assert!(bad(&v3));
        // The MAC of a v1 header is the pre-layout formula; a v2 layout is covered.
        let k = Key32::random();
        assert_eq!(mac(&k, &sample().body, None, None, None), *blake3::keyed_hash(k.bytes(), &postcard::to_allocvec(&sample().body).unwrap()).as_bytes());
        assert_ne!(mac(&k, &v2.body, Some(&l), None, None), mac(&k, &v2.body, Some(&Layout { chunk_size: 2 << 20, ..l }), None, None));
        assert_ne!(mac(&k, &v2.body, Some(&l), None, None), mac(&k, &v2.body, None, None, None));
    }

    #[test]
    fn fixed_section_rules() {
        let l = Layout::LEGACY;
        let f = Fixed { capacity: 64 << 20 };
        let mut v2 = sample();
        v2.body.version = VERSION_LAYOUT;
        v2.layout = Some(l);
        v2.fixed = Some(f);
        let d = decode_copy(&encode_copy(&v2)).ok().unwrap();
        assert_eq!((d.layout, d.fixed), (Some(l), Some(f)));
        // Copies without it (every volume so far) decode as None.
        let mut plain = v2.clone();
        plain.fixed = None;
        assert_eq!(decode_copy(&encode_copy(&plain)).ok().unwrap().fixed, None);
        assert_eq!(decode_copy(&encode_copy(&sample())).ok().unwrap().fixed, None);
        let bad = |h: &Header| matches!(decode_copy(&encode_copy(h)), Err(CopyError::Unsupported(_)));
        let mut v1_fixed = sample();
        v1_fixed.fixed = Some(f);
        assert!(bad(&v1_fixed), "a fixed volume always has a layout section (v2)");
        for cap in [0, Fixed::MIN - 4096, (64 << 20) + 1, Fixed::MAX + 4096] {
            let mut h = v2.clone();
            h.fixed = Some(Fixed { capacity: cap });
            assert!(bad(&h), "{cap}");
        }
        // A build that knows layouts but not this section sees non-zero bytes after its last
        // field and refuses the copy (it would otherwise grow the file past its capacity).
        #[derive(Serialize, Deserialize)]
        struct LayoutOnly {
            body: HeaderBody,
            header_mac: [u8; 32],
            appearance: Option<SignedAppearance>,
            layout: Option<Layout>,
        }
        let enc = encode_copy(&v2);
        let (_, rest): (LayoutOnly, &[u8]) = postcard::take_from_bytes(&enc[..COPY_LEN - 32]).unwrap();
        assert!(rest.iter().any(|&b| b != 0));
        // The capacity is authenticated, and the MAC of a header without it is unchanged.
        let k = Key32::random();
        assert_eq!(mac(&k, &v2.body, Some(&l), None, None), mac(&k, &plain.body, Some(&l), None, None));
        assert_ne!(mac(&k, &v2.body, Some(&l), Some(&f), None), mac(&k, &v2.body, Some(&l), None, None));
        assert_ne!(mac(&k, &v2.body, Some(&l), Some(&f), None), mac(&k, &v2.body, Some(&l), Some(&Fixed { capacity: 128 << 20 }), None));
    }

    #[test]
    fn journal_section_rules() {
        let l = Layout::LEGACY;
        let j = JournalRegion::fresh(4 << 20);
        let mut v2 = sample();
        v2.body.version = VERSION_LAYOUT;
        v2.layout = Some(l);
        v2.journal = Some(j);
        let d = decode_copy(&encode_copy(&v2)).ok().unwrap();
        assert_eq!((d.layout, d.fixed, d.journal), (Some(l), None, Some(j)));
        assert!(encoded_len(&v2) - encoded_len(&Header { journal: None, ..v2.clone() }) <= 16);
        // A migrated volume: its first `region` bytes moved past the old end.
        let moved = JournalRegion { region: 4 << 20, moved_to: STORE_AT + (64 << 20), split: 64 << 20 };
        assert!(moved.is_valid(l.page_size));
        let bad = |h: &Header| matches!(decode_copy(&encode_copy(h)), Err(CopyError::Unsupported(_)));
        let with = |j: JournalRegion| Header { journal: Some(j), ..v2.clone() };
        let mut v1 = sample();
        v1.journal = Some(j);
        assert!(bad(&v1), "a journal region always comes with a layout section (v2)");
        // A fixed-size volume: the region takes the footer region's place (64 KiB less displaced),
        // a growable volume's mapping is not one of it, and the moved bytes start inside the capacity.
        let f = Some(Fixed { capacity: 64 << 20 });
        let jf = JournalRegion::fresh_fixed(4 << 20);
        assert_eq!((jf.moved_to, jf.split, jf.displaced(true)), (STORE_AT + (4 << 20), (4 << 20) - 65536, (4 << 20) - 65536));
        let d = decode_copy(&encode_copy(&Header { fixed: f, journal: Some(jf), ..v2.clone() })).ok().unwrap();
        assert_eq!((d.fixed, d.journal), (f, Some(jf)));
        assert!(bad(&Header { fixed: f, ..v2.clone() }), "a growable mapping in a fixed volume");
        let upgraded = JournalRegion { region: 4 << 20, moved_to: FIXED_DB_AT + (40 << 20), split: 40 << 20 };
        assert!(!bad(&Header { fixed: f, journal: Some(upgraded), ..v2.clone() }));
        assert!(bad(&Header { fixed: Some(Fixed { capacity: 40 << 20 }), journal: Some(upgraded), ..v2.clone() }), "moved past the capacity");
        assert!(bad(&Header { fixed: f, journal: Some(JournalRegion { split: 40 << 20, ..jf }), ..v2.clone() }), "moved_to below base + split");
        for silly in [
            JournalRegion::fresh(64 << 10),
            JournalRegion::fresh((4 << 20) + 4096),
            JournalRegion::fresh(128 << 20),
            JournalRegion { split: (4 << 20) - 65536, ..j },
            JournalRegion { moved_to: STORE_AT + (4 << 20) - 4096, ..j },
            JournalRegion { split: (8 << 20) + 4096, moved_to: STORE_AT + (16 << 20), ..j },
            JournalRegion { moved_to: u64::MAX - 4095, ..moved },
        ] {
            assert!(bad(&with(silly)), "{silly:?}");
        }
        // Every older build refuses it: builds that know fixed sizes (or only layouts) see
        // non-zero bytes after their last field; builds before layouts see version 2.
        #[derive(Serialize, Deserialize)]
        struct FixedOnly {
            body: HeaderBody,
            header_mac: [u8; 32],
            appearance: Option<SignedAppearance>,
            layout: Option<Layout>,
            fixed: Option<Fixed>,
        }
        let enc = encode_copy(&v2);
        let (_, rest): (FixedOnly, &[u8]) = postcard::take_from_bytes(&enc[..COPY_LEN - 32]).unwrap();
        assert!(rest.iter().any(|&b| b != 0));
        // Authenticated; MACs of headers without it are unchanged.
        let k = Key32::random();
        let plain = blake3::keyed_hash(k.bytes(), &[postcard::to_allocvec(&v2.body).unwrap(), postcard::to_allocvec(&l).unwrap()].concat());
        assert_eq!(mac(&k, &v2.body, Some(&l), None, None), *plain.as_bytes());
        assert_ne!(mac(&k, &v2.body, Some(&l), None, Some(&j)), mac(&k, &v2.body, Some(&l), None, None));
        assert_ne!(mac(&k, &v2.body, Some(&l), None, Some(&j)), mac(&k, &v2.body, Some(&l), None, Some(&moved)));
        assert!(mac_ok(&k, &Header { header_mac: mac(&k, &v2.body, Some(&l), None, Some(&j)), ..v2.clone() }));
        assert!(!mac_ok(&k, &Header { header_mac: mac(&k, &v2.body, Some(&l), None, Some(&j)), journal: Some(moved), ..v2.clone() }));
    }

    #[test]
    fn wrap_binds_index_and_kind() {
        let kek = Key32::random();
        let m = Key32::random();
        let (n, w) = wrap(&kek, &m, &[1; 16], 2, 0);
        assert_eq!(unwrap(&kek, &n, &w, &[1; 16], 2, 0).unwrap().bytes(), m.bytes());
        assert!(unwrap(&kek, &n, &w, &[1; 16], 3, 0).is_none());
        assert!(unwrap(&kek, &n, &w, &[1; 16], 2, 1).is_none());
        assert!(unwrap(&kek, &n, &w, &[2; 16], 2, 0).is_none());
    }

    #[test]
    fn label_seal_binds_index_and_key() {
        let m = Key32::random();
        let l = seal_label(&m, &[1; 16], 3, "Busta in cassaforte");
        assert!(!l.ct.windows(5).any(|w| w == b"Busta"));
        assert_eq!(open_label(&m, &[1; 16], 3, &l).as_deref(), Some("Busta in cassaforte"));
        assert!(open_label(&m, &[1; 16], 4, &l).is_none());
        assert!(open_label(&Key32::random(), &[1; 16], 3, &l).is_none());
    }
}
