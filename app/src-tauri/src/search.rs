//! Search, filters and tags (nv_core search.rs):
//!
//!  - `fs_search { vault, query, filters?, sort?, desc?, limit?, cursor? }` -> SearchResult: one
//!    page of matches with their folder path, highlights and tags. When the volume's stored index
//!    has names to fill (a volume from before search, or many new / renamed nodes), a background
//!    job persists them with `search-index { vault, done, total }` (~10/s, then one with
//!    done = total); results are complete meanwhile (missing names are folded in memory).
//!  - `fs_list { vault, dir, filters? }`: the folder listing (unchanged without filters) with each
//!    node's `tags` (omitted when none).
//!  - `tags_list / tags_create / tags_update / tags_delete / tags_assign / tags_unassign / tags_of`;
//!    every change emits `tags-changed { vault, nodes, dirs }` (nodes whose tags changed and their
//!    folders, like fs-changed; both empty for a new / renamed / recoloured tag).

use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, Job, lock_or_recover, random_id};
use nv_core::{Filters, Hit, SearchQuery, Sort, Tag, TagError, TagId, Vault};
use nv_fs::{Fs, Node, NodeId};
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const DEFAULT_LIMIT: usize = 100;
pub const MAX_LIMIT: usize = 1000;
/// Names persisted per step of the index job (one short hold of the store lock each).
const INDEX_STEP: usize = 2000;
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SearchResult {
    pub hits: Vec<Hit>,
    pub total: usize,
    /// Pass back as `cursor` for the next page; null = no more.
    pub next_cursor: Option<String>,
    /// Names the stored index still has to fill (0 = up to date); informational only.
    pub pending: u64,
}

/// A folder listing entry: the node plus its tags.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ListedNode {
    #[serde(flatten)]
    pub node: Node,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<TagId>,
}

impl From<TagError> for ApiError {
    fn from(e: TagError) -> Self {
        match e {
            TagError::InvalidName => {
                ApiError::new("invalid_name", format!("Invalid tag name (1 to {} characters, no control characters).", nv_core::MAX_TAG_NAME))
            }
            TagError::InvalidColor => ApiError::new("invalid_color", "Unknown tag colour."),
            TagError::Exists => ApiError::new("exists", "A tag with this name already exists."),
            TagError::NotFound => ApiError::not_found("Tag"),
            TagError::TooMany => ApiError::new("too_many_tags", format!("There are already {} tags.", nv_core::MAX_TAGS)),
            TagError::Unsupported => ApiError::unsupported_version(),
            TagError::Fs(e) => e.into(),
        }
    }
}

fn parse_sort(s: Option<&str>) -> ApiResult<Sort> {
    Ok(match s.unwrap_or("relevance") {
        "relevance" => Sort::Relevance,
        "name" => Sort::Name,
        "size" => Sort::Size,
        "mtime" => Sort::Mtime,
        "type" => Sort::Type,
        _ => return Err(ApiError::new("invalid_query", "Unknown sort order.")),
    })
}

/// Cursors are opaque to the UI: "o<offset>".
fn parse_cursor(c: Option<&str>) -> ApiResult<usize> {
    match c {
        None | Some("") => Ok(0),
        Some(c) => c.strip_prefix('o').and_then(|n| n.parse().ok()).ok_or_else(|| ApiError::new("invalid_query", "Invalid cursor.")),
    }
}

