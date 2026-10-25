//! In-memory `nv_fs::Fs` used by the nv-mount tests (shared by unit and integration tests).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use nv_fs::{Error, Fs, Kind, Node, NodeId, ROOT, Result, Stats, valid_name};

struct Entry {
    node: Node,
    data: Vec<u8>,
}

pub struct MemFs {
    state: Mutex<(BTreeMap<NodeId, Entry>, NodeId)>,
    pub flushes: AtomicU64,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

impl Default for MemFs {
    fn default() -> Self {
        Self::new()
    }
}

impl MemFs {
    pub fn new() -> Self {
        let mut m = BTreeMap::new();
        m.insert(
            ROOT,
            Entry {
                node: Node { id: ROOT, parent: ROOT, name: String::new(), kind: Kind::Dir, size: 0, mtime: now() },
                data: Vec::new(),
            },
        );
        Self { state: Mutex::new((m, ROOT + 1)), flushes: AtomicU64::new(0) }
    }

    pub fn flush_count(&self) -> u64 {
        self.flushes.load(Ordering::SeqCst)
    }
}

fn child(m: &BTreeMap<NodeId, Entry>, parent: NodeId, name: &str) -> Option<NodeId> {
    m.values()
        .find(|e| e.node.parent == parent && e.node.id != ROOT && e.node.name == name)
        .map(|e| e.node.id)
}

fn has_children(m: &BTreeMap<NodeId, Entry>, id: NodeId) -> bool {
    m.values().any(|e| e.node.parent == id && e.node.id != ROOT)
}

fn dir(m: &BTreeMap<NodeId, Entry>, id: NodeId) -> Result<()> {
    match m.get(&id) {
        None => Err(Error::NotFound),
        Some(e) if e.node.kind != Kind::Dir => Err(Error::NotDir),
        Some(_) => Ok(()),
    }
}

impl Fs for MemFs {
    fn getattr(&self, id: NodeId) -> Result<Node> {
        let s = self.state.lock().unwrap();
        s.0.get(&id).map(|e| e.node.clone()).ok_or(Error::NotFound)
    }

    fn lookup(&self, parent: NodeId, name: &str) -> Result<Node> {
        let s = self.state.lock().unwrap();
        dir(&s.0, parent)?;
        child(&s.0, parent, name).map(|id| s.0[&id].node.clone()).ok_or(Error::NotFound)
    }

    fn readdir(&self, d: NodeId) -> Result<Vec<Node>> {
        let s = self.state.lock().unwrap();
        dir(&s.0, d)?;
        Ok(s.0.values().filter(|e| e.node.parent == d && e.node.id != ROOT).map(|e| e.node.clone()).collect())
    }

    fn create(&self, parent: NodeId, name: &str, kind: Kind) -> Result<Node> {
        if !valid_name(name) {
            return Err(Error::InvalidName);
        }
        let mut s = self.state.lock().unwrap();
        dir(&s.0, parent)?;
        if child(&s.0, parent, name).is_some() {
            return Err(Error::Exists);
        }
        let id = s.1;
        s.1 += 1;
        let t = now();
        let node = Node { id, parent, name: name.to_string(), kind, size: 0, mtime: t };
        s.0.insert(id, Entry { node: node.clone(), data: Vec::new() });
        s.0.get_mut(&parent).unwrap().node.mtime = t;
        Ok(node)
    }

    fn read(&self, id: NodeId, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let s = self.state.lock().unwrap();
        let e = s.0.get(&id).ok_or(Error::NotFound)?;
        if e.node.kind == Kind::Dir {
            return Err(Error::IsDir);
        }
        let len = e.data.len() as u64;
        if offset >= len {
            return Ok(0);
        }
        let n = ((len - offset) as usize).min(buf.len());
        buf[..n].copy_from_slice(&e.data[offset as usize..offset as usize + n]);
        Ok(n)
    }

    fn write(&self, id: NodeId, offset: u64, data: &[u8]) -> Result<usize> {
        let mut s = self.state.lock().unwrap();
        let e = s.0.get_mut(&id).ok_or(Error::NotFound)?;
        if e.node.kind == Kind::Dir {
            return Err(Error::IsDir);
        }
        let end = offset as usize + data.len();
        if e.data.len() < end {
            e.data.resize(end, 0);
        }
        e.data[offset as usize..end].copy_from_slice(data);
        e.node.size = e.data.len() as u64;
        e.node.mtime = now();
        Ok(data.len())
    }

    fn set_size(&self, id: NodeId, size: u64) -> Result<()> {
        let mut s = self.state.lock().unwrap();
        let e = s.0.get_mut(&id).ok_or(Error::NotFound)?;
        if e.node.kind == Kind::Dir {
            return Err(Error::IsDir);
        }
        e.data.resize(size as usize, 0);
        e.node.size = size;
        e.node.mtime = now();
        Ok(())
    }

    fn set_mtime(&self, id: NodeId, mtime: i64) -> Result<()> {
        let mut s = self.state.lock().unwrap();
        s.0.get_mut(&id).ok_or(Error::NotFound)?.node.mtime = mtime;
        Ok(())
    }

    fn rename(&self, id: NodeId, new_parent: NodeId, new_name: &str) -> Result<()> {
        if !valid_name(new_name) {
            return Err(Error::InvalidName);
        }
        let mut s = self.state.lock().unwrap();
        if id == ROOT {
            return Err(Error::InvalidName);
        }
        dir(&s.0, new_parent)?;
        let kind = s.0.get(&id).ok_or(Error::NotFound)?.node.kind;
        // No moving a dir into itself or a descendant.
        let mut p = new_parent;
        loop {
            if p == id {
                return Err(Error::InvalidName);
            }
            if p == ROOT {
                break;
            }
            p = s.0[&p].node.parent;
        }
        if let Some(t) = child(&s.0, new_parent, new_name) {
            if t == id {
                return Ok(());
            }
            let tk = s.0[&t].node.kind;
            match (kind, tk) {
                (Kind::File, Kind::Dir) => return Err(Error::IsDir),
                (Kind::Dir, Kind::File) => return Err(Error::NotDir),
                (Kind::Dir, Kind::Dir) if has_children(&s.0, t) => return Err(Error::Exists),
                _ => {}
            }
            s.0.remove(&t);
        }
        let t = now();
        let old_parent = s.0[&id].node.parent;
        let e = s.0.get_mut(&id).unwrap();
        e.node.parent = new_parent;
        e.node.name = new_name.to_string();
        s.0.get_mut(&old_parent).unwrap().node.mtime = t;
        s.0.get_mut(&new_parent).unwrap().node.mtime = t;
        Ok(())
    }

    fn remove(&self, id: NodeId) -> Result<()> {
        let mut s = self.state.lock().unwrap();
        if id == ROOT {
            return Err(Error::InvalidName);
        }
        if !s.0.contains_key(&id) {
            return Err(Error::NotFound);
        }
        if has_children(&s.0, id) {
            return Err(Error::NotEmpty);
        }
        let parent = s.0.remove(&id).unwrap().node.parent;
        if let Some(p) = s.0.get_mut(&parent) {
            p.node.mtime = now();
        }
        Ok(())
    }

    fn flush(&self) -> Result<()> {
        self.flushes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn stats(&self) -> Result<Stats> {
        let s = self.state.lock().unwrap();
        let used: u64 = s.0.values().map(|e| e.node.size).sum();
        let files = s.0.values().filter(|e| e.node.kind == Kind::File).count() as u64;
        Ok(Stats { total_bytes: 64 << 30, free_bytes: (64 << 30) - used, used_bytes: used, files })
    }
}
