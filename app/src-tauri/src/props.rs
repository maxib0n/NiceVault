//! Proprietà: what the properties dialog shows about a file, a folder, a selection or the volume.
//!
//!  - `fs_props { vault, node }`: at once — name, path, kind, size, dates (modified; created,
//!    imported and the original path from `nv_core::NodeMeta`), the type sniffed from the first
//!    bytes, image dimensions (PNG, JPEG, GIF, WebP, BMP headers) and media duration (MP4/MOV/M4A
//!    `mvhd`, WAV, FLAC, Ogg Vorbis/Opus, WebM/Matroska, MP3 Xing or CBR estimate) when they can be
//!    read from a few headers (nothing is decoded), the cached SHA-256 when still valid, and a
//!    folder's direct item count;
//!  - `fs_props_tree { vault, nodes }` -> job: recursive files / folders / bytes of a folder or a
//!    selection, walked in the background with `props-progress { job, files, dirs, bytes }`
//!    (~10/s) and one `props-done { job, files, dirs, bytes, error }`; cancellable (`job_cancel`);
//!  - `fs_hash { vault, node }` -> job: SHA-256 of a file, `props-progress { job, done_bytes,
//!    total_bytes }`, `props-done { job, sha256, error }`; the result is cached in the volume.

use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, Job, lock_or_recover, random_id};
use nv_core::Vault;
use nv_fs::{Fs, Kind, NodeId, ROOT};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const PROGRESS_EVERY: Duration = Duration::from_millis(100);
/// Bytes read from the start of a file for the type and image headers.
const HEAD: usize = 64 * 1024;
/// JPEG frames can come after big EXIF / thumbnail segments: scan this far at most.
const JPEG_SCAN: u64 = 1 << 20;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct NodeProps {
    pub id: NodeId,
    pub name: String,
    pub kind: Kind,
    /// "/Foto/Mare.jpg" ("/" = the volume root)
    pub path: String,
    pub size: u64,
    /// unix ns
    pub mtime: i64,
    /// unix ns; null = unknown (made before this build kept it)
    pub created: Option<i64>,
    /// unix ns; null = made inside the volume, or unknown
    pub imported: Option<i64>,
    /// host path it was imported from
    pub origin: Option<String>,
    /// sniffed from the first bytes (files)
    pub mime: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    /// hex, when computed for the current content
    pub sha256: Option<String>,
    /// folders: items directly inside
    pub children: Option<u64>,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// "/A/B/name" of `id`.
pub fn node_path(v: &Vault, id: NodeId) -> ApiResult<String> {
    let mut parts = vec![];
    let mut cur = id;
    for _ in 0..4096 {
        if cur == ROOT {
            break;
        }
        let n = v.getattr(cur)?;
        parts.push(n.name);
        cur = n.parent;
    }
    parts.reverse();
    Ok(format!("/{}", parts.join("/")))
}

/// Random-access reads of a node (short at EOF).
struct NodeReader<'a> {
    v: &'a Vault,
    id: NodeId,
    size: u64,
}

impl NodeReader<'_> {
    fn at(&self, off: u64, len: usize) -> Vec<u8> {
        if off >= self.size {
            return vec![];
        }
        let len = len.min((self.size - off) as usize);
        let mut buf = vec![0; len];
        let mut got = 0;
        while got < len {
            match self.v.read(self.id, off + got as u64, &mut buf[got..]) {
                Ok(0) | Err(_) => break,
                Ok(k) => got += k,
            }
        }
        buf.truncate(got);
        buf
    }
}

