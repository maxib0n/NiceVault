//! File search, filters and tags, all inside the volume.
//!
//! Additive tables, created by a versioned migration (`meta nv.search_schema` = 1, one transaction,
//! idempotent, run at every open and create after the volume_id check):
//!
//! ```sql
//! CREATE TABLE search_names (node INTEGER PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
//!                            norm TEXT NOT NULL);           -- fold(name): lowercase, no accents
//! CREATE TABLE search_dirty (node INTEGER PRIMARY KEY);     -- names to (re)fold: new or renamed nodes
//! CREATE TRIGGER search_dirty_insert AFTER INSERT ON nodes ...           -- INSERT OR IGNORE INTO search_dirty
//! CREATE TRIGGER search_dirty_rename AFTER UPDATE OF name ON nodes ...   -- (same, when the name changed)
//! CREATE TABLE tags (id INTEGER PRIMARY KEY, name TEXT NOT NULL, norm TEXT NOT NULL UNIQUE,
//!                    color TEXT NOT NULL, created INTEGER NOT NULL);
//! CREATE TABLE node_tags (node INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
//!                         tag INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
//!                         PRIMARY KEY (node, tag)) WITHOUT ROWID;
//! CREATE INDEX node_tags_by_tag ON node_tags(tag, node);
//! ```
//! The triggers use plain SQL only, so every writer keeps the index honest — this build, the
//! mounted drive, and an older build that knows nothing about search (it renames: the node is
//! queued; it deletes: the foreign keys drop the name and the tag rows). Folding needs Rust, so
//! queued names are folded later: in memory at the next search (results are always complete) and
//! persisted in batches by [`Vault::search_index_step`] (the app's background job). A volume from
//! before this table has no names yet: the same job fills them, crash-safe because every batch is
//! one atomic step and the work left is recomputed from the tables (missing or queued rows).
//! `meta nv.search_norm` = the fold version the stored names were made with; another one drops
//! them (rebuilt by the job).
//!
//! Searching scans an in-memory copy of the index (node, parent, name, folded name, kind group,
//! size, mtime, tags) built from these tables at the first search of a session and refreshed
//! for exactly the nodes changed since (the store reports them), so a query over 100k names is a
//! few milliseconds of `str::contains` with no SQL. The copy lives in the store's state: it goes
//! with the keys when the volume locks.

