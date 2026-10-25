//! Grid thumbnails at the size they are shown: `nvault://<vault>/<node>?thumb=<px>` answers a
//! JPEG (PNG when the picture has transparency) whose longer side is at most `px`, decoded,
//! oriented (EXIF) and scaled in memory. The UI asks for the bucket matching the tile size and
//! the device pixel ratio (S/M/L/XL), so a folder of 24-megapixel photos does not decode 24
//! megapixels per tile. Kept in a small in-memory cache per open volume, wiped when it locks;
//! nothing touches disk. Anything that cannot be thumbnailed (not an image, too big, an
//! animation, already small) falls back to the original bytes (preview.rs).
//! `?fit=<px>`: the same for a picture looked at whole (quick look): sized to the window
//! (FIT_BUCKETS), resampled with Lanczos and kept at JPEG quality 92, so a 12-megapixel photo is
//! not decoded and uploaded to the GPU at full size to be shown at a twelfth of it.

use crate::state::lock_or_recover;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use nv_core::Vault;
use nv_fs::{Fs, Node, NodeId};
use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{Condvar, Mutex};

/// Sizes a thumbnail can have (longer side, device px); requests are rounded up to one.
pub const BUCKETS: [u32; 6] = [128, 256, 384, 512, 768, 1024];
/// Sizes of a `fit` request (longer side, device px).
pub const FIT_BUCKETS: [u32; 4] = [1280, 1600, 2048, 2560];
/// Marks a `fit` size in a cache entry / a request (`px | FIT`).
pub const FIT: u32 = 1 << 31;
/// Source images larger than this are served as they are.
const MAX_SOURCE: u64 = 48 << 20;
/// Memory for cached thumbnails (all volumes).
const CACHE_BYTES: usize = 48 << 20;
/// Decodes running at once (a big JPEG takes ~4 bytes per pixel while it is scaled).
const PARALLEL: usize = 3;

pub fn bucket(px: u32) -> u32 {
    BUCKETS.iter().copied().find(|&b| b >= px).unwrap_or(BUCKETS[BUCKETS.len() - 1])
}

/// `fit=<px>` in a query string, as `bucket | FIT`; None past the largest bucket (the original).
pub fn requested_fit(query: Option<&str>) -> Option<u32> {
    let px = query?.split('&').find_map(|kv| kv.strip_prefix("fit=")).and_then(|v| v.parse::<u32>().ok()).filter(|&p| p > 0)?;
    FIT_BUCKETS.iter().copied().find(|&b| b >= px).map(|b| b | FIT)
}

/// `thumb=<px>` in a query string.
pub fn requested(query: Option<&str>) -> Option<u32> {
    query?.split('&').find_map(|kv| kv.strip_prefix("thumb=")).and_then(|v| v.parse::<u32>().ok()).filter(|&p| p > 0).map(bucket)
}

struct Entry {
    vault: String,
    node: NodeId,
    mtime: i64,
    size: u64,
    px: u32,
    mime: &'static str,
    bytes: Vec<u8>,
}

static CACHE: Mutex<VecDeque<Entry>> = Mutex::new(VecDeque::new());
static SLOTS: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

/// Drops every cached thumbnail of a volume (it locked).
pub fn forget_vault(vault: &str) {
    lock_or_recover(&CACHE).retain(|e| e.vault != vault);
}

fn cached(vault: &str, n: &Node, px: u32) -> Option<(Vec<u8>, &'static str)> {
    let c = lock_or_recover(&CACHE);
    c.iter().find(|e| e.vault == vault && e.node == n.id && e.mtime == n.mtime && e.size == n.size && e.px == px).map(|e| (e.bytes.clone(), e.mime))
}

fn remember(vault: &str, n: &Node, px: u32, mime: &'static str, bytes: &[u8]) {
    let mut c = lock_or_recover(&CACHE);
    c.retain(|e| !(e.vault == vault && e.node == n.id && e.px == px));
    c.push_back(Entry { vault: vault.to_owned(), node: n.id, mtime: n.mtime, size: n.size, px, mime, bytes: bytes.to_vec() });
    let mut total: usize = c.iter().map(|e| e.bytes.len()).sum();
    while total > CACHE_BYTES {
        match c.pop_front() {
            Some(e) => total -= e.bytes.len(),
            None => break,
        }
    }
}

