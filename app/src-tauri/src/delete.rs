//! Deleting as a job (`fs_delete`): "616 elementi, 1,1 GB" took long and showed nothing.
//!
//! SQLCipher keeps `secure_delete` on, so every freed page is overwritten — and journaled first:
//! deleting 1 GB in one transaction wrote ~2 GB and needed ~1 GB of journal room (on a nearly full
//! fixed-size volume, past its capacity). Now the selection is expanded into a post-order list
//! (children before their folder) and removed in **batches**, each one a durability point
//! (`Fs::flush`, whose slow part runs outside the store lock): at most [`BATCH_ITEMS`] items,
//! [`BATCH_BYTES`] of file data or [`BATCH_TIME`] per batch. A file bigger than a batch is cut
//! from its end, [`BATCH_BYTES`] per commit, then removed. Between batches the folders that
//! changed are announced (`fs-changed`) so the listing follows.
//!
//! Cancel: takes effect before the next item (a big file being cut is finished first); what was
//! deleted stays deleted. Errors: `NotFound` = already gone (counted as deleted); `Locked`, `Io`
//! and `Corrupt` stop the job (`no_room_to_delete` when the store or the drive is full); anything
//! else is recorded for that item, which stays — and so do the folders holding it.

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use nv_fs::{Fs, Kind, NodeId, ROOT};
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeSet, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// File data freed per commit (also the step a big file is cut by). Freeing it writes about twice
/// as much (secure delete), part of it synced under the store lock (the journal must reach the
/// device before the pages it saved are overwritten): 16 MiB held listings ~1.5 s on a 15 MB/s
/// stick, 4 MiB ~0.4 s (cancel-hang, tests/cancel_hang.rs).
pub const BATCH_BYTES: u64 = 4 << 20;
/// Items removed per commit.
pub const BATCH_ITEMS: u64 = 128;
/// A batch is committed at the latest this long after its first item.
pub const BATCH_TIME: Duration = Duration::from_millis(750);

/// One node to remove, in removal order.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: NodeId,
    pub parent: NodeId,
    /// Index of the parent's entry when the parent is removed too.
    pub parent_idx: Option<usize>,
    pub kind: Kind,
    pub size: u64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ItemError {
    pub node: NodeId,
    pub name: String,
    pub code: String,
    pub message: String,
}

/// `job-done.outcome` of a delete job.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DeleteOutcome {
    /// Items removed and committed (folders included).
    pub deleted: u64,
    pub total: u64,
    /// File data removed and committed.
    pub freed_bytes: u64,
    /// Items that stay (their folders stay too, silently).
    pub errors: Vec<ItemError>,
}

/// What a delete reports as it goes.
pub struct Progress<'a> {
    pub done_items: u64,
    pub total_items: u64,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub current: &'a str,
    /// A commit was just made (a batch, or one step of cutting a big file).
    pub committed: bool,
    /// The folders whose listing that batch changed.
    pub committed_dirs: &'a [NodeId],
}