use crate::{Inner, Vault, dberr, now_ns};
use nv_fs::{Error, Kind, Node, NodeId, ROOT, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// `meta` key of this schema's version (ASCII integer).
pub const SEARCH_SCHEMA_KEY: &str = "nv.search_schema";
pub const SEARCH_SCHEMA_VERSION: u32 = 1;
/// `meta` key: version of [`fold`] the stored `search_names.norm` / `tags.norm` were made with.
pub const SEARCH_NORM_KEY: &str = "nv.search_norm";
const NORM_VERSION: &str = "1";

const MIGRATIONS: [&str; 1] = ["
CREATE TABLE IF NOT EXISTS search_names (
  node INTEGER PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
  norm TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS search_dirty (node INTEGER PRIMARY KEY);
CREATE TRIGGER IF NOT EXISTS search_dirty_insert AFTER INSERT ON nodes
BEGIN
  INSERT OR IGNORE INTO search_dirty(node) VALUES (NEW.id);
END;
CREATE TRIGGER IF NOT EXISTS search_dirty_rename AFTER UPDATE OF name ON nodes WHEN NEW.name IS NOT OLD.name
BEGIN
  INSERT OR IGNORE INTO search_dirty(node) VALUES (NEW.id);
END;
CREATE TABLE IF NOT EXISTS tags (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  norm TEXT NOT NULL UNIQUE,
  color TEXT NOT NULL,
  created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS node_tags (
  node INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  tag INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (node, tag)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS node_tags_by_tag ON node_tags(tag, node);
"];

fn meta_version(conn: &Connection, key: &str) -> rusqlite::Result<Option<Option<u32>>> {
    use rusqlite::types::Value;
    let raw: Option<Value> = conn.query_row("SELECT v FROM meta WHERE k = ?", [key], |r| r.get(0)).optional()?;
    let parse = |b: &[u8]| std::str::from_utf8(b).ok().and_then(|s| s.trim().parse::<u32>().ok());
    Ok(raw.map(|v| match &v {
        Value::Blob(b) => parse(b),
        Value::Text(s) => parse(s.as_bytes()),
        Value::Integer(n) => u32::try_from(*n).ok(),
        _ => None,
    }))
}

fn meta_text(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    use rusqlite::types::Value;
    let raw: Option<Value> = conn.query_row("SELECT v FROM meta WHERE k = ?", [key], |r| r.get(0)).optional()?;
    Ok(raw.and_then(|v| match v {
        Value::Blob(b) => String::from_utf8(b).ok(),
        Value::Text(s) => Some(s),
        Value::Integer(n) => Some(n.to_string()),
        _ => None,
    }))
}

/// Applies the missing steps in one transaction; idempotent. A schema version this build cannot
/// read (garbage) is left alone; a higher one too (newer builds only add, so the v1 tables it
/// finds are still used). Runs at open and create, after the volume_id check.
pub(crate) fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let have = match meta_version(conn, SEARCH_SCHEMA_KEY)? {
        None => 0,
        Some(Some(n)) => n,
        Some(None) => return Ok(()),
    };
    let norm = meta_text(conn, SEARCH_NORM_KEY)?;
    if have > SEARCH_SCHEMA_VERSION || (have == SEARCH_SCHEMA_VERSION && norm.as_deref() == Some(NORM_VERSION)) {
        return Ok(());
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let r = (|| {
        for m in &MIGRATIONS[have as usize..] {
            conn.execute_batch(m)?;
        }
        if norm.as_deref() != Some(NORM_VERSION) {
            // names folded by another fold version: refold (the background job fills them again)
            conn.execute("DELETE FROM search_names", [])?;
            let tags: Vec<(i64, String)> =
                conn.prepare("SELECT id, name FROM tags")?.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
            for (id, name) in tags {
                // a clash under the new fold keeps both: the id makes the second one unique
                let n = tag_norm(&name);
                if conn.execute("UPDATE OR IGNORE tags SET norm = ? WHERE id = ?", params![n, id])? == 0 {
                    conn.execute("UPDATE tags SET norm = ? WHERE id = ?", params![format!("{n}\u{0}{id}"), id])?;
                }
            }
            conn.execute("INSERT OR REPLACE INTO meta(k, v) VALUES (?, ?)", params![SEARCH_NORM_KEY, NORM_VERSION.as_bytes()])?;
        }
        conn.execute(
            "INSERT OR REPLACE INTO meta(k, v) VALUES (?, ?)",
            params![SEARCH_SCHEMA_KEY, SEARCH_SCHEMA_VERSION.max(have).to_string().into_bytes()],
        )?;
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

fn missing_table(e: &rusqlite::Error) -> bool {
    e.to_string().contains("no such table")
}

// ---------------------------------------------------------------------------
// folding

/// The search form of a text: lowercase, accents and other combining marks removed (NFD), a few
/// letters that do not decompose spelled out (ß ss, æ ae, œ oe, ø o, đ d, ł l, þ th, ı i).
/// `Città` and `CITTA` both fold to `citta`. Version [`NORM_VERSION`]; any change bumps it.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        fold_char(c, &mut out);
    }
    out
}

fn fold_char(c: char, out: &mut String) {
    if c.is_ascii() {
        out.push(c.to_ascii_lowercase());
        return;
    }
    let special = match c {
        'ß' | 'ẞ' => "ss",
        'æ' | 'Æ' => "ae",
        'œ' | 'Œ' => "oe",
        'ø' | 'Ø' => "o",
        'đ' | 'Đ' => "d",
        'ł' | 'Ł' => "l",
        'þ' | 'Þ' => "th",
        'ı' => "i",
        _ => "",
    };
    if !special.is_empty() {
        out.push_str(special);
        return;
    }
    unicode_normalization::char::decompose_canonical(c, |d| {
        if !unicode_normalization::char::is_combining_mark(d) {
            out.extend(d.to_lowercase());
        }
    });
}

/// `fold(s)` plus, for every byte of it, the UTF-16 range of the character of `s` it came from
/// (highlights are UTF-16 offsets: what JavaScript strings index by).
fn fold_mapped(s: &str) -> (String, Vec<(u32, u32)>) {
    let mut out = String::with_capacity(s.len());
    let mut map = Vec::with_capacity(s.len());
    let mut u16pos = 0u32;
    for c in s.chars() {
        let before = out.len();
        fold_char(c, &mut out);
        let end = u16pos + c.len_utf16() as u32;
        map.extend(std::iter::repeat_n((u16pos, end), out.len() - before));
        u16pos = end;
    }
    (out, map)
}

fn tag_norm(name: &str) -> String {
    fold(name).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Search terms of a query: whitespace-separated, `"..."` keeps a phrase together; folded,
/// deduplicated. Every term must occur in a name (substring, anywhere).
pub fn terms(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut push = |t: &str| {
        let f = fold(t.trim());
        if !f.is_empty() && !out.contains(&f) {
            out.push(f);
        }
    };
    let mut rest = text;
    while !rest.is_empty() {
        rest = rest.trim_start();
        if let Some(q) = rest.strip_prefix('"') {
            let (phrase, after) = q.split_once('"').unwrap_or((q, ""));
            push(phrase);
            rest = after;
        } else {
            let end = rest.find(|c: char| c.is_whitespace() || c == '"').unwrap_or(rest.len());
            push(&rest[..end]);
            rest = &rest[end..];
        }
    }
    out
}

// ---------------------------------------------------------------------------
// kind groups

/// What a node is, for filters: from the extension (folders: `folders`). The UI mirrors the map
/// (app/ui/src/lib/search.ts).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KindGroup {
    Photos,
    Videos,
    Audio,
    Documents,
    Archives,
    Code,
    Other,
    Folders,
}

const GROUPS: [(KindGroup, &str); 6] = [
    (KindGroup::Photos, "jpg jpeg jpe jfif png gif webp avif bmp svg heic heif tif tiff ico jxl raw cr2 cr3 nef arw dng orf rw2 raf psd xcf"),
    (KindGroup::Videos, "mp4 m4v webm mov mkv avi wmv flv mpg mpeg 3gp ogv mts m2ts vob"),
    (KindGroup::Audio, "mp3 ogg oga opus wav flac m4a aac wma aif aiff mid midi amr ape"),
    (
        KindGroup::Documents,
        "pdf doc docx odt rtf pages txt md markdown tex log xls xlsx ods numbers csv tsv ppt pptx odp key epub mobi djvu xps",
    ),
    (KindGroup::Archives, "zip gz tgz tar 7z rar xz txz bz2 tbz2 zst lz4 lzma iso dmg cab jar apk deb rpm"),
    (
        KindGroup::Code,
        "json yaml yml toml xml html htm css scss sass less js mjs cjs jsx ts tsx rs py rb go java kt kts c h cpp hpp cc hh cs swift php sh bash zsh fish ps1 bat cmd sql lua pl r dart scala vue svelte ini conf cfg ipynb diff patch",
    ),
];

/// The kind group of a node named `name`.
pub fn kind_group(name: &str, dir: bool) -> KindGroup {
    if dir {
        return KindGroup::Folders;
    }
    let Some((stem, ext)) = name.rsplit_once('.') else { return KindGroup::Other };
    if stem.is_empty() || ext.is_empty() || ext.len() > 8 {
        return KindGroup::Other;
    }
    let mut buf = [0u8; 8];
    let ext = ext.as_bytes();
    for (b, c) in buf.iter_mut().zip(ext) {
        *b = c.to_ascii_lowercase();
    }
    let ext = &buf[..ext.len()];
    for (g, list) in GROUPS {
        if list.split(' ').any(|e| e.as_bytes() == ext) {
            return g;
        }
    }
    KindGroup::Other
}

// ---------------------------------------------------------------------------
// filters + queries

pub type TagId = u64;

/// Narrows search results (and, in the app, a folder listing). Every field is optional; all
/// given ones must hold.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Filters {
    /// Any of these groups (empty = any).
    pub kinds: Vec<KindGroup>,
    /// unix ns, inclusive.
    pub modified_after: Option<i64>,
    /// unix ns, exclusive.
    pub modified_before: Option<i64>,
    /// Bytes, inclusive. A size bound leaves folders out.
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    /// Nodes carrying these tags (empty = no tag condition): all of them, or any with `tags_any`.
    pub tags: Vec<TagId>,
    pub tags_any: bool,
    /// Search only: inside this folder (any depth). Default: the whole volume.
    pub within: Option<NodeId>,
}

impl Filters {
    /// Whether a node passes (everything but `within`).
    pub fn matches(&self, group: KindGroup, size: u64, mtime: i64, tags: &[TagId]) -> bool {
        if !self.kinds.is_empty() && !self.kinds.contains(&group) {
            return false;
        }
        if self.modified_after.is_some_and(|t| mtime < t) || self.modified_before.is_some_and(|t| mtime >= t) {
            return false;
        }
        if (self.min_size.is_some() || self.max_size.is_some())
            && (group == KindGroup::Folders || self.min_size.is_some_and(|s| size < s) || self.max_size.is_some_and(|s| size > s))
        {
            return false;
        }
        if !self.tags.is_empty() {
            let has = |t: &TagId| tags.contains(t);
            if if self.tags_any { !self.tags.iter().any(has) } else { !self.tags.iter().all(has) } {
                return false;
            }
        }
        true
    }

    /// [`Filters::matches`] for a node.
    pub fn matches_node(&self, n: &Node, tags: &[TagId]) -> bool {
        self.matches(kind_group(&n.name, n.kind == Kind::Dir), n.size, n.mtime, tags)
    }
}

/// Result order. Folders come first in every order but `relevance`, like the file browser.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    /// best match first (whole name, name start, word starts, anywhere), then by name; with no
    /// terms the same as `name`
    #[default]
    Relevance,
    Name,
    Size,
    Mtime,
    /// by extension, then name
    Type,
}

#[derive(Clone, Debug, Default)]
pub struct SearchQuery {
    pub text: String,
    pub filters: Filters,
    pub sort: Sort,
    pub desc: bool,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hit {
    pub node: Node,
    /// The folder holding it: "/" or "/Foto/Mare".
    pub path: String,
    /// `[start, end)` UTF-16 ranges of `node.name` that matched the terms (sorted, merged).
    pub highlights: Vec<[u32; 2]>,
    pub tags: Vec<TagId>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchPage {
    pub hits: Vec<Hit>,
    /// Matches in all (this page is `offset .. offset + hits.len()` of them).
    pub total: usize,
    /// Offset of the next page, `None` = this was the last.
    pub next_offset: Option<usize>,
    /// Names not persisted in the index yet ([`Vault::search_index_step`]); the results are
    /// complete anyway.
    pub pending: u64,
}

// ---------------------------------------------------------------------------
// the in-memory index

struct Entry {
    id: NodeId,
    parent: NodeId,
    size: u64,
    mtime: i64,
    group: KindGroup,
    name: Box<str>,
    norm: Box<str>,
}

/// More changed nodes than this since the last search: rebuild instead of refreshing each.
const MAX_TOUCHED: usize = 20_000;

/// Search state of an open volume (lives in [`Inner`], dropped with it at lock).
#[derive(Default)]
pub(crate) struct Cache {
    built: bool,
    entries: Vec<Entry>,
    pos: HashMap<NodeId, usize>,
    /// Nodes changed since the copy was made (the store reports them).
    touched: HashSet<NodeId>,
    tags: HashMap<NodeId, Vec<TagId>>,
    /// Rows without a stored name and not queued, as of the build (minus what was stored since).
    missing: u64,
    /// The missing-rows scan of [`Vault::search_index_step`] continues after this id.
    scan_from: i64,
}

impl Cache {
    /// The store changed these nodes (created, renamed, moved, removed, resized, retimed, tagged).
    pub(crate) fn touch(&mut self, ids: impl IntoIterator<Item = NodeId>) {
        if !self.built {
            return;
        }
        self.touched.extend(ids);
        if self.touched.len() > MAX_TOUCHED {
            self.reset();
        }
    }

    /// Forget the copy (rebuilt at the next search), e.g. after SQLite rolled a transaction back.
    pub(crate) fn reset(&mut self) {
        let scan_from = self.scan_from;
        *self = Cache { scan_from, ..Default::default() };
    }

    fn put(&mut self, e: Entry) {
        match self.pos.get(&e.id) {
            Some(&p) => self.entries[p] = e,
            None => {
                self.pos.insert(e.id, self.entries.len());
                self.entries.push(e);
            }
        }
    }

    fn drop_entry(&mut self, id: NodeId) {
        if let Some(p) = self.pos.remove(&id) {
            self.entries.swap_remove(p);
            if let Some(moved) = self.entries.get(p) {
                self.pos.insert(moved.id, p);
            }
        }
    }

    fn build(&mut self, conn: &Connection) -> Result<()> {
        let stored = meta_text(conn, SEARCH_NORM_KEY).map_err(dberr)?.as_deref() == Some(NORM_VERSION);
        // Three plain scans merged by id: much cheaper than a join (one b-tree probe per row each).
        let mut names: Vec<(u64, String)> = vec![];
        let mut dirty: HashSet<NodeId> = HashSet::new();
        if stored {
            match conn.prepare("SELECT node, norm FROM search_names ORDER BY node") {
                Ok(mut st) => {
                    names = st
                        .query_map([], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?)))
                        .map_err(dberr)?
                        .collect::<rusqlite::Result<_>>()
                        .map_err(dberr)?;
                    dirty = conn
                        .prepare("SELECT node FROM search_dirty")
                        .map_err(dberr)?
                        .query_map([], |r| Ok(r.get::<_, i64>(0)? as u64))
                        .map_err(dberr)?
                        .collect::<rusqlite::Result<_>>()
                        .map_err(dberr)?;
                }
                Err(e) if missing_table(&e) => {}
                Err(e) => return Err(dberr(e)),
            }
        }
        let mut st = conn.prepare("SELECT id, parent, name, kind, size, mtime FROM nodes WHERE id != 1 ORDER BY id").map_err(dberr)?;
        let mut rows = st.query([]).map_err(dberr)?;
        let (mut entries, mut pos, mut missing) = (Vec::new(), HashMap::new(), 0u64);
        let mut stored_names = names.into_iter().peekable();
        while let Some(r) = rows.next().map_err(dberr)? {
            let id = r.get::<_, i64>(0).map_err(dberr)? as u64;
            let name: String = r.get(2).map_err(dberr)?;
            let dir = r.get::<_, i64>(3).map_err(dberr)? == 1;
            while stored_names.next_if(|(n, _)| *n < id).is_some() {}
            let norm = match stored_names.next_if(|(n, _)| *n == id) {
                Some((_, n)) if !dirty.contains(&id) => n,
                had => {
                    missing += u64::from(stored && had.is_none() && !dirty.contains(&id));
                    fold(&name)
                }
            };
            pos.insert(id, entries.len());
            entries.push(Entry {
                id,
                parent: r.get::<_, i64>(1).map_err(dberr)? as u64,
                size: r.get::<_, i64>(4).map_err(dberr)? as u64,
                mtime: r.get(5).map_err(dberr)?,
                group: kind_group(&name, dir),
                name: name.into(),
                norm: norm.into(),
            });
        }
        drop(rows);
        drop(st);
        let mut tags: HashMap<NodeId, Vec<TagId>> = HashMap::new();
        match conn.prepare("SELECT node, tag FROM node_tags") {
            Ok(mut st) => {
                let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64))).map_err(dberr)?;
                for row in rows {
                    let (n, t) = row.map_err(dberr)?;
                    tags.entry(n).or_default().push(t);
                }
            }
            Err(e) if missing_table(&e) => {}
            Err(e) => return Err(dberr(e)),
        }
        *self = Cache { built: true, entries, pos, touched: HashSet::new(), tags, missing, scan_from: self.scan_from };
        Ok(())
    }

    /// Brings the copy up to date: built once, then only the nodes changed since.
    fn refresh(&mut self, conn: &Connection) -> Result<()> {
        if !self.built {
            return self.build(conn);
        }
        if self.touched.is_empty() {
            return Ok(());
        }
        let mut node = conn.prepare_cached("SELECT id, parent, name, kind, size, mtime FROM nodes WHERE id = ? AND id != 1").map_err(dberr)?;
        let mut tags = match conn.prepare_cached("SELECT tag FROM node_tags WHERE node = ?") {
            Ok(s) => Some(s),
            Err(e) if missing_table(&e) => None,
            Err(e) => return Err(dberr(e)),
        };
        for id in std::mem::take(&mut self.touched) {
            let row = node
                .query_row([id as i64], |r| {
                    Ok((r.get::<_, i64>(1)? as u64, r.get::<_, String>(2)?, r.get::<_, i64>(3)? == 1, r.get::<_, i64>(4)? as u64, r.get::<_, i64>(5)?))
                })
                .optional()
                .map_err(dberr)?;
            let Some((parent, name, dir, size, mtime)) = row else {
                self.drop_entry(id);
                self.tags.remove(&id);
                continue;
            };
            let norm = match self.pos.get(&id).map(|&p| &self.entries[p]) {
                Some(e) if *e.name == *name => e.norm.clone(),
                _ => fold(&name).into(),
            };
            self.put(Entry { id, parent, size, mtime, group: kind_group(&name, dir), name: name.into(), norm });
            if let Some(st) = tags.as_mut() {
                let t: Vec<TagId> =
                    st.query_map([id as i64], |r| Ok(r.get::<_, i64>(0)? as u64)).map_err(dberr)?.collect::<rusqlite::Result<_>>().map_err(dberr)?;
                if t.is_empty() {
                    self.tags.remove(&id);
                } else {
                    self.tags.insert(id, t);
                }
            }
        }
        Ok(())
    }

    fn entry(&self, id: NodeId) -> Option<&Entry> {
        self.pos.get(&id).map(|&p| &self.entries[p])
    }

    /// "/A/B" of folder `dir` ("/" for the root), memoised per query.
    fn dir_path(&self, dir: NodeId, memo: &mut HashMap<NodeId, String>) -> String {
        if dir == ROOT {
            return "/".into();
        }
        if let Some(p) = memo.get(&dir) {
            return p.clone();
        }
        let mut parts = vec![];
        let mut cur = dir;
        for _ in 0..4096 {
            if cur == ROOT {
                break;
            }
            let Some(e) = self.entry(cur) else { break };
            parts.push(&*e.name);
            cur = e.parent;
        }
        parts.reverse();
        let p = format!("/{}", parts.join("/"));
        memo.insert(dir, p.clone());
        p
    }

    /// Whether `id` is `top` or below it, memoised per query.
    fn inside(&self, id: NodeId, top: NodeId, memo: &mut HashMap<NodeId, bool>) -> bool {
        let mut chain = vec![];
        let mut cur = id;
        let found = loop {
            if cur == top {
                break true;
            }
            if cur == ROOT || chain.len() > 4096 {
                break false;
            }
            if let Some(&v) = memo.get(&cur) {
                break v;
            }
            chain.push(cur);
            match self.entry(cur) {
                Some(e) => cur = e.parent,
                None => break false,
            }
        };
        for c in chain {
            memo.insert(c, found);
        }
        found
    }

    fn run(&self, q: &SearchQuery, overlay: &dyn Fn(NodeId) -> Option<(u64, i64)>) -> (Vec<Hit>, usize, Option<usize>) {
        let terms = terms(&q.text);
        let joined = terms.join(" ");
        let none: Vec<TagId> = vec![];
        let mut inside_memo = HashMap::new();
        let within = q.filters.within.filter(|&w| w != ROOT);
        // (rank, entry index, size, mtime)
        let mut found: Vec<(u8, usize, u64, i64)> = vec![];
        for (idx, e) in self.entries.iter().enumerate() {
            if !terms.iter().all(|t| e.norm.contains(t.as_str())) {
                continue;
            }
            let (size, mtime) = overlay(e.id).unwrap_or((e.size, e.mtime));
            let tags = self.tags.get(&e.id).unwrap_or(&none);
            if !q.filters.matches(e.group, size, mtime, tags) {
                continue;
            }
            if let Some(w) = within
                && (e.id == w || !self.inside(e.parent, w, &mut inside_memo))
            {
                continue;
            }
            found.push((if terms.is_empty() { 0 } else { rank(&e.norm, &terms, &joined) }, idx, size, mtime));
        }
        let total = found.len();
        let sort = if terms.is_empty() && q.sort == Sort::Relevance { Sort::Name } else { q.sort };
        let cmp = |a: &(u8, usize, u64, i64), b: &(u8, usize, u64, i64)| -> Ordering {
            let (ea, eb) = (&self.entries[a.1], &self.entries[b.1]);
            let (da, db) = (ea.group == KindGroup::Folders, eb.group == KindGroup::Folders);
            let by_name = || natural_cmp(&ea.norm, &eb.norm).then_with(|| ea.name.cmp(&eb.name)).then(ea.id.cmp(&eb.id));
            let r = match sort {
                Sort::Relevance => a.0.cmp(&b.0).then(db.cmp(&da)).then_with(by_name),
                _ => {
                    if da != db {
                        return db.cmp(&da); // folders first, whatever the direction
                    }
                    match sort {
                        Sort::Size => a.2.cmp(&b.2).then_with(by_name),
                        Sort::Mtime => a.3.cmp(&b.3).then_with(by_name),
                        Sort::Type => natural_cmp(ext_of(&ea.norm), ext_of(&eb.norm)).then_with(by_name),
                        _ => by_name(),
                    }
                }
            };
            if q.desc { r.reverse() } else { r }
        };
        let limit = q.limit.max(1);
        let end = q.offset.saturating_add(limit).min(total);
        if q.offset >= total {
            return (vec![], total, None);
        }
        if end < total {
            found.select_nth_unstable_by(end, cmp);
            found.truncate(end);
        }
        found.sort_unstable_by(cmp);
        let mut path_memo = HashMap::new();
        let hits = found[q.offset..]
            .iter()
            .map(|&(_, idx, size, mtime)| {
                let e = &self.entries[idx];
                Hit {
                    node: Node {
                        id: e.id,
                        parent: e.parent,
                        name: e.name.to_string(),
                        kind: if e.group == KindGroup::Folders { Kind::Dir } else { Kind::File },
                        size,
                        mtime,
                    },
                    path: self.dir_path(e.parent, &mut path_memo),
                    highlights: highlights(&e.name, &terms),
                    tags: self.tags.get(&e.id).cloned().unwrap_or_default(),
                }
            })
            .collect();
        (hits, total, (end < total).then_some(end))
    }
}

fn ext_of(norm: &str) -> &str {
    match norm.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext,
        _ => "",
    }
}