/// Volumes whose index job is running (one per volume).
fn indexing() -> &'static Mutex<HashSet<String>> {
    static S: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    S.get_or_init(Mutex::default)
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn fs_search(
        &self,
        vault: &str,
        query: &str,
        filters: Option<Filters>,
        sort: Option<&str>,
        desc: bool,
        limit: Option<usize>,
        cursor: Option<&str>,
    ) -> ApiResult<SearchResult> {
        let v = self.vault(vault)?;
        if query.len() > 1024 {
            return Err(ApiError::new("invalid_query", "The search text is too long."));
        }
        let q = SearchQuery {
            text: query.to_owned(),
            filters: filters.unwrap_or_default(),
            sort: parse_sort(sort)?,
            desc,
            offset: parse_cursor(cursor)?,
            limit: limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
        };
        let page = v.search(&q)?;
        if page.pending > 0 {
            self.start_index_job(vault, v.clone(), page.pending);
        }
        Ok(SearchResult { hits: page.hits, total: page.total, next_cursor: page.next_offset.map(|o| format!("o{o}")), pending: page.pending })
    }

    /// Persists the volume's missing / queued names in the background (one job per volume,
    /// cancelled by lock). Crash-safe: every step is atomic and what is left is recomputed.
    fn start_index_job(&self, vault: &str, v: Arc<Vault>, pending: u64) {
        if !lock_or_recover(indexing()).insert(vault.to_owned()) {
            return;
        }
        let job = random_id();
        let cancel = Arc::new(AtomicBool::new(false));
        lock_or_recover(&self.jobs).insert(job.clone(), Job { vault: vault.to_owned(), cancel: cancel.clone() });
        let st = self.arc();
        let key = vault.to_owned();
        let vault = vault.to_owned();
        let spawned = std::thread::Builder::new().name("nv-search-index".into()).spawn(move || {
            let mut total = pending;
            let mut left = pending;
            let mut last = Instant::now();
            let mut stuck = 0;
            st.emit("search-index", json!({ "vault": vault, "done": 0, "total": total }));
            while left > 0 && !cancel.load(Ordering::Relaxed) {
                let Ok(now) = v.search_index_step(INDEX_STEP) else { break };
                total = total.max(now);
                stuck = if now >= left { stuck + 1 } else { 0 };
                left = now;
                if stuck >= 3 {
                    break; // writes outpace the job (a big copy): the next search starts it again
                }
                if last.elapsed() >= PROGRESS_EVERY {
                    last = Instant::now();
                    st.emit("search-index", json!({ "vault": vault, "done": total - left, "total": total }));
                }
            }
            let _ = v.flush();
            st.emit("search-index", json!({ "vault": vault, "done": total - left, "total": total }));
            lock_or_recover(&st.jobs).remove(&job);
            lock_or_recover(indexing()).remove(&vault);
        });
        if spawned.is_err() {
            lock_or_recover(indexing()).remove(&key);
        }
    }

    /// `fs_list` with tags, optionally filtered (`within` is ignored here).
    pub fn fs_list_tagged(&self, vault: &str, dir: NodeId, filters: Option<Filters>) -> ApiResult<Vec<ListedNode>> {
        let nodes = self.fs_list(vault, dir)?;
        let v = self.vault(vault)?;
        let mut tags = v.child_tags(dir)?;
        Ok(nodes
            .into_iter()
            .map(|n| {
                let t = tags.remove(&n.id).unwrap_or_default();
                ListedNode { node: n, tags: t }
            })
            .filter(|l| filters.as_ref().is_none_or(|f| f.matches_node(&l.node, &l.tags)))
            .collect())
    }

    // ---------------- tags ----------------

    fn tags_changed(&self, vault: &str, v: &Vault, nodes: &[NodeId]) {
        let dirs: BTreeSet<NodeId> = nodes.iter().filter_map(|&n| v.getattr(n).ok().map(|a| a.parent)).collect();
        self.emit("tags-changed", json!({ "vault": vault, "nodes": nodes, "dirs": dirs }));
    }

    pub fn tags_list(&self, vault: &str) -> ApiResult<Vec<Tag>> {
        Ok(self.vault(vault)?.tags()?)
    }

    pub fn tags_create(&self, vault: &str, name: &str, color: &str) -> ApiResult<Tag> {
        let v = self.vault(vault)?;
        let t = v.tag_create(name, color)?;
        v.flush()?;
        self.tags_changed(vault, &v, &[]);
        Ok(t)
    }

    pub fn tags_update(&self, vault: &str, tag: TagId, name: Option<&str>, color: Option<&str>) -> ApiResult<Tag> {
        let v = self.vault(vault)?;
        let t = v.tag_update(tag, name, color)?;
        v.flush()?;
        self.tags_changed(vault, &v, &[]);
        Ok(t)
    }

    pub fn tags_delete(&self, vault: &str, tag: TagId) -> ApiResult<()> {
        let v = self.vault(vault)?;
        let nodes = v.tag_delete(tag)?;
        v.flush()?;
        self.tags_changed(vault, &v, &nodes);
        Ok(())
    }

    /// Adds (`on`) or removes `tags` on `nodes`; returns every tag with its new count.
    pub fn tags_set(&self, vault: &str, tags: &[TagId], nodes: &[NodeId], on: bool) -> ApiResult<Vec<Tag>> {
        if nodes.len() > 1_000_000 || tags.len() > nv_core::MAX_TAGS {
            return Err(ApiError::new("invalid_query", "Too many items."));
        }
        let v = self.vault(vault)?;
        let changed = v.tags_set(tags, nodes, on)?;
        v.flush()?;
        if !changed.is_empty() {
            self.tags_changed(vault, &v, &changed);
        }
        Ok(v.tags()?)
    }

    pub fn tags_of(&self, vault: &str, nodes: &[NodeId]) -> ApiResult<HashMap<NodeId, Vec<TagId>>> {
        Ok(self.vault(vault)?.node_tags(nodes)?)
    }
}

