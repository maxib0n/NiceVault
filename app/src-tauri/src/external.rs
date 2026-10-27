use crate::error::{ApiError, ApiResult};
use crate::jobs::BLOCK;
use crate::state::{AppState, lock_or_recover, random_id};
use nv_core::Vault;
use nv_fs::{Fs, Kind, NodeId};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, SystemTime};

type Stamp = (Option<SystemTime>, u64);

struct Entry {
    vault: String,
    node: NodeId,
    path: PathBuf,
    seen: Stamp,
    pending: Option<Stamp>,
}

const BASE_PREFIX: &str = "nicevault-";
const LOCK_NAME: &str = ".lock";
const UNLOCKED_GRACE: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Inner {
    base: Option<PathBuf>,
    base_lock: Option<File>,
    files: Vec<Entry>,
    pending_wipe: Vec<PathBuf>,
    watcher: bool,
}

#[derive(Default)]
pub struct External {
    inner: Mutex<Inner>,
}

fn stamp(p: &Path) -> Option<Stamp> {
    let m = std::fs::symlink_metadata(p).ok()?;
    Some((m.modified().ok(), m.len()))
}

fn private_dir(p: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(p)
}

pub fn wipe_file(p: &Path) {
    if let Ok(m) = std::fs::symlink_metadata(p)
        && m.is_file()
        && let Ok(mut f) = File::options().write(true).open(p)
    {
        let zeros = vec![0u8; BLOCK];
        let mut left = m.len();
        while left > 0 {
            let k = left.min(BLOCK as u64) as usize;
            if f.write_all(&zeros[..k]).is_err() {
                break;
            }
            left -= k as u64;
        }
        let _ = f.sync_all();
    }
    let _ = std::fs::remove_file(p);
}

fn wipe_tree(dir: &Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) { wipe_tree(&p) } else { wipe_file(&p) }
        }
    }
    let _ = std::fs::remove_dir(dir);
}

fn wipe_entry(e: &Entry) -> Option<PathBuf> {
    wipe_file(&e.path);
    let d = e.path.parent()?;
    wipe_tree(d);
    d.symlink_metadata().is_ok().then(|| d.to_path_buf())
}

fn retry_wipes(inner: &mut Inner) {
    inner.pending_wipe.retain(|d| {
        wipe_tree(d);
        d.symlink_metadata().is_ok()
    });
}

pub fn copies_in_memory() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|d| temp_root() == Path::new(&d))
}

pub fn temp_root() -> PathBuf {
    #[cfg(target_os = "linux")]
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_absolute() && d.is_dir()) {
        return d;
    }
    std::env::temp_dir()
}

fn reimport(v: &Vault, node: NodeId, src: &Path) -> ApiResult<()> {
    if !std::fs::symlink_metadata(src)?.is_file() {
        return Err(ApiError::io("the copy is not a regular file"));
    }
    let mut o = File::options();
    o.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, libc::O_NOFOLLOW);
    let mut f = o.open(src)?;
    let mut buf = vec![0u8; BLOCK];
    let mut off = 0u64;
    loop {
        let k = f.read(&mut buf)?;
        if k == 0 {
            break;
        }
        v.write(node, off, &buf[..k])?;
        off += k as u64;
    }
    v.set_size(node, off)?;
    v.flush()?;
    Ok(())
}

impl External {
    pub fn new() -> Self {
        Self::default()
    }

    fn base(inner: &mut Inner) -> std::io::Result<PathBuf> {
        if let Some(b) = &inner.base
            && b.is_dir()
        {
            return Ok(b.clone());
        }
        let b = temp_root().join(format!("{BASE_PREFIX}{}", random_id()));
        private_dir(&b)?;
        let lock = File::options().write(true).create_new(true).open(b.join(LOCK_NAME))?;
        lock.try_lock().map_err(|e| std::io::Error::other(e.to_string()))?;
        inner.base_lock = Some(lock);
        inner.base = Some(b.clone());
        Ok(b)
    }

