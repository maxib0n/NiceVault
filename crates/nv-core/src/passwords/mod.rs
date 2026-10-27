pub mod generate;
pub mod import;
pub mod matching;
pub mod tidy;
pub mod totp;

use crate::{Vault, dberr, keys::random};
use nv_fs::Error;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use totp::{Totp, TotpCode, TotpError, TotpInfo};
use zeroize::Zeroizing;

pub const SCHEMA_KEY: &str = "nv.pm_schema";
pub const HISTORY_KEEP: usize = 10;
pub const MAX_TEXT: usize = 64 * 1024;
pub const MAX_NOTES: usize = 1 << 20;
pub const MAX_FIELDS: usize = 100;
pub const MAX_URLS: usize = 50;
pub const MAX_TAGS: usize = 50;

const MIGRATIONS: &[&str] = &["
CREATE TABLE IF NOT EXISTS pm_entries (
  id       TEXT PRIMARY KEY,
  title    TEXT NOT NULL,
  username TEXT NOT NULL DEFAULT '',
  urls     TEXT NOT NULL DEFAULT '[]',   -- JSON array of strings
  folder   TEXT NOT NULL DEFAULT '',     -- 'A/B' path, '' = none
  tags     TEXT NOT NULL DEFAULT '[]',   -- JSON array of strings
  favorite INTEGER NOT NULL DEFAULT 0,
  password TEXT NOT NULL DEFAULT '',
  notes    TEXT NOT NULL DEFAULT '',
  totp     TEXT,                         -- canonical otpauth://totp URI
  fields   TEXT NOT NULL DEFAULT '[]',   -- JSON [{name, value, hidden}]
  created  INTEGER NOT NULL,             -- unix ms
  updated  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS pm_history (
  entry    TEXT NOT NULL REFERENCES pm_entries(id) ON DELETE CASCADE,
  password TEXT NOT NULL,
  changed  INTEGER NOT NULL              -- unix ms when it stopped being the password
);
CREATE INDEX IF NOT EXISTS pm_history_entry ON pm_history(entry, changed);
"];

pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

pub const REPAIR_KEY: &str = "nv.pm_repair";

const ICONS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS pm_icons (
  host    TEXT PRIMARY KEY,   -- lowercase DNS name, see icon_host
  png     BLOB,               -- 64x64 PNG; NULL = the site had none usable
  fetched INTEGER NOT NULL    -- unix ms of the last attempt
);";

pub const MAX_ICON: usize = 64 * 1024;

pub(crate) fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    use rusqlite::types::Value;
    let raw: Option<Value> = conn.query_row("SELECT v FROM meta WHERE k = ?", [SCHEMA_KEY], |r| r.get(0)).optional()?;
    let parse = |b: &[u8]| std::str::from_utf8(b).ok().and_then(|s| s.trim().parse::<u32>().ok());
    let have: u32 = match raw {
        None => 0,
        Some(v) => match match &v {
            Value::Blob(b) => parse(b),
            Value::Text(s) => parse(s.as_bytes()),
            Value::Integer(n) => u32::try_from(*n).ok(),
            _ => None,
        } {
            Some(n) => n,
            None => return Ok(()),
        },
    };
    if have >= SCHEMA_VERSION {
        return Ok(());
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let r = (|| {
        for m in &MIGRATIONS[have as usize..] {
            conn.execute_batch(m)?;
        }
        conn.execute("INSERT OR REPLACE INTO meta(k, v) VALUES (?, ?)", params![SCHEMA_KEY, SCHEMA_VERSION.to_string().into_bytes()])?;
        Ok(())
    })();
    match r {
        Ok(()) => conn.execute_batch("COMMIT"),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PmError {
    #[error("no such entry")]
    NotFound,
    #[error("invalid entry: {0}")]
    Invalid(String),
    #[error("invalid TOTP: {0}")]
    Totp(#[from] TotpError),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error(transparent)]
    Fs(#[from] Error),
}

impl From<rusqlite::Error> for PmError {
    fn from(e: rusqlite::Error) -> Self {
        match dberr(e) {
            Error::NotFound => PmError::NotFound,
            e => PmError::Fs(e),
        }
    }
}

pub type PmResult<T> = std::result::Result<T, PmError>;

#[derive(Clone, Serialize, Deserialize)]
struct StoredField {
    name: String,
    value: String,
    hidden: bool,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

struct JsonCol<T> {
    items: Vec<T>,
    other: Vec<serde_json::Value>,
    unreadable: bool,
}

impl<T: serde::de::DeserializeOwned + Serialize> JsonCol<T> {
    fn parse(raw: &str) -> Self {
        match serde_json::from_str::<Vec<serde_json::Value>>(raw) {
            Ok(vals) => {
                let (mut items, mut other) = (Vec::new(), Vec::new());
                for v in vals {
                    match T::deserialize(&v) {
                        Ok(t) => items.push(t),
                        Err(_) => other.push(v),
                    }
                }
                JsonCol { items, other, unreadable: false }
            }
            Err(_) => JsonCol { items: Vec::new(), other: Vec::new(), unreadable: true },
        }
    }

    fn empty() -> Self {
        JsonCol { items: Vec::new(), other: Vec::new(), unreadable: false }
    }

    fn set(&mut self, items: Vec<T>, what: &str) -> PmResult<()> {
        if self.unreadable {
            return Err(PmError::Unsupported(format!("{what} were saved by a newer NiceVault")));
        }
        self.items = items;
        Ok(())
    }

    fn to_json(&self) -> String {
        let mut all: Vec<serde_json::Value> = self.items.iter().filter_map(|t| serde_json::to_value(t).ok()).collect();
        all.extend(self.other.iter().cloned());
        serde_json::to_string(&all).unwrap_or_else(|_| "[]".into())
    }
}

impl Drop for StoredField {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.value);
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct FieldView {
    pub name: String,
    pub hidden: bool,
    pub value: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct FieldInput {
    pub name: String,
    #[serde(default)]
    pub value: Option<Zeroizing<String>>,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct EntrySummary {
    pub id: String,
    pub title: String,
    pub username: String,
    pub urls: Vec<String>,
    pub folder: String,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub has_password: bool,
    pub has_totp: bool,
    pub created: i64,
    pub updated: i64,
    pub icon: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct EntryDetail {
    #[serde(flatten)]
    pub summary: EntrySummary,
    pub notes: String,
    pub fields: Vec<FieldView>,
    pub totp: Option<TotpInfo>,
    pub history_count: usize,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct EntryPatch {
    pub title: Option<String>,
    pub username: Option<String>,
    pub password: Option<Zeroizing<String>>,
    pub urls: Option<Vec<String>>,
    pub notes: Option<Zeroizing<String>>,
    pub totp: Option<Zeroizing<String>>,
    pub folder: Option<String>,
    pub tags: Option<Vec<String>>,
    pub favorite: Option<bool>,
    pub fields: Option<Vec<FieldInput>>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct HistoryItem {
    pub index: usize,
    pub changed: i64,
}

#[derive(Debug, Default)]
pub struct ImportedEntry {
    pub entry: EntryPatch,
    pub history: Vec<(Zeroizing<String>, i64)>,
    pub created: Option<i64>,
    pub updated: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Secret {
    Username,
    Password,
    Notes,
    Totp,
    TotpCode,
    Field(usize),
    History(usize),
}

impl Secret {
    pub fn parse(s: &str) -> Option<Secret> {
        Some(match s {
            "username" => Secret::Username,
            "password" => Secret::Password,
            "notes" => Secret::Notes,
            "totp" => Secret::Totp,
            "totp_code" => Secret::TotpCode,
            _ => match s.split_once(':')? {
                ("field", i) => Secret::Field(i.parse().ok()?),
                ("history", i) => Secret::History(i.parse().ok()?),
                _ => return None,
            },
        })
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn new_id() -> String {
    crate::pq::hex(&random::<16>())
}

fn invalid(m: impl Into<String>) -> PmError {
    PmError::Invalid(m.into())
}

fn clean_list(v: Vec<String>, max: usize, what: &str) -> PmResult<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for s in v.into_iter().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()) {
        if s.len() > MAX_TEXT {
            return Err(invalid(format!("{what} too long")));
        }
        if !out.contains(&s) {
            out.push(s);
        }
    }
    if out.len() > max {
        return Err(invalid(format!("too many {what}")));
    }
    Ok(out)
}

fn clean_folder(f: &str) -> String {
    f.split(['/', '\\']).map(str::trim).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("/")
}

fn check_len(s: &str, max: usize, what: &str) -> PmResult<()> {
    if s.len() > max { Err(invalid(format!("{what} too long"))) } else { Ok(()) }
}

pub fn normalize_totp(s: &str) -> PmResult<Option<Zeroizing<String>>> {
    if s.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(Totp::parse(s)?.to_uri()))
}

struct Row_ {
    summary: EntrySummary,
    password: Zeroizing<String>,
    notes: Zeroizing<String>,
    totp: Option<Zeroizing<String>>,
    urls: JsonCol<String>,
    tags: JsonCol<String>,
    fields: JsonCol<StoredField>,
    changed: Vec<&'static str>,
}

const SUMMARY_COLS: &str = "id, title, username, urls, folder, tags, favorite, password != '', totp IS NOT NULL, created, updated";

fn row_summary(r: &Row<'_>) -> rusqlite::Result<EntrySummary> {
    Ok(EntrySummary {
        id: r.get(0)?,
        title: r.get(1)?,
        username: r.get(2)?,
        urls: JsonCol::<String>::parse(&r.get::<_, String>(3)?).items,
        folder: r.get(4)?,
        tags: JsonCol::<String>::parse(&r.get::<_, String>(5)?).items,
        favorite: r.get::<_, i64>(6)? != 0,
        has_password: r.get(7)?,
        has_totp: r.get(8)?,
        created: r.get(9)?,
        updated: r.get(10)?,
        icon: None,
    })
}

fn load_row(conn: &Connection, id: &str) -> PmResult<Row_> {
    let sql = format!("SELECT {SUMMARY_COLS}, password, notes, totp, fields FROM pm_entries WHERE id = ?");
    conn.query_row(&sql, [id], |r| {
        let fields: Zeroizing<String> = Zeroizing::new(r.get(14)?);
        Ok(Row_ {
            summary: row_summary(r)?,
            password: Zeroizing::new(r.get(11)?),
            notes: Zeroizing::new(r.get(12)?),
            totp: r.get::<_, Option<String>>(13)?.map(Zeroizing::new),
            urls: JsonCol::parse(&r.get::<_, String>(3)?),
            tags: JsonCol::parse(&r.get::<_, String>(5)?),
            fields: JsonCol::parse(&fields),
            changed: Vec::new(),
        })
    })
    .map_err(PmError::from)
}

fn sort_summaries(v: &mut [EntrySummary]) {
    v.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
}

fn apply(row: &mut Row_, p: EntryPatch) -> PmResult<Option<Zeroizing<String>>> {
    let s = &mut row.summary;
    let ch = &mut row.changed;
    if let Some(t) = p.title {
        s.title = t.trim().to_owned();
        ch.push("title");
    }
    if s.title.is_empty() {
        return Err(invalid("title is required"));
    }
    check_len(&s.title, MAX_TEXT, "title")?;
    if let Some(u) = p.username {
        check_len(&u, MAX_TEXT, "username")?;
        s.username = u;
        ch.push("username");
    }
    if let Some(u) = p.urls {
        row.urls.set(clean_list(u, MAX_URLS, "urls")?, "urls")?;
        s.urls = row.urls.items.clone();
        ch.push("urls");
    }
    if let Some(t) = p.tags {
        row.tags.set(clean_list(t, MAX_TAGS, "tags")?, "tags")?;
        s.tags = row.tags.items.clone();
        ch.push("tags");
    }
    if let Some(f) = p.folder {
        s.folder = clean_folder(&f);
        check_len(&s.folder, MAX_TEXT, "folder")?;
        ch.push("folder");
    }
    if let Some(f) = p.favorite {
        s.favorite = f;
        ch.push("favorite");
    }
    if let Some(n) = p.notes {
        check_len(&n, MAX_NOTES, "notes")?;
        row.notes = n;
        ch.push("notes");
    }
    if let Some(t) = p.totp {
        row.totp = normalize_totp(&t)?;
        ch.push("totp");
    }
    if let Some(fields) = p.fields {
        if fields.len() > MAX_FIELDS {
            return Err(invalid("too many custom fields"));
        }
        let mut out = Vec::with_capacity(fields.len());
        for f in fields {
            let name = f.name.trim().to_owned();
            if name.is_empty() {
                return Err(invalid("custom field without a name"));
            }
            check_len(&name, MAX_TEXT, "field name")?;
            let prev = row.fields.items.iter().find(|o| o.name == name);
            let value = match f.value {
                Some(v) => v.to_string(),
                None => prev.map(|o| o.value.clone()).unwrap_or_default(),
            };
            check_len(&value, MAX_TEXT, "field value")?;
            let extra = prev.map(|o| o.extra.clone()).unwrap_or_default();
            out.push(StoredField { name, value, hidden: f.hidden, extra });
        }
        row.fields.set(out, "custom fields")?;
        ch.push("fields");
    }
    let mut old = None;
    if let Some(pw) = p.password {
        check_len(&pw, MAX_TEXT, "password")?;
        if *pw != *row.password {
            old = Some(std::mem::replace(&mut row.password, pw)).filter(|o| !o.is_empty());
            ch.push("password");
        }
    }
    s.has_password = !row.password.is_empty();
    s.has_totp = row.totp.is_some();
    Ok(old)
}

fn empty_row(id: String, now: i64) -> Row_ {
    Row_ {
        summary: EntrySummary {
            id,
            title: String::new(),
            username: String::new(),
            urls: vec![],
            folder: String::new(),
            tags: vec![],
            favorite: false,
            has_password: false,
            has_totp: false,
            created: now,
            updated: now,
            icon: None,
        },
        password: Zeroizing::new(String::new()),
        notes: Zeroizing::new(String::new()),
        totp: None,
        urls: JsonCol::empty(),
        tags: JsonCol::empty(),
        fields: JsonCol::empty(),
        changed: Vec::new(),
    }
}

const WRITE_COLS: [&str; 13] =
    ["id", "title", "username", "urls", "folder", "tags", "favorite", "password", "notes", "totp", "fields", "created", "updated"];

fn write_row(conn: &Connection, row: &Row_, insert: bool) -> rusqlite::Result<()> {
    let s = &row.summary;
    let urls = row.urls.to_json();
    let tags = row.tags.to_json();
    let fields = Zeroizing::new(row.fields.to_json());
    let sql = if insert {
        "INSERT INTO pm_entries(id, title, username, urls, folder, tags, favorite, password, notes, totp, fields, created, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
            .to_owned()
    } else {
        let set: Vec<String> = WRITE_COLS
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == "updated" || row.changed.contains(c))
            .map(|(i, c)| format!("{c} = ?{}", i + 1))
            .collect();
        format!("UPDATE pm_entries SET {} WHERE id = ?1", set.join(", "))
    };
    conn.execute(
        &sql,
        params![
            s.id,
            s.title,
            s.username,
            urls,
            s.folder,
            tags,
            s.favorite as i64,
            row.password.as_str(),
            row.notes.as_str(),
            row.totp.as_ref().map(|t| t.as_str()),
            fields.as_str(),
            s.created,
            s.updated
        ],
    )?;
    Ok(())
}

fn push_history(conn: &Connection, id: &str, old: &str, when: i64) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO pm_history(entry, password, changed) VALUES (?, ?, ?)", params![id, old, when])?;
    conn.execute(
        "DELETE FROM pm_history WHERE entry = ?1 AND rowid NOT IN
           (SELECT rowid FROM pm_history WHERE entry = ?1 ORDER BY changed DESC, rowid DESC LIMIT ?2)",
        params![id, HISTORY_KEEP as i64],
    )?;
    Ok(())
}

fn detail(conn: &Connection, row: &Row_) -> PmResult<EntryDetail> {
    let history_count: i64 = conn.query_row("SELECT count(*) FROM pm_history WHERE entry = ?", [&row.summary.id], |r| r.get(0))?;
    let mut summary = row.summary.clone();
    summary.icon = icon_ref(&icon_map(conn), &summary.urls);
    Ok(EntryDetail {
        summary,
        notes: row.notes.to_string(),
        fields: row.fields.items.iter().map(|f| FieldView { name: f.name.clone(), hidden: f.hidden, value: (!f.hidden).then(|| f.value.clone()) }).collect(),
        totp: row.totp.as_ref().and_then(|t| Totp::parse(t).ok()).map(|t| t.info()),
        history_count: history_count as usize,
    })
}

pub fn matches(s: &EntrySummary, query: &str) -> bool {
    let hay = format!("{}\n{}\n{}\n{}\n{}", s.title, s.username, s.urls.join("\n"), s.tags.join("\n"), s.folder).to_lowercase();
    query.split_whitespace().all(|t| hay.contains(&t.to_lowercase()))
}

pub fn icon_host(urls: &[String]) -> Option<String> {
    urls.iter().find_map(|u| host_of(u))
}

fn host_of(url: &str) -> Option<String> {
    let u = url.trim();
    let rest = match u.split_once("://") {
        Some((scheme, r)) => {
            if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
                return None;
            }
            r
        }
        None => u,
    };
    let auth = rest.split(['/', '?', '#']).next()?;
    let host = auth.rsplit('@').next()?;
    if host.starts_with('[') {
        return None;
    }
    let host = host.split(':').next()?.trim_end_matches('.').to_ascii_lowercase();
    let label_ok = |l: &str| !l.is_empty() && l.len() <= 63 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if host.len() > 253 || !host.contains('.') || !host.split('.').all(label_ok) {
        return None;
    }
    if host.parse::<std::net::Ipv4Addr>().is_ok() || host.ends_with(".localhost") || host.ends_with(".local") || host.ends_with(".internal") {
        return None;
    }
    if host.rsplit('.').next().is_some_and(|tld| tld.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    Some(host)
}

fn icon_map(conn: &Connection) -> std::collections::HashMap<String, i64> {
    let q = || -> rusqlite::Result<std::collections::HashMap<String, i64>> {
        let mut st = conn.prepare("SELECT host, fetched FROM pm_icons WHERE png IS NOT NULL")?;
        st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.collect()
    };
    q().unwrap_or_default()
}

fn icon_ref(map: &std::collections::HashMap<String, i64>, urls: &[String]) -> Option<String> {
    if map.is_empty() {
        return None;
    }
    let h = icon_host(urls)?;
    map.get(&h).map(|f| format!("{h}/{f}"))
}

fn in_folder(conn: &Connection, folder: &str) -> rusqlite::Result<Vec<(String, String)>> {
    let mut st = conn.prepare("SELECT id, folder FROM pm_entries WHERE folder = ?1 OR substr(folder, 1, length(?1) + 1) = ?1 || '/'")?;
    st.query_map([folder], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
}

fn set_folder(conn: &Connection, id: &str, folder: &str) -> rusqlite::Result<usize> {
    conn.execute("UPDATE pm_entries SET folder = ?1 WHERE id = ?2 AND folder != ?1", params![folder, id])
}

pub(crate) fn repair_imported(conn: &Connection) -> rusqlite::Result<usize> {
    let ids: Vec<String> = {
        let mut st = conn.prepare("SELECT id FROM pm_entries WHERE fields != '[]'")?;
        st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    let mut changed = 0;
    for id in ids {
        let Ok(mut row) = load_row(conn, &id) else { continue };
        if row.fields.unreadable || row.fields.items.is_empty() {
            continue;
        }
        let mut found = tidy::Found::default();
        let has_totp = row.totp.is_some();
        let mut kept = Vec::new();
        let mut dropped = false;
        for f in std::mem::take(&mut row.fields.items) {
            let act = if f.extra.is_empty() { tidy::field(&f.name, &f.value, has_totp, &mut found) } else { tidy::Act::Keep };
            match (act, tidy::classify(&f.name)) {
                (tidy::Act::Drop, Some(tidy::Known::Urls)) if row.urls.unreadable => kept.push(f),
                (tidy::Act::Drop, Some(tidy::Known::Urls | tidy::Known::Created | tidy::Known::Updated | tidy::Known::Totp)) => dropped = true,
                _ => kept.push(f),
            }
        }
        if !dropped {
            continue;
        }
        row.fields.items = kept;
        row.changed.push("fields");
        if !found.urls.is_empty() && !row.urls.unreadable {
            let mut urls = row.urls.items.clone();
            for u in found.urls {
                if !urls.contains(&u) && urls.len() < MAX_URLS {
                    urls.push(u);
                }
            }
            row.summary.urls = urls.clone();
            row.urls.items = urls;
            row.changed.push("urls");
        }
        if let Some(c) = found.created {
            row.summary.created = c;
            row.changed.push("created");
        }
        if let Some(u) = found.updated {
            row.summary.updated = u.max(row.summary.created);
        } else if found.created.is_some() {
            row.summary.updated = row.summary.updated.max(row.summary.created);
        }
        if let Some(t) = found.totp.as_deref().and_then(|t| normalize_totp(t).ok().flatten()) {
            row.totp = Some(t);
            row.changed.push("totp");
        }
        write_row(conn, &row, false)?;
        changed += 1;
    }
    Ok(changed)
}

pub(crate) fn repair_once(conn: &Connection) {
    let get = |k: &str| conn.query_row("SELECT v FROM meta WHERE k = ?", [k], |r| r.get::<_, Vec<u8>>(0)).optional().ok().flatten();
    if get(SCHEMA_KEY).as_deref() != Some(SCHEMA_VERSION.to_string().as_bytes()) || get(REPAIR_KEY).is_some() {
        return;
    }
    let r = (|| -> rusqlite::Result<()> {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        repair_imported(conn)?;
        conn.execute("INSERT OR REPLACE INTO meta(k, v) VALUES (?, ?)", params![REPAIR_KEY, b"1".to_vec()])?;
        conn.execute_batch("COMMIT")
    })();
    if r.is_err() {
        let _ = conn.execute_batch("ROLLBACK");
    }
}

impl Vault {
    fn pm<T>(&self, f: impl FnOnce(&mut Connection) -> PmResult<T>) -> PmResult<T> {
        let mut out = None;
        self.with(|i| {
            out = Some(f(&mut i.conn));
            Ok(())
        })?;
        out.expect("closure ran")
    }

    fn pm_write<T>(&self, f: impl FnOnce(&Connection) -> PmResult<T>) -> PmResult<T> {
        let mut pm_err = None;
        let r = self.with(|i| {
            let v = i.op(|c| {
                f(c).map_err(|e| {
                    pm_err = Some(e);
                    nv_fs::Error::Io("password manager step failed".into())
                })
            })?;
            i.durable()?;
            Ok(v)
        });
        match (r, pm_err) {
            (_, Some(e)) => Err(e),
            (Ok(v), None) => Ok(v),
            (Err(e), None) => Err(e.into()),
        }
    }

    pub fn pm_list(&self) -> PmResult<Vec<EntrySummary>> {
        self.pm(|c| {
            let mut st = c.prepare(&format!("SELECT {SUMMARY_COLS} FROM pm_entries"))?;
            let mut v = st.query_map([], row_summary)?.collect::<rusqlite::Result<Vec<_>>>()?;
            let icons = icon_map(c);
            for s in &mut v {
                s.icon = icon_ref(&icons, &s.urls);
            }
            sort_summaries(&mut v);
            Ok(v)
        })
    }

    pub fn pm_search(&self, query: &str) -> PmResult<Vec<EntrySummary>> {
        Ok(self.pm_list()?.into_iter().filter(|s| matches(s, query)).collect())
    }

    pub fn pm_get(&self, id: &str) -> PmResult<EntryDetail> {
        self.pm(|c| detail(c, &load_row(c, id)?))
    }

    pub fn pm_create(&self, p: EntryPatch) -> PmResult<EntryDetail> {
        let now = now_ms();
        let mut row = empty_row(new_id(), now);
        apply(&mut row, p)?;
        self.pm_write(|tx| {
            write_row(tx, &row, true)?;
            detail(tx, &row)
        })
    }

    pub fn pm_update(&self, id: &str, p: EntryPatch) -> PmResult<EntryDetail> {
        self.pm_write(|tx| {
            let mut row = load_row(tx, id)?;
            let now = now_ms().max(row.summary.updated);
            let old = apply(&mut row, p)?;
            row.summary.updated = now;
            write_row(tx, &row, false)?;
            if let Some(old) = old {
                push_history(tx, id, &old, now)?;
            }
            detail(tx, &row)
        })
    }

    pub fn pm_delete(&self, id: &str) -> PmResult<()> {
        self.pm_write(|tx| {
            tx.execute("DELETE FROM pm_history WHERE entry = ?", [id])?;
            if tx.execute("DELETE FROM pm_entries WHERE id = ?", [id])? == 0 {
                return Err(PmError::NotFound);
            }
            Ok(())
        })
    }

    pub fn pm_history(&self, id: &str) -> PmResult<Vec<HistoryItem>> {
        self.pm(|c| {
            load_row(c, id)?;
            let mut st = c.prepare("SELECT changed FROM pm_history WHERE entry = ? ORDER BY changed DESC, rowid DESC")?;
            let v = st.query_map([id], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(v.into_iter().enumerate().map(|(index, changed)| HistoryItem { index, changed }).collect())
        })
    }

    pub fn pm_reveal(&self, id: &str, what: Secret) -> PmResult<Zeroizing<String>> {
        self.pm(|c| {
            let mut row = load_row(c, id)?;
            let v = match what {
                Secret::Username => Zeroizing::new(row.summary.username.clone()),
                Secret::Password => std::mem::take(&mut row.password),
                Secret::Notes => std::mem::take(&mut row.notes),
                Secret::Totp => row.totp.take().ok_or(PmError::NotFound)?,
                Secret::TotpCode => {
                    let t = Totp::parse(row.totp.as_deref().ok_or(PmError::NotFound)?)?;
                    Zeroizing::new(t.now().code)
                }
                Secret::Field(i) => Zeroizing::new(row.fields.items.get(i).ok_or(PmError::NotFound)?.value.clone()),
                Secret::History(i) => Zeroizing::new(
                    c.query_row(
                        "SELECT password FROM pm_history WHERE entry = ? ORDER BY changed DESC, rowid DESC LIMIT 1 OFFSET ?",
                        params![id, i as i64],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?
                    .ok_or(PmError::NotFound)?,
                ),
            };
            if v.is_empty() && what != Secret::Username && what != Secret::Notes {
                return Err(PmError::NotFound);
            }
            Ok(v)
        })
    }

    pub fn pm_totp(&self, id: &str) -> PmResult<TotpCode> {
        let uri = self.pm_reveal(id, Secret::Totp)?;
        Ok(Totp::parse(&uri)?.now())
    }

    pub fn pm_move(&self, ids: &[String], folder: &str) -> PmResult<usize> {
        let folder = clean_folder(folder);
        check_len(&folder, MAX_TEXT, "folder")?;
        self.pm_write(|tx| {
            let mut n = 0;
            for id in ids {
                n += set_folder(tx, id, &folder)?;
            }
            Ok(n)
        })
    }

    pub fn pm_folder_rename(&self, from: &str, to: &str) -> PmResult<usize> {
        let (from, to) = (clean_folder(from), clean_folder(to));
        if from.is_empty() || to.is_empty() {
            return Err(invalid("folder name is required"));
        }
        check_len(&to, MAX_TEXT, "folder")?;
        if to == from {
            return Ok(0);
        }
        self.pm_write(|tx| {
            let mut n = 0;
            for (id, f) in in_folder(tx, &from)? {
                n += set_folder(tx, &id, &format!("{to}{}", &f[from.len()..]))?;
            }
            if n == 0 {
                return Err(PmError::NotFound);
            }
            Ok(n)
        })
    }

    pub fn pm_folder_delete(&self, folder: &str) -> PmResult<usize> {
        let folder = clean_folder(folder);
        if folder.is_empty() {
            return Err(invalid("folder name is required"));
        }
        let parent = folder.rsplit_once('/').map_or("", |(p, _)| p).to_owned();
        self.pm_write(|tx| {
            let mut n = 0;
            for (id, f) in in_folder(tx, &folder)? {
                let rest = f[folder.len()..].trim_start_matches('/');
                let to = [parent.as_str(), rest].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("/");
                n += set_folder(tx, &id, &to)?;
            }
            if n == 0 {
                return Err(PmError::NotFound);
            }
            Ok(n)
        })
    }

    pub fn pm_repair_imported(&self) -> PmResult<usize> {
        self.pm_write(|tx| Ok(repair_imported(tx)?))
    }

    pub fn pm_icon_hosts(&self) -> PmResult<Vec<(String, Option<i64>)>> {
        self.pm(|c| {
            let mut st = c.prepare("SELECT urls FROM pm_entries")?;
            let mut hosts: Vec<String> =
                st.query_map([], |r| r.get::<_, String>(0))?.filter_map(|u| u.ok()).filter_map(|u| icon_host(&JsonCol::<String>::parse(&u).items)).collect();
            hosts.sort();
            hosts.dedup();
            let fetched: std::collections::HashMap<String, i64> = (|| -> rusqlite::Result<_> {
                let mut st = c.prepare("SELECT host, fetched FROM pm_icons")?;
                st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.collect()
            })()
            .unwrap_or_default();
            Ok(hosts.into_iter().map(|h| {
                let f = fetched.get(&h).copied();
                (h, f)
            }).collect())
        })
    }

    pub fn pm_icon_put(&self, host: &str, png: Option<&[u8]>, when: i64) -> PmResult<()> {
        if png.is_some_and(|p| p.len() > MAX_ICON || !p.starts_with(b"\x89PNG\r\n\x1a\n")) {
            return Err(invalid("icon is not a small PNG"));
        }
        self.pm_write(|tx| {
            tx.execute_batch(ICONS_TABLE)?;
            match png {
                Some(p) => tx.execute("INSERT OR REPLACE INTO pm_icons(host, png, fetched) VALUES (?, ?, ?)", params![host, p, when])?,
                None => tx.execute(
                    "INSERT INTO pm_icons(host, png, fetched) VALUES (?1, NULL, ?2) ON CONFLICT(host) DO UPDATE SET fetched = ?2",
                    params![host, when],
                )?,
            };
            Ok(())
        })
    }

    pub fn pm_icon_get(&self, host: &str) -> PmResult<Vec<u8>> {
        self.pm(|c| {
            c.query_row("SELECT png FROM pm_icons WHERE host = ? AND png IS NOT NULL", [host], |r| r.get::<_, Vec<u8>>(0))
                .optional()
                .ok()
                .flatten()
                .ok_or(PmError::NotFound)
        })
    }

    pub fn pm_insert_many(&self, items: Vec<ImportedEntry>) -> PmResult<Vec<String>> {
        let now = now_ms();
        let mut rows = Vec::with_capacity(items.len());
        for it in items {
            let mut row = empty_row(new_id(), it.created.unwrap_or(now));
            apply(&mut row, it.entry)?;
            row.summary.updated = it.updated.unwrap_or(row.summary.created);
            rows.push((row, it.history));
        }
        self.pm_write(|tx| {
            let mut ids = Vec::with_capacity(rows.len());
            for (row, mut hist) in rows {
                write_row(tx, &row, true)?;
                hist.sort_by_key(|h| h.1);
                for (pw, when) in hist.iter().filter(|(p, _)| !p.is_empty() && **p != *row.password) {
                    push_history(tx, &row.summary.id, pw, *when)?;
                }
                ids.push(row.summary.id.clone());
            }
            Ok(ids)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KdfPreset;
    use secrecy::SecretString;

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }
    fn z(s: &str) -> Option<Zeroizing<String>> {
        Some(Zeroizing::new(s.to_owned()))
    }

    fn sample() -> EntryPatch {
        EntryPatch {
            title: Some("Posta Ufficio".into()),
            username: Some("mario.rossi@example.it".into()),
            password: z("ZebraCanguro#4471"),
            urls: Some(vec!["https://mail.example.it".into(), " ".into(), "https://mail.example.it".into()]),
            notes: z("PIN della porta: 1234"),
            totp: z("otpauth://totp/Example:mario?secret=JBSWY3DPEHPK3PXP&issuer=Example"),
            folder: Some(" Lavoro / Posta ".into()),
            tags: Some(vec!["lavoro".into(), "email".into()]),
            favorite: Some(true),
            fields: Some(vec![
                FieldInput { name: "Domanda".into(), value: z("Nome del gatto"), hidden: false },
                FieldInput { name: "Risposta".into(), value: z("Fuffi-Segreto-889"), hidden: true },
            ]),
        }
    }

    #[test]
    fn crud_search_history_and_reveal() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("pm.nvault");
        let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
        assert!(v.pm_list().unwrap().is_empty());
        let e = v.pm_create(sample()).unwrap();
        let s = &e.summary;
        assert_eq!(s.urls, vec!["https://mail.example.it"]);
        assert_eq!(s.folder, "Lavoro/Posta");
        assert!(s.favorite && s.has_password && s.has_totp);
        assert_eq!(e.fields[0].value.as_deref(), Some("Nome del gatto"));
        assert_eq!(e.fields[1].value, None, "hidden field values stay out of details");
        assert_eq!(e.totp.as_ref().unwrap().issuer.as_deref(), Some("Example"));
        let json = serde_json::to_string(&e).unwrap() + &serde_json::to_string(&v.pm_list().unwrap()).unwrap();
        assert!(!json.contains("ZebraCanguro") && !json.contains("Fuffi") && !json.contains("JBSWY3DP"), "{json}");

        let id = s.id.clone();
        let b = v.pm_create(EntryPatch { title: Some("banca".into()), username: Some("mr".into()), ..Default::default() }).unwrap();
        assert!(!b.summary.has_password);
        assert!(matches!(v.pm_create(EntryPatch::default()), Err(PmError::Invalid(_))));
        assert!(matches!(
            v.pm_create(EntryPatch { title: Some("x".into()), totp: z("otpauth://hotp/x?secret=AA"), ..Default::default() }),
            Err(PmError::Totp(_))
        ));

        assert_eq!(v.pm_list().unwrap().iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["banca", "Posta Ufficio"]);
        assert_eq!(v.pm_search("MAIL.example").unwrap().len(), 1);
        assert_eq!(v.pm_search("lavoro rossi").unwrap().len(), 1);
        assert_eq!(v.pm_search("posta").unwrap().len(), 1, "folder");
        assert_eq!(v.pm_search("email").unwrap().len(), 1, "tag");
        assert_eq!(v.pm_search("lavoro banca").unwrap().len(), 0);
        assert_eq!(v.pm_search("  ").unwrap().len(), 2);
        assert!(v.pm_search("Zebra").unwrap().is_empty(), "passwords are not searchable");

        assert_eq!(*v.pm_reveal(&id, Secret::Password).unwrap(), "ZebraCanguro#4471");
        assert_eq!(*v.pm_reveal(&id, Secret::Field(1)).unwrap(), "Fuffi-Segreto-889");
        assert!(v.pm_reveal(&id, Secret::Totp).unwrap().starts_with("otpauth://totp/"));
        assert_eq!(v.pm_reveal(&id, Secret::TotpCode).unwrap().len(), 6);
        assert!(matches!(v.pm_reveal(&id, Secret::Field(5)), Err(PmError::NotFound)));
        assert!(matches!(v.pm_reveal(&b.summary.id, Secret::Password), Err(PmError::NotFound)));
        assert!(matches!(v.pm_reveal("nope", Secret::Password), Err(PmError::NotFound)));
        let code = v.pm_totp(&id).unwrap();
        assert_eq!((code.digits, code.period), (6, 30));
        assert!((1..=30).contains(&code.remaining));

        for i in 0..12 {
            v.pm_update(&id, EntryPatch { password: z(&format!("nuova-{i}")), ..Default::default() }).unwrap();
        }
        let h = v.pm_history(&id).unwrap();
        assert_eq!(h.len(), HISTORY_KEEP);
        assert_eq!(*v.pm_reveal(&id, Secret::History(0)).unwrap(), "nuova-10");
        assert_eq!(*v.pm_reveal(&id, Secret::History(9)).unwrap(), "nuova-1");
        let same = v.pm_update(&id, EntryPatch { password: z("nuova-11"), ..Default::default() }).unwrap();
        assert_eq!(same.history_count, HISTORY_KEEP, "unchanged password adds no history");
        let e2 = v
            .pm_update(
                &id,
                EntryPatch {
                    title: Some("Posta".into()),
                    totp: z(""),
                    fields: Some(vec![FieldInput { name: "Risposta".into(), value: None, hidden: true }]),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(e2.summary.title, "Posta");
        assert!(!e2.summary.has_totp && e2.totp.is_none());
        assert_eq!(e2.summary.username, "mario.rossi@example.it", "untouched");
        assert_eq!(*v.pm_reveal(&id, Secret::Field(0)).unwrap(), "Fuffi-Segreto-889");
        assert!(e2.summary.updated >= e2.summary.created);

        v.close().unwrap();
        let v = Vault::open(&path, &pw("pw")).unwrap();
        assert_eq!(v.pm_get(&id).unwrap().summary.title, "Posta");
        assert_eq!(v.pm_history(&id).unwrap().len(), HISTORY_KEEP);
        v.pm_delete(&id).unwrap();
        assert!(matches!(v.pm_delete(&id), Err(PmError::NotFound)));
        assert!(matches!(v.pm_get(&id), Err(PmError::NotFound)));
        assert!(matches!(v.pm_history(&id), Err(PmError::NotFound)));
        let left: i64 = v.with(|i| i.conn.query_row("SELECT count(*) FROM pm_history", [], |r| r.get(0)).map_err(dberr)).unwrap();
        assert_eq!(left, 0, "history goes with the entry");
        v.close().unwrap();
    }

    #[test]
    fn migrates_a_volume_made_before_the_password_manager() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("old.nvault");
        let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
        v.with(|i| {
            i.conn
                .execute_batch(&format!("DROP TABLE pm_history; DROP TABLE pm_entries; DELETE FROM meta WHERE k = '{SCHEMA_KEY}';"))
                .map_err(dberr)
        })
        .unwrap();
        assert!(v.pm_list().is_err());
        v.close().unwrap();

        let v = Vault::open(&path, &pw("pw")).unwrap();
        assert_eq!(v.meta_get(SCHEMA_KEY).unwrap().as_deref(), Some(SCHEMA_VERSION.to_string().as_bytes()));
        let e = v.pm_create(sample()).unwrap();
        assert_eq!(v.pm_list().unwrap()[0].id, e.summary.id);
        v.check_integrity().unwrap();
        v.close().unwrap();
        let v = Vault::open(&path, &pw("pw")).unwrap();
        assert_eq!(v.pm_list().unwrap().len(), 1);
        v.close().unwrap();
    }

    #[test]
    fn entry_strings_never_reach_the_file_in_clear() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("opaque.nvault");
        let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
        let mut p = sample();
        p.title = Some("TitoloInconfondibileQX".into());
        p.username = Some("UtenteInconfondibileQX".into());
        p.urls = Some(vec!["https://inconfondibile-qx.example".into()]);
        let id = v.pm_create(p).unwrap().summary.id;
        v.pm_update(&id, EntryPatch { password: z("SecondaPasswordQX"), ..Default::default() }).unwrap();
        v.close().unwrap();
        let mut raw = std::fs::read(&path).unwrap();
        for side in ["-wal", "-shm"] {
            let mut p = path.as_os_str().to_owned();
            p.push(side);
            raw.extend(std::fs::read(p).unwrap_or_default());
        }
        for needle in [
            "TitoloInconfondibileQX",
            "UtenteInconfondibileQX",
            "inconfondibile-qx",
            "ZebraCanguro",
            "SecondaPasswordQX",
            "Fuffi-Segreto",
            "JBSWY3DPEHPK3PXP",
            "PIN della porta",
            "pm_entries",
        ] {
            assert!(!raw.windows(needle.len()).any(|w| w == needle.as_bytes()), "{needle} found in the raw volume");
        }
    }

    fn raw_row(v: &Vault, id: &str) -> Vec<(String, String)> {
        v.with(|i| {
            let mut st = i.conn.prepare("SELECT * FROM pm_entries WHERE id = ?").map_err(dberr)?;
            let names: Vec<String> = st.column_names().iter().map(|s| s.to_string()).collect();
            st.query_row([id], |r| {
                (0..names.len())
                    .map(|c| {
                        let v: rusqlite::types::Value = r.get(c)?;
                        Ok((names[c].clone(), format!("{v:?}")))
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(dberr)
        })
        .unwrap()
    }

    fn col<'a>(row: &'a [(String, String)], name: &str) -> &'a str {
        &row.iter().find(|(n, _)| n == name).unwrap().1
    }

    #[test]
    fn newer_row_shapes_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("fwd.nvault");
        let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
        let id = v.pm_create(sample()).unwrap().summary.id;
        v.with(|i| {
            i.conn
                .execute_batch(&format!(
                    r#"ALTER TABLE pm_entries ADD COLUMN future TEXT NOT NULL DEFAULT '';
                    UPDATE pm_entries SET
                      fields = '[{{"name":"Domanda","value":"Nome del gatto","hidden":false,"kind":"date"}},{{"attachment":"a1"}},{{"name":"Risposta","value":"Fuffi","hidden":true}}]',
                      urls = '["https://mail.example.it",{{"url":"https://b.example","match":"host"}}]',
                      tags = '{{"set":["x"]}}',
                      totp = 'otpauth://steam/Example?secret=JBSWY3DPEHPK3PXP',
                      future = 'keep me'
                    WHERE id = '{id}';"#
                ))
                .map_err(dberr)
        })
        .unwrap();
        let before = raw_row(&v, &id);

        let d = v.pm_get(&id).unwrap();
        assert_eq!(d.summary.urls, ["https://mail.example.it"]);
        assert!(d.summary.tags.is_empty());
        assert_eq!(d.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Domanda", "Risposta"]);
        assert!(d.summary.has_totp && d.totp.is_none());
        assert_eq!(*v.pm_reveal(&id, Secret::Field(1)).unwrap(), "Fuffi");
        assert_eq!(v.pm_list().unwrap()[0].urls, ["https://mail.example.it"]);

        v.pm_update(&id, EntryPatch { title: Some("Nuovo titolo".into()), ..Default::default() }).unwrap();
        let after = raw_row(&v, &id);
        for ((n, b), (_, a)) in before.iter().zip(&after) {
            if n != "title" && n != "updated" {
                assert_eq!(a, b, "column {n} rewritten by a title change");
            }
        }
        v.pm_update(&id, EntryPatch { password: z("altra"), ..Default::default() }).unwrap();
        let after2 = raw_row(&v, &id);
        for ((n, b), (_, a)) in after.iter().zip(&after2) {
            if n != "password" && n != "updated" {
                assert_eq!(a, b, "column {n} rewritten by a password change");
            }
        }

        v.pm_update(
            &id,
            EntryPatch {
                fields: Some(vec![
                    FieldInput { name: "Domanda".into(), value: z("Nome del cane"), hidden: false },
                    FieldInput { name: "Nuovo".into(), value: z("n"), hidden: false },
                ]),
                urls: Some(vec!["https://c.example".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        let r = raw_row(&v, &id);
        let text = |c: &str| -> serde_json::Value {
            let s: String = v.with(|i| i.conn.query_row(&format!("SELECT {c} FROM pm_entries WHERE id = ?"), [&id], |r| r.get(0)).map_err(dberr)).unwrap();
            serde_json::from_str(&s).unwrap()
        };
        let fields = text("fields");
        assert_eq!(
            fields,
            serde_json::json!([
                {"name":"Domanda","value":"Nome del cane","hidden":false,"kind":"date"},
                {"name":"Nuovo","value":"n","hidden":false},
                {"attachment":"a1"}
            ])
        );
        assert_eq!(text("urls"), serde_json::json!(["https://c.example", {"url":"https://b.example","match":"host"}]));
        assert_eq!(col(&r, "future"), col(&before, "future"));
        assert_eq!(col(&r, "totp"), col(&before, "totp"));

        assert!(matches!(v.pm_update(&id, EntryPatch { tags: Some(vec!["t".into()]), ..Default::default() }), Err(PmError::Unsupported(_))));
        assert_eq!(col(&raw_row(&v, &id), "tags"), col(&before, "tags"));
        v.close().unwrap();
    }

    #[test]
    fn missing_or_foreign_schema_key() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("k.nvault");
        let v = Vault::create(&path, &pw("pw"), KdfPreset::Fast).unwrap();
        let id = v.pm_create(sample()).unwrap().summary.id;
        v.meta_set(SCHEMA_KEY, None).unwrap();
        v.close().unwrap();
        let v = Vault::open(&path, &pw("pw")).unwrap();
        assert_eq!(v.pm_get(&id).unwrap().summary.title, "Posta Ufficio");
        assert_eq!(v.meta_get(SCHEMA_KEY).unwrap().as_deref(), Some(SCHEMA_VERSION.to_string().as_bytes()));
        v.meta_set(SCHEMA_KEY, Some(b"2-beta")).unwrap();
        v.close().unwrap();
        let v = Vault::open(&path, &pw("pw")).unwrap();
        assert_eq!(v.meta_get(SCHEMA_KEY).unwrap().as_deref(), Some(&b"2-beta"[..]));
        assert_eq!(v.pm_list().unwrap().len(), 1);
        v.close().unwrap();
    }

    #[test]
    fn secret_names() {
        assert_eq!(Secret::parse("password"), Some(Secret::Password));
        assert_eq!(Secret::parse("field:3"), Some(Secret::Field(3)));
        assert_eq!(Secret::parse("history:0"), Some(Secret::History(0)));
        assert_eq!(Secret::parse("totp_code"), Some(Secret::TotpCode));
        assert_eq!(Secret::parse("field:x"), None);
        assert_eq!(Secret::parse("other"), None);
    }
}
