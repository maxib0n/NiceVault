//! Importers: Bitwarden unencrypted JSON export, KeePass KDBX 3.1 / 4 (opened here with its
//! master password and/or key file) and generic CSV (Chrome / Edge / Brave / Opera, Firefox,
//! Safari, 1Password, Bitwarden, LastPass, KeePassXC, Proton Pass, Dashlane, NordPass, RoboForm
//! style headers). They only parse into [`ImportedEntry`]s; nothing touches the disk. Known
//! bookkeeping fields (website lists, times, TOTP secrets, card numbers, noise) are mapped by
//! [`super::tidy`] instead of being kept as custom fields.

use super::tidy::{self, Act};
use super::totp::{Algorithm, Totp, base32_encode};
use super::{EntryPatch, FieldInput, ImportedEntry};
use serde::Deserialize;
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ImportError {
    #[error("wrong master password or key file")]
    WrongKey,
    #[error("encrypted exports are not supported: export unencrypted JSON")]
    Encrypted,
    #[error("unrecognised file: {0}")]
    Format(String),
}

pub type ImportResult = Result<Vec<ImportedEntry>, ImportError>;

fn z(s: impl Into<String>) -> Option<Zeroizing<String>> {
    Some(Zeroizing::new(s.into()))
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// A title for an entry that has none: the URL's host, else the user name.
fn fallback_title(urls: &[String], username: &str) -> String {
    for u in urls {
        let rest = u.split_once("://").map_or(u.as_str(), |(_, r)| r);
        let host = rest.split(['/', '?', '#']).next().unwrap_or("").rsplit('@').next().unwrap_or("");
        let host = host.split(':').next().unwrap_or("").trim_start_matches("www.");
        if !host.is_empty() {
            return host.to_owned();
        }
    }
    if !username.trim().is_empty() { username.trim().to_owned() } else { "Senza titolo".to_owned() }
}

/// Valid TOTP -> stored as such; anything else is kept as a hidden custom field so nothing is lost.
fn put_totp(p: &mut EntryPatch, raw: &str) {
    let raw = raw.trim();
    if raw.is_empty() {
        return;
    }
    if Totp::parse(raw).is_ok() {
        p.totp = z(raw);
    } else {
        p.fields.get_or_insert_with(Vec::new).push(FieldInput { name: "TOTP".into(), value: z(raw), hidden: true });
    }
}

/// Maps the known bookkeeping fields of `e` (see [`tidy`]); the entry's own times win over
/// time fields.
fn tidy_imported(e: &mut ImportedEntry) {
    let fields = e.entry.fields.take().unwrap_or_default();
    let mut found = tidy::Found::default();
    let has_totp = e.entry.totp.is_some();
    let mut kept = Vec::with_capacity(fields.len());
    for mut f in fields {
        let v = f.value.as_deref().map(String::as_str).unwrap_or("");
        match tidy::field(&f.name, v, has_totp, &mut found) {
            Act::Keep => kept.push(f),
            Act::Hide => {
                f.hidden = true;
                kept.push(f);
            }
            Act::Drop => {}
        }
    }
    if !kept.is_empty() {
        e.entry.fields = Some(kept);
    }
    if !found.urls.is_empty() {
        let urls = e.entry.urls.get_or_insert_with(Vec::new);
        for u in found.urls {
            if !urls.contains(&u) {
                urls.push(u);
            }
        }
    }
    if let Some(t) = found.totp {
        e.entry.totp = z(t);
    }
    e.created = e.created.or(found.created);
    e.updated = e.updated.or(found.updated).or(e.created);
    if let (Some(c), Some(u)) = (e.created, e.updated)
        && u < c
    {
        e.updated = Some(c);
    }
}

fn finish(mut p: EntryPatch) -> EntryPatch {
    let urls = p.urls.clone().unwrap_or_default();
    if p.title.as_deref().is_none_or(|t| t.trim().is_empty()) {
        p.title = Some(fallback_title(&urls, p.username.as_deref().unwrap_or("")));
    }
    p
}

pub(super) fn ms_of_rfc3339(s: &str) -> Option<i64> {
    // "2024-01-31T12:34:56.789Z" (Bitwarden). Minimal parser, UTC only.
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (num(0..4)?, num(5..7)?, num(8..10)?, num(11..13)?, num(14..16)?, num(17..19)?);
    let frac = s.get(19..).and_then(|r| r.strip_prefix('.')).map(|r| {
        let digits: String = r.chars().take_while(|c| c.is_ascii_digit()).take(3).collect();
        format!("{digits:0<3}").parse::<i64>().unwrap_or(0)
    });
    // days from civil (Howard Hinnant)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(((days * 86400 + h * 3600 + mi * 60 + se) * 1000) + frac.unwrap_or(0))
}

// ---------------- Bitwarden JSON ----------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BwExport {
    #[serde(default)]
    encrypted: bool,
    #[serde(default)]
    folders: Vec<BwFolder>,
    #[serde(default)]
    items: Option<Vec<BwItem>>,
}