    pub fn extract(&self, state: &Arc<AppState>, vault_id: &str, v: &Vault, node: NodeId) -> ApiResult<PathBuf> {
        let n = v.getattr(node)?;
        if n.kind != Kind::File {
            return Err(nv_fs::Error::IsDir.into());
        }
        let mut g = lock_or_recover(&self.inner);
        if let Some(e) = g.files.iter().find(|e| e.vault == vault_id && e.node == node)
            && e.path.is_file()
        {
            return Ok(e.path.clone());
        }
        g.files.retain(|e| !(e.vault == vault_id && e.node == node));
        let dir = Self::base(&mut g)?.join(random_id());
        private_dir(&dir)?;
        let path = crate::jobs::host_join(&dir, &n.name)?;
        let r = (|| -> ApiResult<()> {
            let mut f = File::options().write(true).create_new(true).open(&path)?;
            let mut buf = vec![0u8; BLOCK];
            let mut off = 0u64;
            while off < n.size {
                let k = v.read(node, off, &mut buf)?;
                if k == 0 {
                    break;
                }
                f.write_all(&buf[..k])?;
                off += k as u64;
            }
            f.sync_all()?;
            Ok(())
        })();
        if let Err(e) = r {
            wipe_tree(&dir);
            return Err(e);
        }
        let seen = stamp(&path).ok_or_else(|| ApiError::io("temp file vanished"))?;
        g.files.push(Entry { vault: vault_id.to_owned(), node, path: path.clone(), seen, pending: None });
        if !g.watcher {
            g.watcher = true;
            let weak = Arc::downgrade(state);
            std::thread::Builder::new().name("nv-external".into()).spawn(move || watch(weak)).expect("spawn watcher");
        }
        Ok(path)
    }

    pub fn poll(&self, state: &AppState) {
        let mut g = lock_or_recover(&self.inner);
        let mut gone = Vec::new();
        for (i, e) in g.files.iter_mut().enumerate() {
            let Some(now) = stamp(&e.path) else { continue };
            if now == e.seen {
                e.pending = None;
                continue;
            }
            if e.pending != Some(now) {
                e.pending = Some(now);
                continue;
            }
            let Ok(v) = state.vault(&e.vault) else { continue };
            match reimport(&v, e.node, &e.path) {
                Ok(()) => {
                    e.seen = now;
                    e.pending = None;
                }
                Err(err) if err.code == "not_found" => gone.push(i),
                Err(_) => {}
            }
        }
        for i in gone.into_iter().rev() {
            let e = g.files.remove(i);
            g.pending_wipe.extend(wipe_entry(&e));
        }
        retry_wipes(&mut g);
    }

    pub fn close_vault(&self, vault_id: &str, v: Option<&Vault>) {
        let mut g = lock_or_recover(&self.inner);
        let (mine, rest): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut g.files).into_iter().partition(|e| e.vault == vault_id);
        g.files = rest;
        for e in mine {
            if let Some(v) = v
                && stamp(&e.path).is_some_and(|s| s != e.seen)
            {
                let _ = reimport(v, e.node, &e.path);
            }
            g.pending_wipe.extend(wipe_entry(&e));
        }
    }

    pub fn forget_node(&self, vault_id: &str, node: NodeId) {
        let mut g = lock_or_recover(&self.inner);
        let (hit, rest): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut g.files).into_iter().partition(|e| e.vault == vault_id && e.node == node);
        g.files = rest;
        for e in hit {
            g.pending_wipe.extend(wipe_entry(&e));
        }
    }

    pub fn wipe_all(&self) {
        let mut g = lock_or_recover(&self.inner);
        for e in std::mem::take(&mut g.files) {
            g.pending_wipe.extend(wipe_entry(&e));
        }
        retry_wipes(&mut g);
        drop(g.base_lock.take());
        if let Some(b) = g.base.take() {
            wipe_tree(&b);
        }
    }

    pub fn tracked(&self) -> Vec<PathBuf> {
        lock_or_recover(&self.inner).files.iter().map(|e| e.path.clone()).collect()
    }
}

fn is_our_base_name(name: &str) -> bool {
    name.strip_prefix(BASE_PREFIX).is_some_and(|id| id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn cleanup_stale(temp_root: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(temp_root) else { return 0 };
    let mut wiped = 0;
    for e in rd.flatten() {
        if !e.file_name().to_str().is_some_and(is_our_base_name) {
            continue;
        }
        let dir = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&dir) else { continue };
        if !meta.is_dir() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.uid() != unsafe { libc::geteuid() } {
                continue;
            }
        }
        let lock_path = dir.join(LOCK_NAME);
        match File::options().write(true).open(&lock_path) {
            Ok(f) => match f.try_lock() {
                Ok(()) => drop(f),
                Err(_) => continue,
            },
            Err(_) => {
                let young = meta.modified().ok().and_then(|m| m.elapsed().ok()).is_none_or(|age| age < UNLOCKED_GRACE);
                if young {
                    continue;
                }
            }
        }
        wipe_tree(&dir);
        wiped += 1;
    }
    wiped
}

fn watch(state: Weak<AppState>) {
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let Some(s) = state.upgrade() else { return };
        s.external.poll(&s);
    }
}