/// 0 = the whole name (or the name without its extension) is the query, 1 = the name starts with
/// the first term, 2 = every term starts a word, 3 = anywhere.
fn rank(norm: &str, terms: &[String], joined: &str) -> u8 {
    let stem = match norm.rsplit_once('.') {
        Some((s, _)) if !s.is_empty() => s,
        _ => norm,
    };
    if norm == joined || stem == joined {
        0
    } else if norm.starts_with(terms[0].as_str()) {
        1
    } else if terms.iter().all(|t| {
        norm.match_indices(t.as_str()).any(|(i, _)| i == 0 || !norm[..i].chars().next_back().is_some_and(char::is_alphanumeric))
    }) {
        2
    } else {
        3
    }
}

/// Every occurrence of every term in `name`, as merged UTF-16 ranges.
fn highlights(name: &str, terms: &[String]) -> Vec<[u32; 2]> {
    if terms.is_empty() {
        return vec![];
    }
    let (norm, map) = fold_mapped(name);
    let mut r: Vec<[u32; 2]> = vec![];
    for t in terms {
        for (i, m) in norm.match_indices(t.as_str()) {
            r.push([map[i].0, map[i + m.len() - 1].1]);
        }
    }
    r.sort_unstable();
    let mut out: Vec<[u32; 2]> = vec![];
    for x in r {
        match out.last_mut() {
            Some(l) if x[0] <= l[1] => l[1] = l[1].max(x[1]),
            _ => out.push(x),
        }
    }
    out
}

