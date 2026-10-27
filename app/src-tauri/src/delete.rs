use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use nv_fs::{Fs, Kind, NodeId, ROOT};
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeSet, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const BATCH_BYTES: u64 = 4 << 20;
pub const BATCH_ITEMS: u64 = 128;
pub const BATCH_TIME: Duration = Duration::from_millis(750);

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: NodeId,
    pub parent: NodeId,
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

#[derive(Debug, Clone, Default, Serialize)]
pub struct DeleteOutcome {
    pub deleted: u64,
    pub total: u64,
    pub freed_bytes: u64,
    pub errors: Vec<ItemError>,
}

pub struct Progress<'a> {
    pub done_items: u64,
    pub total_items: u64,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub current: &'a str,
    pub committed: bool,
    pub committed_dirs: &'a [NodeId],
}

pub fn plan(fs: &dyn Fs, nodes: &[NodeId]) -> ApiResult<Vec<Entry>> {
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

fn is_full(m: &str) -> bool {
    let m = m.to_ascii_lowercase();
    m.contains("disk is full") || m.contains("no space") || m.contains("os error 28")
}

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

#[allow(unused_assignments)]
pub fn run(fs: &dyn Fs, entries: &[Entry], cancel: &AtomicBool, mut report: impl FnMut(&Progress)) -> (DeleteOutcome, ApiResult<()>) {
    let total_bytes: u64 = entries.iter().map(|e| e.size).sum();
    let mut out = DeleteOutcome { total: entries.len() as u64, ..Default::default() };
    let mut kept = vec![false; entries.len()];
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

pub fn delete_now(fs: &dyn Fs, nodes: &[NodeId]) -> ApiResult<DeleteOutcome> {
    let entries = plan(fs, nodes)?;
    let (out, r) = run(fs, &entries, &AtomicBool::new(false), |_| {});
    r.map(|()| out)
}

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