impl AppState {
    pub fn fs_props(&self, vault: &str, node: NodeId) -> ApiResult<NodeProps> {
        let v = self.vault(vault)?;
        let n = v.getattr(node)?;
        let meta = v.node_meta(node).unwrap_or_default();
        let mut p = NodeProps {
            id: n.id,
            name: n.name.clone(),
            kind: n.kind,
            path: node_path(&v, node)?,
            size: n.size,
            mtime: n.mtime,
            created: meta.created,
            imported: meta.imported,
            origin: meta.origin,
            sha256: meta.sha256.map(|h| hex(&h)),
            mime: None,
            width: None,
            height: None,
            duration_ms: None,
            children: None,
        };
        if n.kind == Kind::Dir {
            p.children = Some(v.readdir(node)?.len() as u64);
            return Ok(p);
        }
        let r = NodeReader { v: &v, id: node, size: n.size };
        let head = r.at(0, HEAD);
        p.mime = sniff(&head).map(str::to_owned);
        if let Some((w, h)) = image_size(&head, |off, len| r.at(off, len)) {
            (p.width, p.height) = (Some(w), Some(h));
        }
        p.duration_ms = media_duration(&head, n.size, |off, len| r.at(off, len));
        Ok(p)
    }

    /// Recursive totals of `nodes` (a folder, a selection) as a cancellable job.
    pub fn fs_props_tree(&self, vault: &str, nodes: Vec<NodeId>) -> ApiResult<String> {
        let v = self.vault(vault)?;
        for &n in &nodes {
            v.getattr(n)?;
        }
        Ok(self.props_job(vault, "nv-props", move |st, job, cancel| {
            let (mut files, mut dirs, mut bytes) = (0u64, 0u64, 0u64);
            let mut last = Instant::now();
            let mut stack: Vec<NodeId> = vec![];
            for &n in &nodes {
                let a = v.getattr(n)?;
                match a.kind {
                    Kind::File => {
                        files += 1;
                        bytes += a.size;
                    }
                    Kind::Dir => stack.push(n),
                }
            }
            // the folders given are not counted themselves, only what they hold
            while let Some(d) = stack.pop() {
                if cancel.load(Ordering::Relaxed) {
                    return Err(ApiError::cancelled());
                }
                for c in v.readdir(d)? {
                    match c.kind {
                        Kind::File => {
                            files += 1;
                            bytes += c.size;
                        }
                        Kind::Dir => {
                            dirs += 1;
                            stack.push(c.id);
                        }
                    }
                }
                if last.elapsed() >= PROGRESS_EVERY {
                    last = Instant::now();
                    st.emit("props-progress", json!({ "job": job, "files": files, "dirs": dirs, "bytes": bytes }));
                }
            }
            Ok(json!({ "files": files, "dirs": dirs, "bytes": bytes }))
        }))
    }

    /// SHA-256 of a file as a cancellable job; cached in the volume for its current content.
    pub fn fs_hash(&self, vault: &str, node: NodeId) -> ApiResult<String> {
        let v = self.vault(vault)?;
        let n = v.getattr(node)?;
        if n.kind != Kind::File {
            return Err(nv_fs::Error::IsDir.into());
        }
        Ok(self.props_job(vault, "nv-hash", move |st, job, cancel| {
            let mut h = Sha256::new();
            let mut buf = vec![0u8; crate::jobs::BLOCK];
            let mut off = 0u64;
            let mut last = Instant::now();
            let n = v.getattr(node)?;
            while off < n.size {
                if cancel.load(Ordering::Relaxed) {
                    return Err(ApiError::cancelled());
                }
                let want = buf.len().min((n.size - off) as usize);
                let k = v.read(node, off, &mut buf[..want])?;
                if k == 0 {
                    break;
                }
                h.update(&buf[..k]);
                off += k as u64;
                if last.elapsed() >= PROGRESS_EVERY {
                    last = Instant::now();
                    st.emit("props-progress", json!({ "job": job, "done_bytes": off, "total_bytes": n.size }));
                }
            }
            let digest: [u8; 32] = h.finalize().into();
            // only if the file did not change meanwhile
            if v.getattr(node).is_ok_and(|now| now.size == n.size && now.mtime == n.mtime) {
                let _ = v.set_sha256(node, &digest, n.size, n.mtime);
                let _ = v.flush();
            }
            Ok(json!({ "sha256": hex(&digest), "done_bytes": off, "total_bytes": n.size }))
        }))
    }

