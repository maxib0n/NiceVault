//! Volume emblem: the big mark on the volume's "security document". Stored privately inside
//! the encrypted volume (`meta` key [`META_KEY`], with `show_when_locked` = the volume's
//! appearance visibility). When visible, a copy goes to the header's plaintext appearance
//! section (appearance.rs) so the locked screens can show it.

use crate::error::{ApiError, ApiResult};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};

pub const META_KEY: &str = "nicevault.emblem";
/// Frames drawn round the emblem (ids in app/ui/src/ornament/frames.ts). First = default, the
/// guilloché rosette every volume had before frames could be chosen.
pub const FRAMES: [&str; 6] = ["guilloche", "laurel", "rope", "meander", "starburst", "hairline"];
/// Decoded size limit of a custom image (the UI makes a 256x256 WebP/PNG well under this).
pub const MAX_IMAGE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Emblem {
    /// The volume's initial letter (monogram).
    #[default]
    Initial,
    /// One of the UI's curated icons (ids in app/ui/src/lib/emblem.ts).
    Icon { id: String },
    /// A square PNG or WebP image, base64 (standard alphabet, padded).
    Image { data_base64: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmblemState {
    pub emblem: Emblem,
    pub show_when_locked: bool,
    /// The frame round the emblem (one of [`FRAMES`], or a newer build's id: kept as it is, drawn
    /// as the default). Absent in records written before frames: the guilloché rosette.
    #[serde(default = "default_frame")]
    pub emblem_frame: String,
}

pub fn default_frame() -> String {
    FRAMES[0].to_owned()
}

impl Default for EmblemState {
    fn default() -> Self {
        Self { emblem: Emblem::Initial, show_when_locked: true, emblem_frame: default_frame() }
    }
}

/// Checks a frame id coming from the UI: `[a-z0-9-]`, 1..=32 bytes (a newer build's frame is
/// stored as it is; this build draws an id it does not know as the default rosette).
pub fn validate_frame(id: &str) -> ApiResult<String> {
    let ok = !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok { Ok(id.to_owned()) } else { Err(bad("Invalid frame.")) }
}

fn bad(msg: &str) -> ApiError {
    ApiError::new("invalid_emblem", msg)
}

/// Checks and canonicalizes an emblem coming from the UI.
pub fn validate(e: Emblem) -> ApiResult<Emblem> {
    match e {
        Emblem::Initial => Ok(Emblem::Initial),
        Emblem::Icon { id } => {
            let ok = !id.is_empty() && id.len() <= 40 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
            if ok { Ok(Emblem::Icon { id }) } else { Err(bad("Invalid icon.")) }
        }
        Emblem::Image { data_base64 } => {
            let raw = data_base64.split_once(',').filter(|(p, _)| p.starts_with("data:")).map_or(&*data_base64, |(_, d)| d);
            let bytes = B64.decode(raw.trim()).map_err(|_| bad("Unreadable image."))?;
            if bytes.len() > MAX_IMAGE_BYTES {
                return Err(bad("Immagine troppo grande (massimo 64 KB)."));
            }
            let png = bytes.starts_with(b"\x89PNG\r\n\x1a\n");
            let webp = bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP";
            if !png && !webp {
                return Err(bad("L’immagine deve essere PNG o WebP."));
            }
            Ok(Emblem::Image { data_base64: B64.encode(bytes) })
        }
    }
}

/// What the volume holds: `None` when the record exists but this build cannot read it (written
/// by a newer build with an emblem kind or shape it does not know, or unreadable). Such a record
/// is never rewritten implicitly and the header section is left as it is (see
/// `appearance::sync_header`); only an explicit new emblem from the user replaces it.
/// An absent record is the default (initial, visible): volumes made before emblems existed.
pub fn stored(v: &nv_core::Vault) -> Option<EmblemState> {
    match v.meta_get(META_KEY) {
        Ok(None) => Some(EmblemState::default()),
        Ok(Some(b)) => serde_json::from_slice(&b).ok(),
        Err(_) => None,
    }
}

/// The state to show: an unknown record ([`stored`] = `None`) shows as the neutral look
/// (initial, encrypted) — never as "visible".
pub fn load(v: &nv_core::Vault) -> EmblemState {
    stored(v).unwrap_or(EmblemState { emblem: Emblem::Initial, show_when_locked: false, emblem_frame: default_frame() })
}

/// Patches the stored record: only the given parts are replaced, every other key (a newer
/// build's) is kept. Refused (`unsupported_version`) when the result would not be a complete
/// record this build can read, i.e. changing only the visibility of an unknown emblem.
pub fn save(v: &nv_core::Vault, emblem: Option<&Emblem>, show_when_locked: Option<bool>, frame: Option<&str>) -> ApiResult<()> {
    use serde_json::Value;
    let err = |e: serde_json::Error| ApiError::new("error", e.to_string());
    let mut obj = match v.meta_get(META_KEY)?.map(|b| serde_json::from_slice::<Value>(&b)) {
        None => match serde_json::to_value(EmblemState::default()).map_err(err)? {
            Value::Object(m) => m,
            _ => unreachable!("a struct serializes to an object"),
        },
        Some(Ok(Value::Object(m))) => m,
        Some(_) => serde_json::Map::new(),
    };
    if let Some(e) = emblem {
        obj.insert("emblem".into(), serde_json::to_value(e).map_err(err)?);
    }
    if let Some(s) = show_when_locked {
        obj.insert("show_when_locked".into(), Value::Bool(s));
    }
    if let Some(f) = frame {
        obj.insert("emblem_frame".into(), Value::String(f.to_owned()));
    }
    let obj = Value::Object(obj);
    if serde_json::from_value::<EmblemState>(obj.clone()).is_err() {
        return Err(ApiError::new(
            "unsupported_version",
            "This volume's emblem was saved by a newer version of NiceVault: pick a new emblem to change it here.",
        ));
    }
    Ok(v.meta_set(META_KEY, Some(&serde_json::to_vec(&obj).map_err(err)?))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates() {
        assert!(validate(Emblem::Icon { id: "house".into() }).is_ok());
        assert!(validate(Emblem::Icon { id: "../x".into() }).is_err());
        let png = B64.encode(b"\x89PNG\r\n\x1a\nrest");
        assert_eq!(validate(Emblem::Image { data_base64: format!("data:image/png;base64,{png}") }).unwrap(), Emblem::Image { data_base64: png });
        assert!(validate(Emblem::Image { data_base64: B64.encode(b"GIF89a....") }).is_err());
        let big = [b"\x89PNG\r\n\x1a\n".as_slice(), &vec![0u8; MAX_IMAGE_BYTES]].concat();
        assert!(validate(Emblem::Image { data_base64: B64.encode(big) }).is_err());
        let j = serde_json::to_string(&Emblem::Icon { id: "key".into() }).unwrap();
        assert_eq!(j, r#"{"kind":"icon","id":"key"}"#);
    }

    /// The frame is additive: a record written before it reads the default rosette; ids are
    /// checked for their charset only (a newer build's frame survives).
    #[test]
    fn frames() {
        let old: EmblemState = serde_json::from_str(r#"{"emblem":{"kind":"initial"},"show_when_locked":true}"#).unwrap();
        assert_eq!(old.emblem_frame, "guilloche");
        let new: EmblemState = serde_json::from_str(r#"{"emblem":{"kind":"initial"},"show_when_locked":true,"emblem_frame":"laurel"}"#).unwrap();
        assert_eq!(new.emblem_frame, "laurel");
        assert!(validate_frame("starburst").is_ok() && validate_frame("future-frame-2").is_ok());
        assert!(validate_frame("").is_err() && validate_frame("../x").is_err() && validate_frame(&"a".repeat(33)).is_err());
    }
}
