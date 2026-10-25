//! The volume's optional plaintext "appearance" (accent + a small locked-screen emblem),
//! kept in the header so a locked volume can look like itself without a password.
//! Display-only and unauthenticated until unlock; the keyed MAC is checked by `Vault::open`.

use serde::{Deserialize, Serialize};

/// Longest accent preset id, bytes (`[a-z0-9-]`).
pub const MAX_ACCENT_ID: usize = 32;
/// Longest icon id, bytes (`[a-z0-9-]`).
pub const MAX_ICON_ID: usize = 40;
/// Longest `Initial` text, bytes (one character, any script).
pub const MAX_INITIAL: usize = 8;
/// Largest header emblem image (PNG or WebP), bytes.
pub const MAX_LOCKED_IMAGE: usize = 24 * 1024;
/// Largest header emblem image side, pixels.
pub const MAX_LOCKED_SIDE: u32 = 128;

/// What the locked screens may show. Variant order is part of the format (postcard tags).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockedEmblem {
    /// Monogram: the volume's initial (at most one character, <= [`MAX_INITIAL`] bytes; may be empty).
    Initial(String),
    /// A curated icon id (`[a-z0-9-]{1,40}`).
    Icon(String),
    /// A PNG or WebP, <= [`MAX_LOCKED_IMAGE`] bytes and <= [`MAX_LOCKED_SIDE`] px per side.
    Image(Vec<u8>),
}

/// The plaintext appearance section of the header. Field order is part of the format.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Appearance {
    /// Accent preset id (the app's list; the core only checks the charset and length).
    pub accent: Option<String>,
    pub emblem: Option<LockedEmblem>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppearanceError {
    #[error("accent id must be [a-z0-9-], 1..={MAX_ACCENT_ID} bytes")]
    Accent,
    #[error("icon id must be [a-z0-9-], 1..={MAX_ICON_ID} bytes")]
    Icon,
    #[error("initial must be one character, <= {MAX_INITIAL} bytes")]
    Initial,
    #[error("emblem image is over {MAX_LOCKED_IMAGE} bytes")]
    ImageTooLarge,
    #[error("emblem image must be PNG or WebP")]
    ImageFormat,
    #[error("emblem image must be at most {MAX_LOCKED_SIDE}x{MAX_LOCKED_SIDE} pixels")]
    ImageSide,
    /// Writing the header failed, or the volume is locked.
    #[error(transparent)]
    Fs(#[from] nv_fs::Error),
}

fn id_ok(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Appearance {
    /// Checks every cap. A valid appearance always fits the header next to 8 full slots.
    pub fn validate(&self) -> Result<(), AppearanceError> {
        if let Some(a) = &self.accent
            && !id_ok(a, MAX_ACCENT_ID)
        {
            return Err(AppearanceError::Accent);
        }
        match &self.emblem {
            None => Ok(()),
            Some(LockedEmblem::Initial(s)) => {
                if s.len() <= MAX_INITIAL && s.chars().count() <= 1 { Ok(()) } else { Err(AppearanceError::Initial) }
            }
            Some(LockedEmblem::Icon(id)) => {
                if id_ok(id, MAX_ICON_ID) { Ok(()) } else { Err(AppearanceError::Icon) }
            }
            Some(LockedEmblem::Image(b)) => {
                if b.len() > MAX_LOCKED_IMAGE {
                    return Err(AppearanceError::ImageTooLarge);
                }
                let (w, h) = image_size(b).ok_or(AppearanceError::ImageFormat)?;
                if w == 0 || h == 0 || w > MAX_LOCKED_SIDE || h > MAX_LOCKED_SIDE {
                    return Err(AppearanceError::ImageSide);
                }
                Ok(())
            }
        }
    }
}

/// Width and height of a PNG or WebP (lossy, lossless or extended), from its header bytes.
pub fn image_size(b: &[u8]) -> Option<(u32, u32)> {
    let be = |o: usize| b.get(o..o + 4).map(|s| u32::from_be_bytes(s.try_into().unwrap()));
    let le16 = |o: usize| b.get(o..o + 2).map(|s| u16::from_le_bytes(s.try_into().unwrap()) as u32);
    let le24 = |o: usize| b.get(o..o + 3).map(|s| s[0] as u32 | (s[1] as u32) << 8 | (s[2] as u32) << 16);
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        // The first chunk must be IHDR: length(4) "IHDR" width(4) height(4).
        if b.get(12..16)? != b"IHDR" {
            return None;
        }
        return Some((be(16)?, be(20)?));
    }
    if b.len() < 16 || &b[..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return None;
    }
    match b.get(12..16)? {
        b"VP8 " => {
            // frame tag(3) start code 9d 01 2a, then 14-bit width / height.
            if b.get(23..26)? != [0x9d, 0x01, 0x2a] {
                return None;
            }
            Some((le16(26)? & 0x3fff, le16(28)? & 0x3fff))
        }
        b"VP8L" => {
            if *b.get(20)? != 0x2f {
                return None;
            }
            let bits = u32::from_le_bytes(b.get(21..25)?.try_into().ok()?);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => Some((le24(24)? + 1, le24(27)? + 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn png(w: u32, h: u32, len: usize) -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.resize(len.max(v.len()), 0);
        v
    }

    #[test]
    fn sizes() {
        assert_eq!(image_size(&png(128, 64, 40)), Some((128, 64)));
        // VP8L 100x50: width-1 = 99, height-1 = 49
        let bits: u32 = 99 | 49 << 14;
        let mut l = b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0\x2f".to_vec();
        l.extend_from_slice(&bits.to_le_bytes());
        assert_eq!(image_size(&l), Some((100, 50)));
        let mut x = b"RIFF\0\0\0\0WEBPVP8X\0\0\0\0\0\0\0\0".to_vec();
        x.extend_from_slice(&[127, 0, 0, 255, 0, 0]);
        assert_eq!(image_size(&x), Some((128, 256)));
        let mut y = b"RIFF\0\0\0\0WEBPVP8 \0\0\0\0\0\0\0\x9d\x01\x2a".to_vec();
        y.extend_from_slice(&[64, 0, 32, 0]);
        assert_eq!(image_size(&y), Some((64, 32)));
        assert_eq!(image_size(b"GIF89a"), None);
    }

    #[test]
    fn caps() {
        let ok = |e| Appearance { accent: Some("prugna".into()), emblem: Some(e) }.validate().map_err(|e| e.to_string());
        let err = |e: AppearanceError| Err(e.to_string());
        assert!(ok(LockedEmblem::Initial("É".into())).is_ok());
        assert!(ok(LockedEmblem::Initial(String::new())).is_ok());
        assert_eq!(ok(LockedEmblem::Initial("AB".into())), err(AppearanceError::Initial));
        assert!(ok(LockedEmblem::Icon("key-round".into())).is_ok());
        assert_eq!(ok(LockedEmblem::Icon("../x".into())), err(AppearanceError::Icon));
        assert!(ok(LockedEmblem::Image(png(128, 128, MAX_LOCKED_IMAGE))).is_ok());
        assert_eq!(ok(LockedEmblem::Image(png(128, 128, MAX_LOCKED_IMAGE + 1))), err(AppearanceError::ImageTooLarge));
        assert_eq!(ok(LockedEmblem::Image(png(256, 256, 100))), err(AppearanceError::ImageSide));
        assert_eq!(ok(LockedEmblem::Image(b"GIF89a....".to_vec())), err(AppearanceError::ImageFormat));
        let bad = Appearance { accent: Some("#0047ff".into()), emblem: None };
        assert!(matches!(bad.validate(), Err(AppearanceError::Accent)));
    }
}