    /// Runs `work` on its own thread as a job of `vault` (cancelled by job_cancel or lock); its
    /// result object is merged into `props-done`.
    fn props_job(
        &self,
        vault: &str,
        name: &str,
        work: impl FnOnce(&AppState, &str, &AtomicBool) -> ApiResult<serde_json::Value> + Send + 'static,
    ) -> String {
        let job = random_id();
        let cancel = Arc::new(AtomicBool::new(false));
        lock_or_recover(&self.jobs).insert(job.clone(), Job { vault: vault.to_owned(), cancel: cancel.clone() });
        let st = self.arc();
        let id = job.clone();
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let r = work(&st, &id, &cancel);
                lock_or_recover(&st.jobs).remove(&id);
                let mut done = match &r {
                    Ok(serde_json::Value::Object(o)) => o.clone(),
                    _ => serde_json::Map::new(),
                };
                done.insert("job".into(), json!(id));
                done.insert("error".into(), json!(r.err()));
                st.emit("props-done", serde_json::Value::Object(done));
            })
            .expect("spawn props job");
        job
    }
}

// ---------------------------------------------------------------------------
// sniffing (pure functions over bytes; unit-tested)

/// MIME type from the first bytes, or None.
pub fn sniff(h: &[u8]) -> Option<&'static str> {
    let at = |off: usize, sig: &[u8]| h.len() >= off + sig.len() && &h[off..off + sig.len()] == sig;
    Some(if at(0, b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if at(0, b"\xff\xd8\xff") {
        "image/jpeg"
    } else if at(0, b"GIF87a") || at(0, b"GIF89a") {
        "image/gif"
    } else if at(0, b"RIFF") && at(8, b"WEBP") {
        "image/webp"
    } else if at(0, b"RIFF") && at(8, b"WAVE") {
        "audio/wav"
    } else if at(0, b"RIFF") && at(8, b"AVI ") {
        "video/x-msvideo"
    } else if at(0, b"BM") && h.len() >= 26 && (at(14, &[12, 0, 0, 0]) || at(14, &[40, 0, 0, 0]) || at(14, &[108, 0, 0, 0]) || at(14, &[124, 0, 0, 0])) {
        "image/bmp"
    } else if at(4, b"ftyp") {
        let brand = h.get(8..12).unwrap_or_default();
        match brand {
            b"qt  " => "video/quicktime",
            b"M4A " | b"M4B " => "audio/mp4",
            b"heic" | b"heix" | b"mif1" | b"msf1" => "image/heic",
            b"avif" | b"avis" => "image/avif",
            _ => "video/mp4",
        }
    } else if at(0, b"\x1a\x45\xdf\xa3") {
        if h.windows(4).take(64).any(|w| w == b"webm") { "video/webm" } else { "video/x-matroska" }
    } else if at(0, b"OggS") {
        if h.windows(8).take(128).any(|w| w == b"OpusHead") { "audio/opus" } else if h.windows(6).take(128).any(|w| w == b"\x01vorbis") { "audio/ogg" } else { "application/ogg" }
    } else if at(0, b"fLaC") {
        "audio/flac"
    } else if at(0, b"ID3") || (h.len() >= 2 && h[0] == 0xff && h[1] & 0xe0 == 0xe0 && h[1] & 0x06 != 0) {
        "audio/mpeg"
    } else if at(0, b"%PDF-") {
        "application/pdf"
    } else if at(0, b"PK\x03\x04") {
        zip_kind(h)
    } else if at(0, b"\x1f\x8b") {
        "application/gzip"
    } else if at(0, b"7z\xbc\xaf\x27\x1c") {
        "application/x-7z-compressed"
    } else if at(0, b"Rar!\x1a\x07") {
        "application/vnd.rar"
    } else if at(0, b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1") {
        "application/x-ole-storage" // .doc / .xls / .msg
    } else if at(0, b"SQLite format 3\0") {
        "application/vnd.sqlite3"
    } else if at(0, b"NICEVLT1") {
        "application/x-nicevault"
    } else if at(0, b"<?xml") {
        if h.windows(4).take(512).any(|w| w == b"<svg") { "image/svg+xml" } else { "application/xml" }
    } else if at(0, b"<svg") {
        "image/svg+xml"
    } else if at(0, b"{\\rtf") {
        "application/rtf"
    } else if at(0, b"-----BEGIN ") {
        "application/x-pem-file"
    } else if !h.is_empty() && looks_textual(h) {
        "text/plain"
    } else {
        return None;
    })
}