/// Text order with digit runs compared as numbers ("foto 2" before "foto 10").
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.as_bytes(), b.as_bytes());
    loop {
        match (x.first(), y.first()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let n = x.iter().take_while(|c| c.is_ascii_digit()).count();
                let m = y.iter().take_while(|c| c.is_ascii_digit()).count();
                let (p, q) = (&x[..n], &y[..m]);
                let (p2, q2) = (trim_zeros(p), trim_zeros(q));
                let o = p2.len().cmp(&q2.len()).then_with(|| p2.cmp(q2)).then(n.cmp(&m));
                if o != Ordering::Equal {
                    return o;
                }
                x = &x[n..];
                y = &y[m..];
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(d);
                }
                x = &x[1..];
                y = &y[1..];
            }
        }
    }
}

fn trim_zeros(d: &[u8]) -> &[u8] {
    let z = d.iter().take_while(|&&c| c == b'0').count();
    &d[z.min(d.len().saturating_sub(1))..]
}

// ---------------------------------------------------------------------------
// tags

/// Tag colours: the 7 inks + neutral. Another value read from a volume (a newer build's) is
/// returned as stored; the UI shows it neutral.
pub const TAG_COLORS: [&str; 8] = ["velluto", "prugna", "prussia", "bottiglia", "seppia", "ardesia", "ottanio", "neutral"];
pub const MAX_TAGS: usize = 1000;
/// Characters (Unicode scalar values) of a tag name.
pub const MAX_TAG_NAME: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Tag {
    pub id: TagId,
    pub name: String,
    pub color: String,
    /// Nodes carrying it.
    pub count: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum TagError {
    #[error("invalid tag name")]
    InvalidName,
    #[error("invalid tag colour")]
    InvalidColor,
    #[error("a tag with this name exists")]
    Exists,
    #[error("tag not found")]
    NotFound,
    #[error("too many tags")]
    TooMany,
    /// The volume's tag tables are a newer build's (or missing): nothing written.
    #[error("tags of a newer version")]
    Unsupported,
    #[error(transparent)]
    Fs(#[from] Error),
}

fn tag_err(e: rusqlite::Error) -> TagError {
    if missing_table(&e) { TagError::Unsupported } else { TagError::Fs(dberr(e)) }
}

fn valid_tag_name(name: &str) -> std::result::Result<&str, TagError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > MAX_TAG_NAME || n.chars().any(char::is_control) {
        return Err(TagError::InvalidName);
    }
    Ok(n)
}

fn valid_color(c: &str) -> std::result::Result<&str, TagError> {
    if TAG_COLORS.contains(&c) { Ok(c) } else { Err(TagError::InvalidColor) }
}

fn tag_row(conn: &Connection, id: TagId) -> std::result::Result<Tag, TagError> {
    conn.query_row(
        "SELECT t.id, t.name, t.color, (SELECT count(*) FROM node_tags n WHERE n.tag = t.id) FROM tags t WHERE t.id = ?",
        [id as i64],
        |r| Ok(Tag { id: r.get::<_, i64>(0)? as u64, name: r.get(1)?, color: r.get(2)?, count: r.get::<_, i64>(3)? as u64 }),
    )
    .optional()
    .map_err(tag_err)?
    .ok_or(TagError::NotFound)
}

fn tag_exists(conn: &Connection, id: TagId) -> std::result::Result<(), TagError> {
    conn.query_row("SELECT 1 FROM tags WHERE id = ?", [id as i64], |_| Ok(())).optional().map_err(tag_err)?.ok_or(TagError::NotFound)
}

/// One write step for tags (savepoint of the open transaction; durable at the next durability
/// point — the app flushes after each command).
fn tag_op<T>(i: &mut Inner, f: impl FnOnce(&Connection) -> std::result::Result<T, TagError>) -> std::result::Result<T, TagError> {
    let mut out = None;
    let r = i.op(|c| match f(c) {
        Ok(v) => {
            out = Some(Ok(v));
            Ok(())
        }
        Err(e) => {
            out = Some(Err(e));
            Err(Error::Io("tag step refused".into())) // rolls the savepoint back
        }
    });
    match (r, out) {
        (_, Some(v)) => v,
        (Err(e), None) => Err(TagError::Fs(e)),
        (Ok(()), None) => unreachable!(),
    }
}

impl Vault {
    /// Searches the volume (names, filters, order, one page). See [`SearchQuery`].
    pub fn search(&self, q: &SearchQuery) -> Result<SearchPage> {
        self.with(|i| {
            let Inner { conn, search, bufs, .. } = i;
            search.refresh(conn)?;
            let overlay = |id: NodeId| bufs.get(&id).map(|b| (b.size, b.mtime));
            let none = |_: NodeId| None;
            let (hits, total, next_offset) = search.run(q, if bufs.is_empty() { &none } else { &overlay });
            let dirty = conn.query_row("SELECT count(*) FROM search_dirty", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as u64;
            Ok(SearchPage { hits, total, next_offset, pending: search.missing + dirty })
        })
    }

    /// Persists up to `max` folded names into the index (queued ones first, then nodes that have
    /// none, e.g. in a volume from before search) as one atomic step; returns how many are still
    /// pending. Idempotent and crash-safe: what is left is always recomputed from the tables.
    /// Does nothing (0) in a volume whose index this build does not write (newer fold version).
    pub fn search_index_step(&self, max: usize) -> Result<u64> {
        self.with(|i| {
            if meta_text(&i.conn, SEARCH_NORM_KEY).map_err(dberr)?.as_deref() != Some(NORM_VERSION) {
                return Ok(0);
            }
            let max = max.max(1);
            let scan_from = i.search.scan_from;
            let (_stored, scanned_to, scan_done) = i.op(|c| {
                let queued: Vec<(i64, Option<String>)> = c
                    .prepare_cached("SELECT d.node, n.name FROM search_dirty d LEFT JOIN nodes n ON n.id = d.node AND n.id != 1 LIMIT ?")
                    .map_err(dberr)?
                    .query_map([max as i64], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map_err(dberr)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(dberr)?;
                let mut put = c.prepare_cached("INSERT OR REPLACE INTO search_names(node, norm) VALUES (?, ?)").map_err(dberr)?;
                let mut del = c.prepare_cached("DELETE FROM search_names WHERE node = ?").map_err(dberr)?;
                let mut unq = c.prepare_cached("DELETE FROM search_dirty WHERE node = ?").map_err(dberr)?;
                for (id, name) in &queued {
                    match name {
                        Some(n) => put.execute(params![id, fold(n)]).map_err(dberr)?,
                        None => del.execute([id]).map_err(dberr)?,
                    };
                    unq.execute([id]).map_err(dberr)?;
                }
                let room = max - queued.len();
                if room == 0 {
                    return Ok((0u64, scan_from, false));
                }
                let missing: Vec<(i64, String)> = c
                    .prepare_cached(
                        "SELECT n.id, n.name FROM nodes n WHERE n.id > ?1 AND n.id != 1
                         AND NOT EXISTS (SELECT 1 FROM search_names s WHERE s.node = n.id) ORDER BY n.id LIMIT ?2",
                    )
                    .map_err(dberr)?
                    .query_map(params![scan_from, room as i64], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map_err(dberr)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(dberr)?;
                for (id, n) in &missing {
                    put.execute(params![id, fold(n)]).map_err(dberr)?;
                }
                Ok((missing.len() as u64, missing.last().map_or(scan_from, |m| m.0), missing.len() < room))
            })?;
            i.search.scan_from = if scan_done { 0 } else { scanned_to };
            // left: nodes with no stored name and not queued, plus everything queued
            let (missing, dirty): (i64, i64) = i
                .conn
                .query_row(
                    "SELECT (SELECT count(*) FROM nodes) - 1 - (SELECT count(*) FROM search_names)
                            - (SELECT count(*) FROM search_dirty d WHERE EXISTS (SELECT 1 FROM nodes n WHERE n.id = d.node AND n.id != 1)
                               AND NOT EXISTS (SELECT 1 FROM search_names s WHERE s.node = d.node)),
                            (SELECT count(*) FROM search_dirty)",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(dberr)?;
            i.search.missing = missing.max(0) as u64;
            let dirty = dirty as u64;
            Ok(i.search.missing + dirty)
        })
    }

    // ---------------- tags ----------------

    /// Every tag, by name, with how many nodes carry it. None in a volume without the tables.
    pub fn tags(&self) -> Result<Vec<Tag>> {
        self.with(|i| {
            let mut st = match i.conn.prepare(
                "SELECT t.id, t.name, t.color, count(n.node) FROM tags t LEFT JOIN node_tags n ON n.tag = t.id GROUP BY t.id ORDER BY t.norm, t.id",
            ) {
                Ok(s) => s,
                Err(e) if missing_table(&e) => return Ok(vec![]),
                Err(e) => return Err(dberr(e)),
            };
            st.query_map([], |r| Ok(Tag { id: r.get::<_, i64>(0)? as u64, name: r.get(1)?, color: r.get(2)?, count: r.get::<_, i64>(3)? as u64 }))
                .map_err(dberr)?
                .collect::<rusqlite::Result<_>>()
                .map_err(dberr)
        })
    }

    /// A new tag. Names are unique ignoring case, accents and repeated spaces.
    pub fn tag_create(&self, name: &str, color: &str) -> std::result::Result<Tag, TagError> {
        let name = valid_tag_name(name)?;
        let color = valid_color(color)?;
        let mut g = self.lock()?;
        let i = g.as_mut().unwrap();
        tag_op(i, |c| {
            let n: i64 = c.query_row("SELECT count(*) FROM tags", [], |r| r.get(0)).map_err(tag_err)?;
            if n as usize >= MAX_TAGS {
                return Err(TagError::TooMany);
            }
            match c.execute("INSERT INTO tags(name, norm, color, created) VALUES (?, ?, ?, ?)", params![name, tag_norm(name), color, now_ns() / 1_000_000]) {
                Ok(_) => {}
                Err(e) if missing_table(&e) => return Err(TagError::Unsupported),
                Err(e) => return Err(match dberr(e) { Error::Exists => TagError::Exists, o => TagError::Fs(o) }),
            }
            tag_row(c, c.last_insert_rowid() as u64)
        })
    }

    /// Renames and / or recolours a tag (`None` = unchanged).
    pub fn tag_update(&self, id: TagId, name: Option<&str>, color: Option<&str>) -> std::result::Result<Tag, TagError> {
        let name = name.map(valid_tag_name).transpose()?;
        let color = color.map(valid_color).transpose()?;
        let mut g = self.lock()?;
        let i = g.as_mut().unwrap();
        tag_op(i, |c| {
            tag_exists(c, id)?;
            if let Some(n) = name {
                let taken: Option<i64> =
                    c.query_row("SELECT id FROM tags WHERE norm = ? AND id != ?", params![tag_norm(n), id as i64], |r| r.get(0)).optional().map_err(tag_err)?;
                if taken.is_some() {
                    return Err(TagError::Exists);
                }
                c.execute("UPDATE tags SET name = ?, norm = ? WHERE id = ?", params![n, tag_norm(n), id as i64]).map_err(tag_err)?;
            }
            if let Some(col) = color {
                c.execute("UPDATE tags SET color = ? WHERE id = ?", params![col, id as i64]).map_err(tag_err)?;
            }
            tag_row(c, id)
        })
    }

    /// Deletes a tag (and every assignment of it); returns the nodes that carried it.
    pub fn tag_delete(&self, id: TagId) -> std::result::Result<Vec<NodeId>, TagError> {
        let mut g = self.lock()?;
        let i = g.as_mut().unwrap();
        let nodes = tag_op(i, |c| {
            tag_exists(c, id)?;
            let nodes: Vec<NodeId> = c
                .prepare("SELECT node FROM node_tags WHERE tag = ? ORDER BY node")
                .map_err(tag_err)?
                .query_map([id as i64], |r| Ok(r.get::<_, i64>(0)? as u64))
                .map_err(tag_err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(tag_err)?;
            c.execute("DELETE FROM tags WHERE id = ?", [id as i64]).map_err(tag_err)?; // node_tags: ON DELETE CASCADE
            Ok(nodes)
        })?;
        i.search.touch(nodes.iter().copied());
        Ok(nodes)
    }

    /// Adds (`on`) or removes every tag of `tags` to / from every node of `nodes` (missing nodes
    /// and the root are skipped); returns the nodes whose tags changed.
    pub fn tags_set(&self, tags: &[TagId], nodes: &[NodeId], on: bool) -> std::result::Result<Vec<NodeId>, TagError> {
        let mut g = self.lock()?;
        let i = g.as_mut().unwrap();
        let changed = tag_op(i, |c| {
            for &t in tags {
                tag_exists(c, t)?;
            }
            let sql = if on {
                "INSERT OR IGNORE INTO node_tags(node, tag) SELECT id, ?2 FROM nodes WHERE id = ?1 AND id != 1"
            } else {
                "DELETE FROM node_tags WHERE node = ?1 AND tag = ?2"
            };
            let mut st = c.prepare_cached(sql).map_err(tag_err)?;
            let mut changed = std::collections::BTreeSet::new();
            for &n in nodes {
                for &t in tags {
                    if st.execute(params![n as i64, t as i64]).map_err(tag_err)? > 0 {
                        changed.insert(n);
                    }
                }
            }
            Ok(changed.into_iter().collect::<Vec<_>>())
        })?;
        i.search.touch(changed.iter().copied());
        Ok(changed)
    }

    /// The tags of each of `nodes` that has any.
    pub fn node_tags(&self, nodes: &[NodeId]) -> Result<HashMap<NodeId, Vec<TagId>>> {
        self.with(|i| {
            let mut st = match i.conn.prepare_cached("SELECT tag FROM node_tags WHERE node = ? ORDER BY tag") {
                Ok(s) => s,
                Err(e) if missing_table(&e) => return Ok(HashMap::new()),
                Err(e) => return Err(dberr(e)),
            };
            let mut out = HashMap::new();
            for &n in nodes {
                let t: Vec<TagId> =
                    st.query_map([n as i64], |r| Ok(r.get::<_, i64>(0)? as u64)).map_err(dberr)?.collect::<rusqlite::Result<_>>().map_err(dberr)?;
                if !t.is_empty() {
                    out.insert(n, t);
                }
            }
            Ok(out)
        })
    }

    /// The tags of the children of `dir` that have any (one query, for a folder listing).
    pub fn child_tags(&self, dir: NodeId) -> Result<HashMap<NodeId, Vec<TagId>>> {
        self.with(|i| {
            let mut st = match i.conn.prepare_cached(
                "SELECT t.node, t.tag FROM nodes n JOIN node_tags t ON t.node = n.id WHERE n.parent = ? AND n.id != 1 ORDER BY t.node, t.tag",
            ) {
                Ok(s) => s,
                Err(e) if missing_table(&e) => return Ok(HashMap::new()),
                Err(e) => return Err(dberr(e)),
            };
            let mut out: HashMap<NodeId, Vec<TagId>> = HashMap::new();
            for row in st.query_map([dir as i64], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64))).map_err(dberr)? {
                let (n, t) = row.map_err(dberr)?;
                out.entry(n).or_default().push(t);
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding() {
        assert_eq!(fold("Città"), "citta");
        assert_eq!(fold("CITTÀ É Ñandú"), "citta e nandu");
        assert_eq!(fold("Straße Æsir Øre Łódź"), "strasse aesir ore lodz");
        assert_eq!(fold("ÅNGSTRÖM"), "angstrom");
        let (n, map) = fold_mapped("Aß😀é");
        assert_eq!(n, "ass😀e");
        assert_eq!(map[0], (0, 1));
        assert_eq!(map[1], (1, 2));
        assert_eq!(map[2], (1, 2));
        assert_eq!(map[3], (2, 4)); // the emoji: 4 UTF-8 bytes, 2 UTF-16 units
        assert_eq!(*map.last().unwrap(), (4, 5));
    }

    #[test]
    fn query_terms() {
        assert_eq!(terms("  Mare  Estate mare "), vec!["mare", "estate"]);
        assert_eq!(terms(r#"foto "Mare d'inverno" x"#), vec!["foto", "mare d'inverno", "x"]);
        assert_eq!(terms(r#""aperto"#), vec!["aperto"]);
        assert!(terms("   ").is_empty());
    }

    #[test]
    fn kind_groups() {
        assert_eq!(kind_group("Mare.JPG", false), KindGroup::Photos);
        assert_eq!(kind_group("clip.mkv", false), KindGroup::Videos);
        assert_eq!(kind_group("a.flac", false), KindGroup::Audio);
        assert_eq!(kind_group("Bolletta.pdf", false), KindGroup::Documents);
        assert_eq!(kind_group("x.tar.gz", false), KindGroup::Archives);
        assert_eq!(kind_group("main.rs", false), KindGroup::Code);
        assert_eq!(kind_group(".bashrc", false), KindGroup::Other);
        assert_eq!(kind_group("README", false), KindGroup::Other);
        assert_eq!(kind_group("Foto.jpg", true), KindGroup::Folders);
    }

    #[test]
    fn ranks_and_highlights() {
        let t = terms("mare");
        assert_eq!(rank("mare.jpg", &t, "mare"), 0);
        assert_eq!(rank("mare d'inverno.jpg", &t, "mare"), 1);
        assert_eq!(rank("al mare.jpg", &t, "mare"), 2);
        assert_eq!(rank("amarena.jpg", &t, "mare"), 3);
        assert_eq!(highlights("Città di MARE e mare", &terms("mare citta")), vec![[0, 5], [9, 13], [16, 20]]);
        assert_eq!(highlights("Città", &terms("a")), vec![[4, 5]]);
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["foto 10", "foto 2", "foto 02", "foto", "foto 1a"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["foto", "foto 1a", "foto 2", "foto 02", "foto 10"]);
    }

    #[test]
    fn filters() {
        let f = Filters { kinds: vec![KindGroup::Photos], min_size: Some(10), ..Default::default() };
        assert!(f.matches(KindGroup::Photos, 10, 0, &[]));
        assert!(!f.matches(KindGroup::Photos, 9, 0, &[]));
        assert!(!f.matches(KindGroup::Videos, 100, 0, &[]));
        let f = Filters { min_size: Some(0), ..Default::default() };
        assert!(!f.matches(KindGroup::Folders, 0, 0, &[]), "a size bound leaves folders out");
        let f = Filters { modified_after: Some(10), modified_before: Some(20), ..Default::default() };
        assert!(f.matches(KindGroup::Other, 0, 10, &[]) && !f.matches(KindGroup::Other, 0, 20, &[]) && !f.matches(KindGroup::Other, 0, 9, &[]));
        let f = Filters { tags: vec![1, 2], ..Default::default() };
        assert!(f.matches(KindGroup::Other, 0, 0, &[2, 1, 3]) && !f.matches(KindGroup::Other, 0, 0, &[1]));
        let f = Filters { tags: vec![1, 2], tags_any: true, ..Default::default() };
        assert!(f.matches(KindGroup::Other, 0, 0, &[2]) && !f.matches(KindGroup::Other, 0, 0, &[3]));
    }
}