/// The thumbnail of image node `n`, or None (serve the original).
pub fn thumbnail(vault_id: &str, v: &Vault, n: &Node, px: u32) -> Option<(Vec<u8>, &'static str)> {
    if n.size == 0 || n.size > MAX_SOURCE {
        return None;
    }
    if let Some(hit) = cached(vault_id, n, px) {
        return Some(hit);
    }
    // a few at a time: the webview asks for a whole folder at once
    {
        let mut busy = lock_or_recover(&SLOTS.0);
        while *busy >= PARALLEL {
            busy = SLOTS.1.wait(busy).unwrap_or_else(|e| e.into_inner());
        }
        *busy += 1;
    }
    let r = (|| {
        let mut src = vec![0u8; n.size as usize];
        let mut got = 0;
        while got < src.len() {
            match v.read(n.id, got as u64, &mut src[got..]) {
                Ok(0) | Err(_) => break,
                Ok(k) => got += k,
            }
        }
        src.truncate(got);
        let size = px & !FIT;
        let out = if decodable(&src) {
            if px & FIT != 0 { scale_fine(&src, size)? } else { scale(&src, size)? }
        } else {
            native(&src, size)?
        };
        remember(vault_id, n, px, out.1, &out.0);
        Some(out)
    })();
    *lock_or_recover(&SLOTS.0) -= 1;
    SLOTS.1.notify_one();
    r
}

/// Whether `scale` / `scale_fine` read this format (JPEG, PNG, WebP, BMP).
fn decodable(src: &[u8]) -> bool {
    ImageReader::new(Cursor::new(src))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.format())
        .is_some_and(|f| matches!(f, image::ImageFormat::Jpeg | image::ImageFormat::Png | image::ImageFormat::WebP | image::ImageFormat::Bmp))
}

/// A picture the `image` crate cannot read (HEIC/HEIF, AVIF, TIFF, DNG) made small by the
/// system: on iOS ImageIO (app/ios NvThumb.swift), as a JPEG. Elsewhere None (the original is
/// served; a desktop web view cannot show a HEIC anyway and draws the sheet).
#[cfg(target_os = "ios")]
fn native(src: &[u8], px: u32) -> Option<(Vec<u8>, &'static str)> {
    unsafe extern "C" {
        fn nv_imageio_thumbnail(src: *const u8, len: usize, px: u32, out_len: *mut usize) -> *mut u8;
        fn nv_imageio_free(p: *mut u8, len: usize);
    }
    let mut n = 0usize;
    // SAFETY: `src` is valid for `len` bytes for the call (ImageIO reads it without keeping it);
    // the returned buffer of `n` bytes is ours until `nv_imageio_free`.
    unsafe {
        let p = nv_imageio_thumbnail(src.as_ptr(), src.len(), px, &mut n);
        if p.is_null() || n == 0 {
            return None;
        }
        let out = std::slice::from_raw_parts(p, n).to_vec();
        nv_imageio_free(p, n);
        Some((out, "image/jpeg"))
    }
}

#[cfg(not(target_os = "ios"))]
fn native(_src: &[u8], _px: u32) -> Option<(Vec<u8>, &'static str)> {
    None
}

/// Decodes `src` (JPEG, PNG, WebP, BMP), applies its EXIF orientation and scales it so the
/// longer side is `px`; None when it is not one of those, is already that small, or is animated.
pub fn scale(src: &[u8], px: u32) -> Option<(Vec<u8>, &'static str)> {
    resample(src, px, false)
}

/// `scale` for a picture looked at whole: Lanczos resampling, JPEG quality 92.
pub fn scale_fine(src: &[u8], px: u32) -> Option<(Vec<u8>, &'static str)> {
    resample(src, px, true)
}