/// Office Open XML / OpenDocument / EPUB inside a zip, from the first entry names.
fn zip_kind(h: &[u8]) -> &'static str {
    let has = |s: &[u8]| h.windows(s.len()).any(|w| w == s);
    if has(b"mimetypeapplication/vnd.oasis.opendocument.text") {
        "application/vnd.oasis.opendocument.text"
    } else if has(b"mimetypeapplication/vnd.oasis.opendocument.spreadsheet") {
        "application/vnd.oasis.opendocument.spreadsheet"
    } else if has(b"mimetypeapplication/epub+zip") {
        "application/epub+zip"
    } else if has(b"word/") {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    } else if has(b"xl/") {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    } else if has(b"ppt/") {
        "application/vnd.openxmlformats-officedocument.presentationml.presentation"
    } else {
        "application/zip"
    }
}

/// UTF-8 (a cut-off last character allowed) without control bytes other than whitespace.
fn looks_textual(h: &[u8]) -> bool {
    let h = &h[..h.len().min(8192)];
    let ok = match std::str::from_utf8(h) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none() && e.valid_up_to() + 4 > h.len(),
    };
    ok && !h.iter().any(|&b| b < 0x09 || (b > 0x0d && b < 0x20 && b != 0x1b))
}

fn be16(b: &[u8], i: usize) -> Option<u32> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?) as u32)
}
fn le16(b: &[u8], i: usize) -> Option<u32> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as u32)
}
fn be32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn le32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn be64(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(i..i + 8)?.try_into().ok()?))
}
fn le24(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, 0]))
}

/// Pixel size from the header bytes (`read(off, len)` for JPEGs whose frame comes later).
pub fn image_size(h: &[u8], read: impl Fn(u64, usize) -> Vec<u8>) -> Option<(u32, u32)> {
    match sniff(h)? {
        "image/png" if h.get(12..16) == Some(b"IHDR") => Some((be32(h, 16)?, be32(h, 20)?)),
        "image/gif" => Some((le16(h, 6)?, le16(h, 8)?)),
        "image/bmp" => {
            if le32(h, 14)? == 12 {
                Some((le16(h, 18)?, le16(h, 20)?))
            } else {
                Some((le32(h, 18)?, (le32(h, 22)? as i32).unsigned_abs()))
            }
        }
        "image/webp" => match h.get(12..16)? {
            b"VP8 " => Some((le16(h, 26)? & 0x3fff, le16(h, 28)? & 0x3fff)),
            b"VP8L" => {
                let b = le32(h, 21)?;
                Some(((b & 0x3fff) + 1, ((b >> 14) & 0x3fff) + 1))
            }
            b"VP8X" => Some((le24(h, 24)? + 1, le24(h, 27)? + 1)),
            _ => None,
        },
        "image/jpeg" => jpeg_size(read),
        _ => None,
    }
    .filter(|&(w, h)| w > 0 && h > 0)
}

/// Walks the JPEG segments up to the first SOFn frame header.
fn jpeg_size(read: impl Fn(u64, usize) -> Vec<u8>) -> Option<(u32, u32)> {
    let mut off = 2u64;
    while off < JPEG_SCAN {
        let seg = read(off, 9);
        if seg.len() < 4 || seg[0] != 0xff {
            return None;
        }
        let marker = seg[1];
        if marker == 0xff {
            off += 1; // fill byte
            continue;
        }
        if marker == 0xd8 || (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
            off += 2;
            continue;
        }
        let len = be16(&seg, 2)? as u64;
        let sof = matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc);
        if sof {
            return Some((be16(&seg, 7)?, be16(&seg, 5)?));
        }
        if marker == 0xda || marker == 0xd9 || len < 2 {
            return None; // image data before any frame header
        }
        off += 2 + len;
    }
    None
}