#[derive(Deserialize)]
struct BwFolder {
    id: String,
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct BwItem {
    name: Option<String>,
    notes: Option<String>,
    favorite: bool,
    folder_id: Option<String>,
    login: Option<BwLogin>,
    card: Option<serde_json::Map<String, serde_json::Value>>,
    identity: Option<serde_json::Map<String, serde_json::Value>>,
    fields: Vec<BwField>,
    password_history: Option<Vec<BwHistory>>,
    creation_date: Option<String>,
    revision_date: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct BwLogin {
    username: Option<String>,
    password: Option<Zeroizing<String>>,
    totp: Option<Zeroizing<String>>,
    uris: Option<Vec<BwUri>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct BwUri {
    uri: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct BwField {
    name: Option<String>,
    value: Option<serde_json::Value>,
    /// 0 text, 1 hidden, 2 boolean, 3 linked
    #[serde(rename = "type")]
    kind: u8,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct BwHistory {
    last_used_date: Option<String>,
    password: Option<Zeroizing<String>>,
}

fn json_text(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub fn bitwarden_json(data: &[u8]) -> ImportResult {
    let ex: BwExport = serde_json::from_slice(data).map_err(|e| ImportError::Format(format!("Bitwarden JSON: {e}")))?;
    if ex.encrypted {
        return Err(ImportError::Encrypted);
    }
    let items = ex.items.ok_or_else(|| ImportError::Format("Bitwarden JSON: no items".into()))?;
    let folder = |id: &Option<String>| id.as_ref().and_then(|id| ex.folders.iter().find(|f| &f.id == id)).map(|f| f.name.clone());
    let mut out = Vec::new();
    for it in items {
        let mut p = EntryPatch {
            title: it.name.clone(),
            notes: it.notes.clone().map(Zeroizing::new),
            favorite: Some(it.favorite),
            folder: folder(&it.folder_id),
            ..Default::default()
        };
        let mut history = Vec::new();
        if let Some(l) = it.login {
            p.username = l.username;
            p.password = l.password;
            p.urls = Some(l.uris.unwrap_or_default().into_iter().filter_map(|u| u.uri).collect());
            if let Some(t) = l.totp {
                put_totp(&mut p, &t);
            }
        }
        let mut fields = p.fields.take().unwrap_or_default();
        // 3 = card, 4 = identity: their properties become custom fields (card number / code hidden).
        for (map, hidden) in [(it.card.as_ref(), ["number", "code"].as_slice()), (it.identity.as_ref(), ["ssn", "passportNumber", "licenseNumber"].as_slice())] {
            for (k, v) in map.into_iter().flatten() {
                if let Some(v) = json_text(v) {
                    fields.push(FieldInput { name: k.clone(), value: z(v), hidden: hidden.contains(&k.as_str()) });
                }
            }
        }
        for f in it.fields {
            let Some(name) = non_empty(f.name.as_deref()).map(str::to_owned) else { continue };
            if f.kind == 3 {
                continue; // linked: a pointer to the user name / password, not a value
            }
            let value = f.value.as_ref().and_then(json_text).unwrap_or_default();
            fields.push(FieldInput { name, value: z(value), hidden: f.kind == 1 });
        }
        if !fields.is_empty() {
            p.fields = Some(fields);
        }
        for h in it.password_history.unwrap_or_default() {
            if let Some(pw) = h.password {
                history.push((pw, h.last_used_date.as_deref().and_then(ms_of_rfc3339).unwrap_or(0)));
            }
        }
        let mut e = ImportedEntry {
            entry: p,
            history,
            created: it.creation_date.as_deref().and_then(ms_of_rfc3339),
            updated: it.revision_date.as_deref().and_then(ms_of_rfc3339),
        };
        tidy_imported(&mut e);
        e.entry = finish(e.entry);
        out.push(e);
    }
    Ok(out)
}

// ---------------- KeePass ----------------

const KP_STANDARD: [&str; 5] = ["Title", "UserName", "Password", "URL", "Notes"];

/// KeePass 2 TimeOtp-* fields / KeePassXC legacy "TOTP Seed" + "TOTP Settings" -> otpauth URI.
fn keepass_totp(get: &dyn Fn(&str) -> Option<String>) -> Option<(String, Vec<&'static str>)> {
    if let Some(uri) = get("otp") {
        return Some((uri, vec!["otp"]));
    }
    let secret = if let Some(s) = get("TimeOtp-Secret-Base32") {
        Some((s, "TimeOtp-Secret-Base32"))
    } else if let Some(s) = get("TimeOtp-Secret") {
        Some((base32_encode(s.as_bytes()), "TimeOtp-Secret"))
    } else if let Some(s) = get("TimeOtp-Secret-Hex") {
        let b: Option<Vec<u8>> = (0..s.len() / 2).map(|i| u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()).collect();
        b.map(|b| (base32_encode(&b), "TimeOtp-Secret-Hex"))
    } else if let Some(s) = get("TimeOtp-Secret-Base64") {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(s.trim()).ok().map(|b| (base32_encode(&b), "TimeOtp-Secret-Base64"))
    } else {
        None
    };
    if let Some((secret, key)) = secret {
        let alg = get("TimeOtp-Algorithm").and_then(|a| Algorithm::parse(&a)).unwrap_or(Algorithm::Sha1);
        let digits: u32 = get("TimeOtp-Length").and_then(|d| d.trim().parse().ok()).unwrap_or(6);
        let period: u64 = get("TimeOtp-Period").and_then(|d| d.trim().parse().ok()).unwrap_or(30);
        let alg = match alg {
            Algorithm::Sha1 => "SHA1",
            Algorithm::Sha256 => "SHA256",
            Algorithm::Sha512 => "SHA512",
        };
        let uri = format!("otpauth://totp/?secret={}&algorithm={alg}&digits={digits}&period={period}", secret.replace([' ', '='], ""));
        return Some((uri, vec![key, "TimeOtp-Algorithm", "TimeOtp-Length", "TimeOtp-Period"]));
    }
    if let Some(seed) = get("TOTP Seed") {
        // "30;6" (period;digits); "S" (Steam) digits are not supported -> default 6.
        let settings = get("TOTP Settings").unwrap_or_default();
        let mut it = settings.split(';');
        let period: u64 = it.next().and_then(|p| p.trim().parse().ok()).unwrap_or(30);
        let digits: u32 = it.next().and_then(|d| d.trim().parse().ok()).unwrap_or(6);
        let uri = format!("otpauth://totp/?secret={}&digits={digits}&period={period}", seed.replace([' ', '='], ""));
        return Some((uri, vec!["TOTP Seed", "TOTP Settings"]));
    }
    None
}

fn keepass_entry(e: &keepass::db::Entry, folder: &str) -> ImportedEntry {
    let get = |k: &str| e.fields.get(k).map(|v| v.get().clone()).filter(|v| !v.trim().is_empty());
    let mut p = EntryPatch {
        title: get("Title"),
        username: get("UserName"),
        password: get("Password").map(Zeroizing::new),
        notes: get("Notes").map(Zeroizing::new),
        folder: Some(folder.to_owned()),
        tags: Some(e.tags.clone()),
        ..Default::default()
    };
    let mut urls: Vec<String> = get("URL").into_iter().collect();
    let mut used: Vec<&str> = KP_STANDARD.to_vec();
    let totp = keepass_totp(&get);
    if let Some((uri, keys)) = &totp {
        used.extend(keys.iter().copied());
        put_totp(&mut p, uri);
    }
    let mut extra: Vec<(&String, &keepass::db::Value<String>)> = e.fields.iter().filter(|(k, _)| !used.contains(&k.as_str())).collect();
    extra.sort_by(|a, b| a.0.cmp(b.0));
    let mut fields = p.fields.take().unwrap_or_default();
    for (k, v) in extra {
        // KeePassXC / KeePass2Android extra URLs.
        if (k == "KP2A_URL" || k.starts_with("KP2A_URL_") || k.starts_with("URL_")) && !v.get().trim().is_empty() {
            urls.push(v.get().trim().to_owned());
            continue;
        }
        fields.push(FieldInput { name: k.clone(), value: z(v.get().clone()), hidden: v.is_protected() });
    }
    if !fields.is_empty() {
        p.fields = Some(fields);
    }
    p.urls = Some(urls);
    let current = get("Password").unwrap_or_default();
    let mut history = Vec::new();
    if let Some(h) = &e.history {
        for old in h.get_entries() {
            if let Some(pw) = old.fields.get("Password").map(|v| v.get().clone()).filter(|p| !p.is_empty() && *p != current) {
                let when = old.times.last_modification.map(|t| t.and_utc().timestamp_millis()).unwrap_or(0);
                if !history.iter().any(|(p, _): &(Zeroizing<String>, i64)| **p == pw) {
                    history.push((Zeroizing::new(pw), when));
                }
            }
        }
    }
    let mut out = ImportedEntry {
        entry: p,
        history,
        created: e.times.creation.map(|t| t.and_utc().timestamp_millis()),
        updated: e.times.last_modification.map(|t| t.and_utc().timestamp_millis()),
    };
    tidy_imported(&mut out);
    out.entry = finish(out.entry);
    out
}

/// Largest KeePass database / key file accepted.
pub const MAX_KDBX: usize = 64 << 20;
pub const MAX_KEYFILE: usize = 1 << 20;
const MAX_AES_ROUNDS: u64 = 100_000_000;
const MAX_ARGON2_MEMORY: u64 = 1 << 30;
const MAX_ARGON2_ITERATIONS: u64 = 100;
const MAX_ARGON2_PARALLELISM: u64 = 16;

/// Checks the outer header before `keepass` sees it: keepass 0.17 panics on short numeric
/// fields (byteorder) and takes the KDF cost from the file with no bound.
fn kdbx_guard(data: &[u8]) -> Result<(), ImportError> {
    let bad = |m: &str| ImportError::Format(format!("KeePass: {m}"));
    let u32_at = |p: usize| data.get(p..p + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    if data.len() > MAX_KDBX {
        return Err(bad("file too large"));
    }
    if u32_at(0) != Some(0x9AA2_D903) {
        return Err(bad("not a KDBX file"));
    }
    let major = data.get(10..12).map(|b| u16::from_le_bytes([b[0], b[1]]));
    match (u32_at(4), major) {
        (Some(0xB54B_FB67), Some(3 | 4)) => {}
        (Some(0xB54B_FB65), _) => return Err(bad("KeePass 1 (.kdb) files are not supported")),
        _ => return Err(bad("unsupported version")),
    }
    let v4 = major == Some(4);
    let mut pos = 12;
    loop {
        let id = *data.get(pos).ok_or_else(|| bad("truncated header"))?;
        let (len, hdr) = if v4 { (u32_at(pos + 1).map(|l| l as usize), 5) } else { (data.get(pos + 1..pos + 3).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize), 3) };
        let len = len.ok_or_else(|| bad("truncated header"))?;
        let buf = data.get(pos + hdr..(pos + hdr).saturating_add(len)).ok_or_else(|| bad("truncated header"))?;
        pos += hdr + len;
        let want = match (v4, id) {
            (_, 0) => break,
            (_, 3) | (false, 10) => Some(4),
            (false, 6) => Some(8),
            _ => None,
        };
        if want.is_some_and(|w| w != len) {
            return Err(bad("malformed header field"));
        }
        if !v4 && id == 6 && u64::from_le_bytes(buf.try_into().unwrap()) > MAX_AES_ROUNDS {
            return Err(bad("key derivation too expensive"));
        }
        if v4 && (id == 11 || id == 12) {
            variant_dict_guard(buf, id == 11).map_err(bad)?;
        }
    }
    Ok(())
}

/// Same walk as keepass's VariantDictionary::parse, with exact numeric lengths and KDF bounds.
fn variant_dict_guard(b: &[u8], kdf: bool) -> Result<(), &'static str> {
    let mut pos = 2;
    let len32 = |p: usize| b.get(p..p + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()) as usize).ok_or("truncated KDF parameters");
    while pos + 9 < b.len() {
        let ty = b[pos];
        let kl = len32(pos + 1)?;
        let key = b.get(pos + 5..(pos + 5).saturating_add(kl)).ok_or("truncated KDF parameters")?;
        pos = pos + 5 + kl;
        let vl = len32(pos)?;
        let val = b.get(pos + 4..(pos + 4).saturating_add(vl)).ok_or("truncated KDF parameters")?;
        pos += 4 + vl;
        let n = match (ty, vl) {
            (0x04 | 0x0c, 4) => u32::from_le_bytes(val.try_into().unwrap()) as u64,
            (0x05 | 0x0d, 8) => u64::from_le_bytes(val.try_into().unwrap()),
            (0x04 | 0x0c | 0x05 | 0x0d, _) => return Err("malformed KDF parameters"),
            _ => continue,
        };
        let max = match key {
            b"R" => MAX_AES_ROUNDS,
            b"M" => MAX_ARGON2_MEMORY,
            b"I" => MAX_ARGON2_ITERATIONS,
            b"P" => MAX_ARGON2_PARALLELISM,
            _ => u64::MAX,
        };
        if kdf && n > max {
            return Err("key derivation too expensive");
        }
    }
    Ok(())
}

/// Opens a KDBX 3.1 / 4 file with its master password and/or key file. Groups become folders
/// ("A/B", the root group itself is not part of the path); the recycle bin is skipped.
pub fn keepass(data: &[u8], password: Option<&str>, keyfile: Option<&[u8]>) -> ImportResult {
    use keepass::{Database, DatabaseKey, db::DatabaseOpenError};
    kdbx_guard(data)?;
    if keyfile.is_some_and(|k| k.len() > MAX_KEYFILE) {
        return Err(ImportError::Format("key file: too large".into()));
    }
    let mut key = DatabaseKey::new();
    if let Some(pw) = password {
        key = key.with_password(pw);
    }
    if let Some(mut kf) = keyfile {
        key = key.with_keyfile(&mut kf).map_err(|e| ImportError::Format(format!("key file: {e}")))?;
    }
    // A panic in the parser (inner header, payload) must not take the app down with it.
    let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Database::parse(data, key)))
        .map_err(|_| ImportError::Format("KeePass: malformed file".into()))?;
    let db = parsed.map_err(|e| match e {
        DatabaseOpenError::Key(keepass::error::DatabaseKeyError::IncorrectKey) => ImportError::WrongKey,
        DatabaseOpenError::Key(keepass::error::DatabaseKeyError::EmptyKey) => ImportError::WrongKey,
        // KDBX 3.1 has no key check: a wrong key shows up as a failed decryption.
        DatabaseOpenError::Cryptography(_) => ImportError::WrongKey,
        e => ImportError::Format(format!("KeePass: {e}")),
    })?;
    let bin = db.recycle_bin().map(|g| g.id());
    let mut out = Vec::new();
    fn walk(g: keepass::db::GroupRef<'_>, path: &str, bin: Option<keepass::db::GroupId>, out: &mut Vec<ImportedEntry>) {
        for e in g.entries() {
            out.push(keepass_entry(&e, path));
        }
        let mut subs: Vec<_> = g.groups().filter(|s| Some(s.id()) != bin).collect();
        subs.sort_by(|a, b| a.name.cmp(&b.name));
        for s in subs {
            let name = s.name.replace('/', "-");
            let p = if path.is_empty() { name } else { format!("{path}/{name}") };
            walk(s, &p, bin, out);
        }
    }
    walk(db.root(), "", bin, &mut out);
    Ok(out)
}

// ---------------- CSV ----------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Col {
    Title,
    Username,
    Password,
    Url,
    Notes,
    Totp,
    Folder,
    Tags,
    Favorite,
    /// Bitwarden CSV "fields": "name: value" lines
    Fields,
    /// e-mail: the user name when there is no user name column value, else a custom field
    Email,
    Created,
    Updated,
    Ignore,
    /// a custom field, hidden (card number, CVC, PIN)
    Hidden,
    Custom,
}

fn column(h: &str) -> Col {
    let k = tidy::norm(h);
    match k.as_str() {
        "title" | "name" | "account" | "accountname" | "itemname" => Col::Title,
        "username" | "login" | "loginusername" | "user" | "usernamefield" | "loginname" => Col::Username,
        "email" | "emailaddress" | "mail" => Col::Email,
        "password" | "loginpassword" | "pass" | "pwd" => Col::Password,
        "url" | "uri" | "loginuri" | "website" | "websiteurl" | "weburl" | "loginurl" | "urls" | "website1" => Col::Url,
        "notes" | "note" | "extra" | "comments" | "comment" | "notesplain" => Col::Notes,
        "totp" | "otp" | "logintotp" | "otpauth" | "onetimepassword" | "otpurl" | "2fa" => Col::Totp,
        "folder" | "group" | "grouping" | "category" | "path" => Col::Folder,
        "tags" | "tag" | "labels" => Col::Tags,
        "favorite" | "favourite" | "fav" | "starred" => Col::Favorite,
        "fields" => Col::Fields,
        "httprealm" | "formactionorigin" | "guid" | "timelastused" | "lastused" | "type" | "reprompt" | "archived" | "id"
        | "vault" | "rffieldsv2" | "usagecount" => Col::Ignore,
        _ => match tidy::classify(h) {
            Some(tidy::Known::Urls) => Col::Url,
            Some(tidy::Known::Created) => Col::Created,
            Some(tidy::Known::Updated) => Col::Updated,
            Some(tidy::Known::Totp) => Col::Totp,
            Some(tidy::Known::Hide) => Col::Hidden,
            Some(tidy::Known::Noise) => Col::Ignore,
            None => Col::Custom,
        },
    }
}

pub fn csv(data: &[u8]) -> ImportResult {
    let text = String::from_utf8_lossy(data);
    let text = text.trim_start_matches('\u{feff}');
    // Semicolon-separated files (some European exports): pick the delimiter from the header line.
    let first = text.lines().next().unwrap_or("");
    let delim = if first.matches(';').count() > first.matches(',').count() { b';' } else { b',' };
    let mut rd = csv::ReaderBuilder::new().delimiter(delim).flexible(true).from_reader(text.as_bytes());
    let headers = rd.headers().map_err(|e| ImportError::Format(format!("CSV: {e}")))?.clone();
    let cols: Vec<Col> = headers.iter().map(column).collect();
    if !cols.contains(&Col::Password) && !cols.contains(&Col::Username) && !cols.contains(&Col::Email) {
        return Err(ImportError::Format("CSV: no username or password column".into()));
    }
    let mut out = Vec::new();
    for rec in rd.records() {
        let rec = rec.map_err(|e| ImportError::Format(format!("CSV: {e}")))?;
        let mut p = EntryPatch { urls: Some(vec![]), ..Default::default() };
        let mut fields = Vec::new();
        let mut created = None;
        let mut updated = None;
        let mut email: Option<(usize, String)> = None;
        for (i, v) in rec.iter().enumerate() {
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            match cols.get(i).copied().unwrap_or(Col::Ignore) {
                Col::Title => p.title = Some(v.into()),
                Col::Username => p.username = Some(v.into()),
                Col::Password => p.password = z(v),
                // LastPass marks secure notes with the pseudo URL "http://sn"
                Col::Url if v.eq_ignore_ascii_case("http://sn") => {}
                Col::Email => email = Some((i, v.to_owned())),
                Col::Url => {
                    let found = tidy::parse_urls(v).unwrap_or_else(|| v.split([',', '\n']).map(|u| u.trim().to_owned()).filter(|u| !u.is_empty()).collect());
                    let urls = p.urls.get_or_insert_with(Vec::new);
                    urls.extend(found.into_iter().filter(|u| !urls.contains(u)).collect::<Vec<_>>());
                }
                Col::Notes => p.notes = z(v),
                Col::Totp => {
                    p.fields = Some(std::mem::take(&mut fields));
                    put_totp(&mut p, v);
                    fields = p.fields.take().unwrap_or_default();
                }
                Col::Folder => {
                    let f = v.replace('\\', "/");
                    // KeePassXC writes the root group's name first ("Root/Internet")
                    let f = if headers.get(i).is_some_and(|h| h.trim().eq_ignore_ascii_case("group")) {
                        f.strip_prefix("Root/").map(str::to_owned).unwrap_or_else(|| if f == "Root" { String::new() } else { f.clone() })
                    } else {
                        f
                    };
                    p.folder = Some(f);
                }
                Col::Tags => p.tags = Some(v.split([',', ';']).map(|t| t.trim().to_owned()).collect()),
                Col::Favorite => p.favorite = Some(matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "sì" | "si" | "x")),
                Col::Fields => {
                    for line in v.lines() {
                        if let Some((k, val)) = line.split_once(": ").or_else(|| line.split_once(':')) {
                            fields.push(FieldInput { name: k.trim().into(), value: z(val.trim()), hidden: false });
                        }
                    }
                }
                Col::Created => created = tidy::parse_time(v).or(created),
                Col::Updated => updated = tidy::parse_time(v).max(updated),
                Col::Custom | Col::Hidden => {
                    let name = headers.get(i).unwrap_or("Campo").trim().trim_start_matches('\u{feff}').to_owned();
                    fields.push(FieldInput { name, value: z(v), hidden: cols[i] == Col::Hidden });
                }
                Col::Ignore => {}
            }
        }
        if let Some((i, mail)) = email {
            if p.username.as_deref().is_none_or(|u| u.trim().is_empty()) {
                p.username = Some(mail);
            } else if p.username.as_deref() != Some(mail.as_str()) {
                fields.push(FieldInput { name: headers.get(i).unwrap_or("E-mail").trim().to_owned(), value: z(mail), hidden: false });
            }
        }
        if p.title.is_none() && p.username.is_none() && p.password.is_none() && p.urls.as_ref().is_none_or(Vec::is_empty) {
            continue; // blank line
        }
        if !fields.is_empty() {
            p.fields = Some(fields);
        }
        let mut e = ImportedEntry { entry: p, history: vec![], created, updated };
        tidy_imported(&mut e);
        e.entry = finish(e.entry);
        out.push(e);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339() {
        assert_eq!(ms_of_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(ms_of_rfc3339("2024-02-29T12:00:00.5Z"), Some(1_709_208_000_500));
        assert_eq!(ms_of_rfc3339("nope"), None);
    }

    #[test]
    fn titles_fall_back_to_the_host() {
        assert_eq!(fallback_title(&["https://www.example.com:8443/login?x".into()], "u"), "example.com");
        assert_eq!(fallback_title(&[], "  mario "), "mario");
        assert_eq!(fallback_title(&[], ""), "Senza titolo");
    }

    /// A KDBX 3.1 header whose compression field is 0 bytes made keepass 0.17 panic
    /// (byteorder read_u32 on an empty slice), which is an abort in release.
    #[test]
    fn crafted_kdbx_headers_are_refused_not_panics() {
        let sig = |minor_major: [u8; 4]| [&[0x03, 0xd9, 0xa2, 0x9a, 0x67, 0xfb, 0x4b, 0xb5][..], &minor_major].concat();
        let v3 = |fields: &[(u8, &[u8])]| {
            let mut d = sig([1, 0, 3, 0]);
            for (id, b) in fields {
                d.push(*id);
                d.extend((b.len() as u16).to_le_bytes());
                d.extend(*b);
            }
            d
        };
        let v4_kdf = |vd: &[u8]| {
            let mut d = sig([1, 0, 4, 0]);
            d.push(11);
            d.extend((vd.len() as u32).to_le_bytes());
            d.extend(vd);
            d
        };
        let entry = |ty: u8, k: &[u8], v: &[u8]| [&[ty][..], &(k.len() as u32).to_le_bytes(), k, &(v.len() as u32).to_le_bytes(), v].concat();
        let vd = |es: &[Vec<u8>]| [&[0u8, 1][..], &es.concat(), &[0]].concat();
        let cases: Vec<Vec<u8>> = vec![
            v3(&[(3, &[]), (0, &[])]),
            v3(&[(6, &[1, 2]), (0, &[])]),
            v3(&[(10, &[1]), (0, &[])]),
            v3(&[(6, &u64::MAX.to_le_bytes()), (0, &[])]),
            v3(&[(2, &[0; 16])]), // no end
            v4_kdf(&vd(&[entry(0x05, b"I", &[1, 2, 3])])),
            v4_kdf(&vd(&[entry(0x04, b"P", &[1])])),
            v4_kdf(&vd(&[entry(0x05, b"M", &(4u64 << 30).to_le_bytes())])),
            v4_kdf(&vd(&[entry(0x05, b"I", &1_000_000u64.to_le_bytes())])),
            v4_kdf(&vd(&[entry(0x05, b"R", &u64::MAX.to_le_bytes())])),
            [&[0x03, 0xd9, 0xa2, 0x9a, 0x65, 0xfb, 0x4b, 0xb5][..], &[0; 120]].concat(), // KeePass 1
            vec![0x03, 0xd9, 0xa2],
        ];
        for (i, c) in cases.iter().enumerate() {
            assert!(matches!(keepass(c, Some("x"), None), Err(ImportError::Format(_))), "case {i}");
        }
        assert!(matches!(keepass(&vec![0u8; MAX_KDBX + 1], None, None), Err(ImportError::Format(_))));
    }

    #[test]
    fn csv_column_names() {
        assert_eq!(column("login_uri"), Col::Url);
        assert_eq!(column("Login Username"), Col::Username);
        assert_eq!(column("\u{feff}name"), Col::Title);
        assert_eq!(column("OTPAuth"), Col::Totp);
        assert_eq!(column("httpRealm"), Col::Ignore);
        assert_eq!(column("AUTOFILLURLS"), Col::Url);
        assert_eq!(column("createTime"), Col::Created);
        assert_eq!(column("modifyTime"), Col::Updated);
        assert_eq!(column("timePasswordChanged"), Col::Updated);
        assert_eq!(column("otpSecret"), Col::Totp);
        assert_eq!(column("cvv"), Col::Hidden);
        assert_eq!(column("Icon"), Col::Ignore);
        assert_eq!(column("Security question"), Col::Custom);
    }
}