fn resample(src: &[u8], px: u32, fine: bool) -> Option<(Vec<u8>, &'static str)> {
    let mut reader = ImageReader::new(Cursor::new(src)).with_guessed_format().ok()?;
    let format = reader.format()?;
    if !matches!(format, image::ImageFormat::Jpeg | image::ImageFormat::Png | image::ImageFormat::WebP | image::ImageFormat::Bmp) {
        return None;
    }
    if format == image::ImageFormat::Png && src.windows(4).take(4096).any(|w| w == b"acTL") {
        return None; // animated PNG: keep it moving
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(30_000);
    limits.max_image_height = Some(30_000);
    limits.max_alloc = Some(512 << 20);
    reader.limits(limits);
    let mut dec = reader.into_decoder().ok()?;
    let (w, h) = dec.dimensions();
    if w.max(h) <= px {
        return None;
    }
    let orientation = dec.orientation().ok();
    let mut img = DynamicImage::from_decoder(dec).ok()?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    let small = if fine { img.resize(px, px, image::imageops::FilterType::Lanczos3) } else { img.thumbnail(px, px) };
    let mut out = Cursor::new(Vec::new());
    if small.color().has_alpha() {
        small.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Some((out.into_inner(), "image/png"))
    } else {
        let rgb = small.into_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, if fine { 92 } else { 84 }).encode_image(&rgb).ok()?;
        Some((out.into_inner(), "image/jpeg"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_and_queries() {
        assert_eq!(bucket(1), 128);
        assert_eq!(bucket(300), 384);
        assert_eq!(bucket(5000), 1024);
        assert_eq!(requested(Some("thumb=200")), Some(256));
        assert_eq!(requested(Some("a=1&thumb=512")), Some(512));
        assert_eq!(requested(Some("thumb=0")), None);
        assert_eq!(requested(Some("thumb=x")), None);
        assert_eq!(requested(None), None);
        assert_eq!(requested_fit(Some("fit=1500")), Some(1600 | FIT));
        assert_eq!(requested_fit(Some("fit=1")), Some(1280 | FIT));
        assert_eq!(requested_fit(Some("fit=4000")), None, "past the largest: the original");
        assert_eq!(requested(Some("fit=1500")), None);
    }

    #[test]
    fn heic_goes_to_the_system() {
        // an ftyp/heic box: not something `image` reads, so it goes to `native` (ImageIO on iOS)
        let heic = [&[0, 0, 0, 24][..], b"ftypheic", &[0, 0, 0, 0], b"mif1heic"].concat();
        assert!(!decodable(&heic));
        let mut jpg = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image::RgbImage::new(8, 8)).write_to(&mut jpg, image::ImageFormat::Jpeg).unwrap();
        assert!(decodable(jpg.get_ref()));
        #[cfg(not(target_os = "ios"))]
        assert!(native(&heic, 256).is_none(), "elsewhere the original is served");
    }

    #[test]
    fn fine_scale_keeps_the_aspect() {
        let mut big = image::RgbImage::new(3000, 2000);
        for (x, y, p) in big.enumerate_pixels_mut() {
            *p = image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]);
        }
        let mut jpg = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(big).write_to(&mut jpg, image::ImageFormat::Jpeg).unwrap();
        let (bytes, mime) = scale_fine(jpg.get_ref(), 1280).unwrap();
        assert_eq!(mime, "image/jpeg");
        let t = image::load_from_memory(&bytes).unwrap();
        assert_eq!((t.width(), t.height()), (1280, 853));
        assert!(scale_fine(jpg.get_ref(), 4096).is_none(), "already small enough");
    }

    #[test]
    fn scales_and_falls_back() {
        let mut big = image::RgbImage::new(800, 400);
        for (x, _, p) in big.enumerate_pixels_mut() {
            *p = image::Rgb([(x % 256) as u8, 40, 90]);
        }
        let mut png = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(big).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let (bytes, mime) = scale(png.get_ref(), 256).unwrap();
        assert_eq!(mime, "image/jpeg", "opaque: JPEG");
        let t = image::load_from_memory(&bytes).unwrap();
        assert_eq!((t.width(), t.height()), (256, 128));
        assert!(scale(png.get_ref(), 1024).is_none(), "already small enough");
        assert!(scale(b"not an image", 128).is_none());

        let rgba = DynamicImage::ImageRgba8(image::RgbaImage::new(600, 600));
        let mut p = Cursor::new(Vec::new());
        rgba.write_to(&mut p, image::ImageFormat::Png).unwrap();
        assert_eq!(scale(p.get_ref(), 128).unwrap().1, "image/png", "transparency kept");
    }
}