/// Playing time from container headers; None when not cheaply known.
pub fn media_duration(h: &[u8], size: u64, read: impl Fn(u64, usize) -> Vec<u8>) -> Option<u64> {
    let ms = |secs: f64| (secs.is_finite() && secs > 0.0 && secs < 1e9).then(|| (secs * 1000.0).round() as u64);
    match sniff(h)? {
        "video/mp4" | "video/quicktime" | "audio/mp4" => mp4_duration(size, &read).and_then(ms),
        "audio/wav" => wav_duration(h).and_then(ms),
        "audio/flac" => {
            // STREAMINFO: sample rate 20 bits, channels 3, bps 5, total samples 36
            if h.get(4)? & 0x7f != 0 {
                return None;
            }
            let b = h.get(18..26)?;
            let rate = ((b[0] as u32) << 12) | ((b[1] as u32) << 4) | ((b[2] as u32) >> 4);
            let total = (((b[3] & 0x0f) as u64) << 32) | u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as u64;
            (rate > 0 && total > 0).then(|| total as f64 / rate as f64).and_then(ms)
        }
        "audio/ogg" | "audio/opus" => ogg_duration(h, size, &read).and_then(ms),
        "video/webm" | "video/x-matroska" => ebml_duration(h).and_then(ms),
        "audio/mpeg" => mp3_duration(h, size).and_then(ms),
        _ => None,
    }
}

/// MP4 / MOV: moov -> mvhd (timescale, duration); the top-level boxes are walked by their headers.
fn mp4_duration(size: u64, read: &impl Fn(u64, usize) -> Vec<u8>) -> Option<f64> {
    let mut off = 0u64;
    for _ in 0..64 {
        if off + 8 > size {
            return None;
        }
        let hdr = read(off, 16);
        let mut len = be32(&hdr, 0)? as u64;
        let kind = hdr.get(4..8)?;
        let mut body = off + 8;
        if len == 1 {
            len = be64(&hdr, 8)?;
            body += 8;
        } else if len == 0 {
            len = size - off;
        }
        if kind == b"moov" {
            let moov = read(body, (len.saturating_sub(body - off)).min(1 << 20) as usize);
            // mvhd is near the start of moov
            let mut i = 0;
            while i + 8 <= moov.len() {
                let l = be32(&moov, i)? as usize;
                if moov.get(i + 4..i + 8)? == b"mvhd" {
                    let v = *moov.get(i + 8)?;
                    let (scale, dur) = if v == 1 { (be32(&moov, i + 28)?, be64(&moov, i + 32)?) } else { (be32(&moov, i + 20)?, be32(&moov, i + 24)? as u64) };
                    return (scale > 0).then(|| dur as f64 / scale as f64);
                }
                if l < 8 {
                    return None;
                }
                i += l;
            }
            return None;
        }
        if len < 8 {
            return None;
        }
        off += len;
    }
    None
}

fn wav_duration(h: &[u8]) -> Option<f64> {
    let mut i = 12;
    let mut byte_rate = None;
    while i + 8 <= h.len() {
        let id = h.get(i..i + 4)?;
        let len = le32(h, i + 4)? as usize;
        if id == b"fmt " {
            byte_rate = le32(h, i + 16);
        } else if id == b"data" {
            let br = byte_rate? as f64;
            return (br > 0.0).then(|| len as f64 / br);
        }
        i += 8 + len + (len & 1);
    }
    None
}

/// Ogg: sample rate from the identification header, last granule position from the end.
fn ogg_duration(h: &[u8], size: u64, read: &impl Fn(u64, usize) -> Vec<u8>) -> Option<f64> {
    let (rate, pre_skip) = if let Some(p) = h.windows(8).position(|w| w == b"OpusHead") {
        (48_000.0, le16(h, p + 10)? as f64)
    } else {
        let p = h.windows(7).position(|w| w == b"\x01vorbis")?;
        (le32(h, p + 12)? as f64, 0.0)
    };
    let tail_off = size.saturating_sub(64 * 1024);
    let tail = read(tail_off, (size - tail_off) as usize);
    let p = tail.windows(4).rposition(|w| w == b"OggS")?;
    let granule = u64::from_le_bytes(tail.get(p + 6..p + 14)?.try_into().ok()?);
    (rate > 0.0 && granule != u64::MAX).then(|| (granule as f64 - pre_skip).max(0.0) / rate)
}