/// The selection as a post-order list; nodes already gone are skipped, a node selected with one
/// of its folders is taken once.
pub fn plan(fs: &dyn Fs, nodes: &[NodeId]) -> ApiResult<Vec<Entry>> {
    // post-order: a folder right after everything inside it
    fn walk(fs: &dyn Fs, n: nv_fs::Node, out: &mut Vec<Entry>, seen: &mut HashSet<NodeId>) -> ApiResult<()> {
        if !seen.insert(n.id) {
            return Ok(());
        }
        let mut size = n.size;
        if n.kind == Kind::Dir {
            size = 0;
            let kids = match fs.readdir(n.id) {
                Ok(k) => k,
                Err(nv_fs::Error::NotFound) => vec![],
                Err(e) => return Err(e.into()),
            };
            let from = out.len();
            for c in kids {
                walk(fs, c, out, seen)?;
            }
            let me = out.len();
            for e in &mut out[from..] {
                if e.parent == n.id {
                    e.parent_idx = Some(me);
                }
            }
        }
        out.push(Entry { id: n.id, parent: n.parent, parent_idx: None, kind: n.kind, size, name: n.name });
        Ok(())
    }
    if nodes.contains(&ROOT) {
        return Err(ApiError::new("invalid_name", "The root cannot be deleted."));
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for &id in nodes {
        match fs.getattr(id) {
            Ok(n) => walk(fs, n, &mut out, &mut seen)?,
            Err(nv_fs::Error::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(out)
}

/// The store or the drive is full (SQLITE_FULL, ENOSPC).
fn is_full(m: &str) -> bool {
    let m = m.to_ascii_lowercase();
    m.contains("disk is full") || m.contains("no space") || m.contains("os error 28")
}

/// `Some(error)` when `e` must stop the whole delete.
pub fn fatal(e: &nv_fs::Error) -> Option<ApiError> {
    use nv_fs::Error as E;
    match e {
        E::Io(m) if is_full(m) => Some(ApiError::new(
            "no_room_to_delete",
            "There is not enough room to finish deleting: extend the volume (fixed-size) or free space on the drive.",
        )),
        E::Io(m) => Some(ApiError::io(m)),
        E::Locked => Some(ApiError::locked()),
        E::Corrupt(m) => Some(E::Corrupt(m.clone()).into()),
        _ => None,
    }
}

fn fail(e: nv_fs::Error) -> ApiError {
    fatal(&e).unwrap_or_else(|| e.into())
}

/// Removes `entries` (from [`plan`]) in committed batches. Returns what was done, and the error
/// that stopped it (`cancelled` included); what was committed stays deleted either way.
#[allow(unused_assignments)] // the batch macros also reset the batch right before a return
pub fn run(fs: &dyn Fs, entries: &[Entry], cancel: &AtomicBool, mut report: impl FnMut(&Progress)) -> (DeleteOutcome, ApiResult<()>) {
    let total_bytes: u64 = entries.iter().map(|e| e.size).sum();
    let mut out = DeleteOutcome { total: entries.len() as u64, ..Default::default() };
    let mut kept = vec![false; entries.len()];
    // the batch not committed yet
    let mut items = 0u64;
    let mut bytes = 0u64;
    let mut dirs = BTreeSet::new();
    let mut since: Option<Instant> = None;

    macro_rules! progress {
        ($cur:expr, $committed:expr, $dirs:expr) => {
            report(&Progress {
                done_items: out.deleted + items,
                total_items: out.total,
                done_bytes: out.freed_bytes + bytes,
                total_bytes,
                current: $cur,
                committed: $committed,
                committed_dirs: $dirs,
            })
        };
    }
    macro_rules! commit {
        ($cur:expr) => {{
            if items > 0 || !dirs.is_empty() {
                if let Err(e) = fs.flush() {
                    return (out, Err(fail(e)));
                }
                out.deleted += items;
                out.freed_bytes += bytes;
                items = 0;
                bytes = 0;
                since = None;
                let d: Vec<NodeId> = std::mem::take(&mut dirs).into_iter().collect();
                progress!($cur, true, &d);
            }
        }};
    }
    // stop on a fatal error: what the open batch did is kept if it still commits
    macro_rules! stop {
        ($e:expr) => {{
            let err = $e;
            if items > 0 && fs.flush().is_ok() {
                out.deleted += items;
                out.freed_bytes += bytes;
            }
            return (out, Err(err));
        }};
    }

    for (k, e) in entries.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            commit!("");
            return (out, Err(ApiError::cancelled()));
        }
        if kept[k] {
            if let Some(p) = e.parent_idx {
                kept[p] = true;
            }
            continue;
        }
        let mut size = e.size;
        if e.kind == Kind::File && size > BATCH_BYTES {
            commit!(&e.name);
            // cut from the end, one committed step at a time
            while size > BATCH_BYTES {
                let next = (size - 1) / BATCH_BYTES * BATCH_BYTES;
                match fs.set_size(e.id, next).and_then(|()| fs.flush()) {
                    Ok(()) => {}
                    Err(nv_fs::Error::NotFound) => break,
                    Err(err) => stop!(fail(err)),
                }
                out.freed_bytes += size - next;
                size = next;
                progress!(&e.name, true, &[]);
            }
        }
        match fs.remove(e.id) {
            Ok(()) | Err(nv_fs::Error::NotFound) => {
                items += 1;
                bytes += size;
                dirs.insert(e.parent);
                since.get_or_insert_with(Instant::now);
            }
            Err(err) => {
                if let Some(f) = fatal(&err) {
                    stop!(f);
                }
                let a: ApiError = err.into();
                out.errors.push(ItemError { node: e.id, name: e.name.clone(), code: a.code, message: a.message });
                if let Some(p) = e.parent_idx {
                    kept[p] = true;
                }
            }
        }
        if items >= BATCH_ITEMS || bytes >= BATCH_BYTES || since.is_some_and(|t| t.elapsed() >= BATCH_TIME) {
            commit!(&e.name);
        } else {
            progress!(&e.name, false, &[]);
        }
    }
    commit!("");
    (out, Ok(()))
}

/// Plans and runs a delete on the calling thread (no progress, not cancellable).
pub fn delete_now(fs: &dyn Fs, nodes: &[NodeId]) -> ApiResult<DeleteOutcome> {
    let entries = plan(fs, nodes)?;
    let (out, r) = run(fs, &entries, &AtomicBool::new(false), |_| {});
    r.map(|()| out)
}

/// `fs_delete`: the job. Events `job-progress { job, done_bytes, total_bytes, current, done_items,
/// total_items }` (bytes = file data freed), `fs-changed { vault, dirs }` after every committed
/// batch, `job-done { job, error, outcome: DeleteOutcome }`.
pub fn fs_delete(state: &AppState, vault_id: &str, nodes: Vec<NodeId>) -> ApiResult<String> {
    let v = state.vault(vault_id)?;
    if nodes.contains(&ROOT) {
        return Err(ApiError::new("invalid_name", "The root cannot be deleted."));
    }
    for &n in &nodes {
        state.external.forget_node(vault_id, n);
    }
    let vid = vault_id.to_owned();
    Ok(crate::jobs::start(state, vault_id, "nv-delete", move |cx| {
        cx.items = Some((0, 0));
        cx.progress("", true);
        let entries = plan(&*v, &nodes)?;
        cx.total = entries.iter().map(|e| e.size).sum();
        cx.items = Some((0, entries.len() as u64));
        cx.progress("", true);
        let cancel = cx.cancel;
        let (out, r) = run(&*v, &entries, cancel, |p| {
            cx.done = p.done_bytes;
            cx.items = Some((p.done_items, p.total_items));
            if !p.committed_dirs.is_empty() {
                cx.state.emit("fs-changed", json!({ "vault": vid, "dirs": p.committed_dirs }));
            }
            cx.progress(p.current, p.committed);
        });
        cx.outcome = Some(json!(out));
        r
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_is_fatal_with_its_own_code() {
        let e = fatal(&nv_fs::Error::Io("database or disk is full".into())).unwrap();
        assert_eq!(e.code, "no_room_to_delete");
        let e = fatal(&nv_fs::Error::Io("No space left on device (os error 28)".into())).unwrap();
        assert_eq!(e.code, "no_room_to_delete");
        assert_eq!(fatal(&nv_fs::Error::Io("disk I/O error".into())).unwrap().code, "io");
        assert_eq!(fatal(&nv_fs::Error::Locked).unwrap().code, "locked");
        assert!(fatal(&nv_fs::Error::NotEmpty).is_none());
        assert!(fatal(&nv_fs::Error::NotFound).is_none());
    }
}