pub mod cmd {
    use super::{ListedNode, SearchResult};
    use crate::error::{ApiError, ApiResult};
    use crate::state::AppState;
    use nv_core::{Filters, Tag, TagId};
    use nv_fs::NodeId;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tauri::State;

    type St<'a> = State<'a, Arc<AppState>>;

    async fn blocking<T: Send + 'static>(st: &St<'_>, f: impl FnOnce(&AppState) -> ApiResult<T> + Send + 'static) -> ApiResult<T> {
        let s = st.inner().clone();
        tauri::async_runtime::spawn_blocking(move || f(&s)).await.map_err(|e| ApiError::new("error", format!("Internal error: {e}")))?
    }

    #[tauri::command(rename_all = "snake_case")]
    #[allow(clippy::too_many_arguments)]
    pub async fn fs_search(
        st: St<'_>,
        vault: String,
        query: String,
        filters: Option<Filters>,
        sort: Option<String>,
        desc: Option<bool>,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> ApiResult<SearchResult> {
        blocking(&st, move |s| s.fs_search(&vault, &query, filters, sort.as_deref(), desc.unwrap_or(false), limit, cursor.as_deref())).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn fs_list(st: St<'_>, vault: String, dir: NodeId, filters: Option<Filters>) -> ApiResult<Vec<ListedNode>> {
        blocking(&st, move |s| s.fs_list_tagged(&vault, dir, filters)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn tags_list(st: St<'_>, vault: String) -> ApiResult<Vec<Tag>> {
        blocking(&st, move |s| s.tags_list(&vault)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn tags_create(st: St<'_>, vault: String, name: String, color: String) -> ApiResult<Tag> {
        blocking(&st, move |s| s.tags_create(&vault, &name, &color)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn tags_update(st: St<'_>, vault: String, tag: TagId, name: Option<String>, color: Option<String>) -> ApiResult<Tag> {
        blocking(&st, move |s| s.tags_update(&vault, tag, name.as_deref(), color.as_deref())).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn tags_delete(st: St<'_>, vault: String, tag: TagId) -> ApiResult<()> {
        blocking(&st, move |s| s.tags_delete(&vault, tag)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn tags_assign(st: St<'_>, vault: String, tags: Vec<TagId>, nodes: Vec<NodeId>) -> ApiResult<Vec<Tag>> {
        blocking(&st, move |s| s.tags_set(&vault, &tags, &nodes, true)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn tags_unassign(st: St<'_>, vault: String, tags: Vec<TagId>, nodes: Vec<NodeId>) -> ApiResult<Vec<Tag>> {
        blocking(&st, move |s| s.tags_set(&vault, &tags, &nodes, false)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn tags_of(st: St<'_>, vault: String, nodes: Vec<NodeId>) -> ApiResult<HashMap<NodeId, Vec<TagId>>> {
        blocking(&st, move |s| s.tags_of(&vault, &nodes)).await
    }
}