/// Matroska / WebM: Segment > Info > (TimecodeScale, Duration) within the first bytes.
fn ebml_duration(h: &[u8]) -> Option<f64> {
    fn vint(b: &[u8], i: usize, keep_marker: bool) -> Option<(u64, usize)> {
        let first = *b.get(i)?;
        let n = first.leading_zeros() as usize + 1;
        if n > 8 {
            return None;
        }
        let mut v = if keep_marker { first as u64 } else { (first as u64) & ((1u64 << (8 - n)) - 1) };
        for k in 1..n {
            v = (v << 8) | *b.get(i + k)? as u64;
        }
        Some((v, n))
    }
    const SEGMENT: u64 = 0x1853_8067;
    const INFO: u64 = 0x1549_a966;
    const SCALE: u64 = 0x2a_d7b1;
    const DURATION: u64 = 0x4489;
    let mut i = 0;
    let mut scale = 1_000_000f64;
    let mut duration = None;
    let mut in_info_until = 0usize;
    while i < h.len() {
        let (id, a) = vint(h, i, true)?;
        let (len, b) = vint(h, i + a, false)?;
        let data = i + a + b;
        match id {
            SEGMENT | INFO => {
                if id == INFO {
                    in_info_until = data.saturating_add(len as usize);
                }
                i = data; // descend
                continue;
            }
            SCALE => {
                let mut v = 0u64;
                for k in 0..len as usize {
                    v = (v << 8) | *h.get(data + k)? as u64;
                }
                scale = v as f64;
            }
            DURATION => {
                duration = match len {
                    4 => Some(f32::from_be_bytes(h.get(data..data + 4)?.try_into().ok()?) as f64),
                    8 => Some(f64::from_be_bytes(h.get(data..data + 8)?.try_into().ok()?)),
                    _ => None,
                };
            }
            _ => {}
        }
        if in_info_until > 0 && data + len as usize >= in_info_until {
            break;
        }
        // unknown size (live streams): stop
        if len >= (1u64 << 56) - 1 {
            break;
        }
        i = data + len as usize;
    }
    duration.map(|d| d * scale / 1e9)
}

/// MP3: frames from a Xing / Info / VBRI header, else a constant-bitrate estimate.
fn mp3_duration(h: &[u8], size: u64) -> Option<f64> {
    let mut i = 0usize;
    if h.starts_with(b"ID3") {
        let sz = h.get(6..10)?.iter().fold(0usize, |a, &b| (a << 7) | (b & 0x7f) as usize);
        i = 10 + sz + if h.get(5)? & 0x10 != 0 { 10 } else { 0 };
    }
    // first frame sync
    while i + 4 <= h.len() && !(h[i] == 0xff && h[i + 1] & 0xe0 == 0xe0) {
        i += 1;
    }
    let hdr = h.get(i..i + 4)?;
    let version = (hdr[1] >> 3) & 3; // 3 = MPEG1, 2 = MPEG2, 0 = MPEG2.5
    let layer = (hdr[1] >> 1) & 3; // 1 = layer III
    let br_idx = (hdr[2] >> 4) as usize;
    let sr_idx = ((hdr[2] >> 2) & 3) as usize;
    if layer != 1 || br_idx == 0 || br_idx == 15 || sr_idx == 3 || version == 1 {
        return None;
    }
    const BR1: [u32; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
    const BR2: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let rates: [u32; 3] = match version {
        3 => [44_100, 48_000, 32_000],
        2 => [22_050, 24_000, 16_000],
        _ => [11_025, 12_000, 8_000],
    };
    let rate = rates[sr_idx] as f64;
    let samples = if version == 3 { 1152.0 } else { 576.0 };
    let mono = (hdr[3] >> 6) == 3;
    let side = match (version == 3, mono) {
        (true, false) => 32,
        (true, true) => 17,
        (false, false) => 17,
        (false, true) => 9,
    };
    let x = i + 4 + side;
    if let Some(tag) = h.get(x..x + 4)
        && (tag == b"Xing" || tag == b"Info")
        && be32(h, x + 4)? & 1 != 0
    {
        return Some(be32(h, x + 8)? as f64 * samples / rate);
    }
    if h.get(i + 36..i + 40) == Some(b"VBRI") {
        return Some(be32(h, i + 36 + 14)? as f64 * samples / rate);
    }
    let kbps = if version == 3 { BR1[br_idx] } else { BR2[br_idx] } as f64;
    Some((size - i as u64) as f64 * 8.0 / (kbps * 1000.0))
}

pub mod cmd {
    use super::NodeProps;
    use crate::error::{ApiError, ApiResult};
    use crate::state::AppState;
    use nv_fs::NodeId;
    use std::sync::Arc;
    use tauri::State;

    type St<'a> = State<'a, Arc<AppState>>;

    async fn blocking<T: Send + 'static>(st: &St<'_>, f: impl FnOnce(&AppState) -> ApiResult<T> + Send + 'static) -> ApiResult<T> {
        let s = st.inner().clone();
        tauri::async_runtime::spawn_blocking(move || f(&s)).await.map_err(|e| ApiError::new("error", format!("Internal error: {e}")))?
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn fs_props(st: St<'_>, vault: String, node: NodeId) -> ApiResult<NodeProps> {
        blocking(&st, move |s| s.fs_props(&vault, node)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn fs_props_tree(st: St<'_>, vault: String, nodes: Vec<NodeId>) -> ApiResult<String> {
        blocking(&st, move |s| s.fs_props_tree(&vault, nodes)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn fs_hash(st: St<'_>, vault: String, node: NodeId) -> ApiResult<String> {
        blocking(&st, move |s| s.fs_hash(&vault, node)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: u64, _: usize) -> Vec<u8> {
        vec![]
    }

    #[test]
    fn sniffs_common_types() {
        let png = [b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".as_slice(), &640u32.to_be_bytes(), &480u32.to_be_bytes()].concat();
        assert_eq!(sniff(&png), Some("image/png"));
        assert_eq!(image_size(&png, none), Some((640, 480)));
        let gif = [b"GIF89a".as_slice(), &[0x20, 0x03, 0x58, 0x02]].concat();
        assert_eq!(image_size(&gif, none), Some((800, 600)));
        assert_eq!(sniff(b"%PDF-1.7\n"), Some("application/pdf"));
        assert_eq!(sniff(b"hello world\nline two\n"), Some("text/plain"));
        assert_eq!(sniff("città è bella".as_bytes()), Some("text/plain"));
        assert_eq!(sniff(&[0, 1, 2, 3, 200, 7]), None);
        assert_eq!(sniff(b"PK\x03\x04....word/document.xml"), Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"));
        assert_eq!(sniff(b"\0\0\0\x18ftypqt  \0\0\0\0"), Some("video/quicktime"));
        assert_eq!(sniff(b"\0\0\0\x18ftypisom\0\0\0\0"), Some("video/mp4"));
        assert_eq!(sniff(b"NICEVLT1\0\x01"), Some("application/x-nicevault"));
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn jpeg_frame_after_segments() {
        // SOI, APP0 (16 bytes), SOF0 with 480x640 (h, w)
        let mut j = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10];
        j.extend([0u8; 14]);
        j.extend([0xff, 0xc0, 0x00, 0x11, 0x08, 0x01, 0xe0, 0x02, 0x80, 0x03]);
        let read = |off: u64, len: usize| j.get(off as usize..(off as usize + len).min(j.len())).unwrap_or_default().to_vec();
        assert_eq!(image_size(&j.clone(), read), Some((640, 480)));
    }

    #[test]
    fn webp_and_bmp_sizes() {
        let mut vp8x = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\0\0\0\0".to_vec();
        vp8x.extend([0x7f, 0x07, 0x00, 0x37, 0x04, 0x00]); // 1920 x 1080
        assert_eq!(image_size(&vp8x, none), Some((1920, 1080)));
        let mut bmp = b"BM".to_vec();
        bmp.extend([0u8; 12]);
        bmp.extend(40u32.to_le_bytes());
        bmp.extend(300i32.to_le_bytes());
        bmp.extend((-200i32).to_le_bytes());
        assert_eq!(image_size(&bmp, none), Some((300, 200)));
    }

    #[test]
    fn wav_and_flac_durations() {
        let mut w = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x02\0\x44\xac\0\0".to_vec();
        w.extend(176_400u32.to_le_bytes()); // byte rate
        w.extend([4, 0, 16, 0]);
        w.extend(b"data");
        w.extend((176_400u32 * 3).to_le_bytes());
        assert_eq!(media_duration(&w, 100, none), Some(3000));

        // fLaC, STREAMINFO block header (last=0, type 0, len 34), 44100 Hz, 441000 samples = 10 s
        let mut f = b"fLaC\0\0\0\x22".to_vec();
        f.extend([0u8; 10]);
        let rate: u32 = 44_100;
        let total: u64 = 441_000;
        f.extend([(rate >> 12) as u8, (rate >> 4) as u8, ((rate & 0xf) << 4) as u8 | 0x02, ((total >> 32) & 0xf) as u8 | 0xf0]);
        f.extend((total as u32).to_be_bytes());
        f.extend([0u8; 16]);
        assert_eq!(media_duration(&f, 1000, none), Some(10_000));
    }

    #[test]
    fn mp4_mvhd() {
        // ftyp(16) + moov(8 + mvhd v0: 8 + 100)
        let mut b = b"\0\0\0\x10ftypisom\0\0\0\0".to_vec();
        let mut mvhd = vec![0u8; 100];
        mvhd[0] = 0; // version
        mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes()); // timescale
        mvhd[16..20].copy_from_slice(&125_500u32.to_be_bytes()); // duration
        let mvhd_box = [&(108u32).to_be_bytes()[..], b"mvhd", &mvhd].concat();
        b.extend((8 + mvhd_box.len() as u32).to_be_bytes());
        b.extend(b"moov");
        b.extend(&mvhd_box);
        let size = b.len() as u64;
        let read = |off: u64, len: usize| b.get(off as usize..(off as usize + len).min(b.len())).unwrap_or_default().to_vec();
        assert_eq!(media_duration(&b.clone(), size, read), Some(125_500));
    }

    #[test]
    fn webm_info_duration() {
        // EBML header (empty), Segment (unknown size), Info { TimecodeScale 1_000_000, Duration f64 2500.0 ms }
        let mut b = vec![0x1a, 0x45, 0xdf, 0xa3, 0x87, 0x42, 0x82, 0x84, b'w', b'e', b'b', b'm'];
        b.extend([0x18, 0x53, 0x80, 0x67, 0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
        let mut info = vec![0x2a, 0xd7, 0xb1, 0x83, 0x0f, 0x42, 0x40];
        info.extend([0x44, 0x89, 0x88]);
        info.extend(2500f64.to_be_bytes());
        b.extend([0x15, 0x49, 0xa9, 0x66, 0x80 | info.len() as u8]);
        b.extend(&info);
        assert_eq!(sniff(&b), Some("video/webm"));
        assert_eq!(media_duration(&b, 1 << 20, none), Some(2500));
    }

    #[test]
    fn mp3_cbr_estimate() {
        // MPEG1 layer III, 128 kbps, 44.1 kHz, stereo; 1 MB of it ~ 65.5 s
        let mut b = vec![0xff, 0xfb, 0x90, 0x00];
        b.extend([0u8; 200]);
        assert_eq!(sniff(&b), Some("audio/mpeg"));
        let d = media_duration(&b, 1_048_576, none).unwrap();
        assert!((65_000..66_000).contains(&d), "{d}");
    }
}
