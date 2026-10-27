use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, Job, lock_or_recover, random_id};
use nv_core::Vault;
use nv_fs::{Fs, Kind, NodeId};
use serde_json::json;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const BLOCK: usize = 1 << 20;
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

pub(crate) struct Ctx<'a> {
    pub(crate) state: &'a AppState,
    job: &'a str,
    pub(crate) cancel: &'a AtomicBool,
    pub(crate) done: u64,
    pub(crate) total: u64,
    pub(crate) items: Option<(u64, u64)>,
    pub(crate) outcome: Option<serde_json::Value>,
    last: Instant,
    buf: Vec<u8>,
}

impl Ctx<'_> {
    fn check(&self) -> ApiResult<()> {
        if self.cancel.load(Ordering::Relaxed) { Err(ApiError::cancelled()) } else { Ok(()) }
    }
    pub(crate) fn progress(&mut self, current: &str, force: bool) {
        if force || self.last.elapsed() >= PROGRESS_EVERY {
            self.last = Instant::now();
            let mut p = json!({ "job": self.job, "done_bytes": self.done, "total_bytes": self.total, "current": current });
            if let Some((d, t)) = self.items {
                p["done_items"] = d.into();
                p["total_items"] = t.into();
            }
            self.state.emit("job-progress", p);
        }
    }
}

pub(crate) fn start(state: &AppState, vault_id: &str, name: &str, work: impl FnOnce(&mut Ctx) -> ApiResult<()> + Send + 'static) -> String {
    let job = random_id();
    let cancel = Arc::new(AtomicBool::new(false));
    lock_or_recover(&state.jobs).insert(job.clone(), Job { vault: vault_id.to_owned(), cancel: cancel.clone() });
    let st = state.arc();
    let id = job.clone();
    let buf = if name.starts_with("nv-delete") { 0 } else { BLOCK };
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let mut ctx = Ctx { state: &st, job: &id, cancel: &cancel, done: 0, total: 0, items: None, outcome: None, last: Instant::now(), buf: vec![0; buf] };
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&mut ctx)))
                .unwrap_or_else(|_| Err(ApiError::new("error", "Internal error: the job stopped unexpectedly.")));
            if r.is_ok() {
                ctx.progress("", true);
            }
            lock_or_recover(&st.jobs).remove(&id);
            let mut done = json!({ "job": id, "error": r.err() });
            if let Some(o) = ctx.outcome.take() {
                done["outcome"] = o;
            }
            st.emit("job-done", done);
        })
        .expect("spawn job thread");
    job
}

fn ns(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64)
}

fn numbered(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({n}){}", &name[..i], &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

fn unique_in_vault(v: &Vault, parent: NodeId, name: &str) -> ApiResult<String> {
    let mut cand = name.to_owned();
    for n in 2.. {
        match v.lookup(parent, &cand) {
            Err(nv_fs::Error::NotFound) => return Ok(cand),
            Err(e) => return Err(e.into()),
            Ok(_) => cand = numbered(name, n),
        }
    }
    unreachable!()
}

pub fn host_join(dir: &Path, name: &str) -> ApiResult<PathBuf> {
    let c = nv_fs::host_name(name);
    let mut it = Path::new(&c).components();
    match (it.next(), it.next()) {
        (Some(std::path::Component::Normal(_)), None) => Ok(dir.join(c)),
        _ => Err(ApiError::new("invalid_name", format!("Invalid name: {name}"))),
    }
}

fn unique_on_host(dir: &Path, name: &str) -> ApiResult<PathBuf> {
    let mut cand = host_join(dir, name)?;
    let mut n = 2;
    while cand.symlink_metadata().is_ok() {
        cand = host_join(dir, &numbered(name, n))?;
        n += 1;
    }
    Ok(cand)
}

fn host_size(p: &Path) -> u64 {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => std::fs::read_dir(p).map_or(0, |rd| rd.flatten().map(|e| host_size(&e.path())).sum()),
        Ok(m) if m.is_symlink() => std::fs::metadata(p).ok().filter(|m| m.is_file()).map_or(0, |m| m.len()),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

pub fn import_size(paths: &[String]) -> u64 {
    paths.iter().map(|p| host_size(Path::new(p))).sum()
}

fn name_of(p: &Path) -> ApiResult<String> {
    p.file_name()
        .map(|s| nv_fs::host_name(&s.to_string_lossy()))
        .filter(|n| nv_fs::valid_name(n))
        .ok_or_else(|| ApiError::new("invalid_name", format!("Invalid name: {}", p.display())))
}

fn import_one(cx: &mut Ctx, v: &Vault, parent: NodeId, src: &Path, name: &str) -> ApiResult<()> {
    cx.check()?;
    let lmd = std::fs::symlink_metadata(src)?;
    if lmd.is_dir() {
        let d = v.create(parent, name, Kind::Dir)?;
        let mut entries: Vec<_> = std::fs::read_dir(src)?.flatten().map(|e| e.path()).collect();
        entries.sort();
        for e in entries {
            let n = name_of(&e)?;
            import_one(cx, v, d.id, &e, &n)?;
        }
        if let Ok(t) = lmd.modified() {
            let _ = v.set_mtime(d.id, ns(t));
        }
        record_import(v, d.id, src, &lmd);
        return Ok(());
    }
    let md = std::fs::metadata(src)?;
    if !md.is_file() {
        return Ok(());
    }
    let mut f = File::open(src)?;
    let node = v.create(parent, name, Kind::File)?;
    let r = (|| {
        let mut off = 0u64;
        loop {
            cx.check()?;
            let n = read_full(&mut f, &mut cx.buf)?;
            if n == 0 {
                break;
            }
            v.write(node.id, off, &cx.buf[..n])?;
            off += n as u64;
            cx.done += n as u64;
            cx.progress(name, false);
        }
        if let Ok(t) = md.modified() {
            v.set_mtime(node.id, ns(t))?;
        }
        record_import(v, node.id, src, &md);
        Ok(())
    })();
    if r.is_err() {
        let _ = v.discard(node.id);
    }
    r
}

fn record_import(v: &Vault, id: NodeId, src: &Path, md: &std::fs::Metadata) {
    let born = md.created().ok().map(ns).filter(|&t| t > 0);
    let origin = std::path::absolute(src).unwrap_or_else(|_| src.to_path_buf());
    let _ = v.set_import_meta(id, born, ns(SystemTime::now()), &origin.to_string_lossy());
}

fn read_full(f: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

pub fn fs_import(state: &AppState, vault_id: &str, parent: NodeId, host_paths: Vec<String>) -> ApiResult<String> {
    fs_import_then(state, vault_id, parent, host_paths, || ())
}

pub fn fs_import_then(state: &AppState, vault_id: &str, parent: NodeId, host_paths: Vec<String>, after: impl FnOnce() + Send + 'static) -> ApiResult<String> {
    let v = state.vault(vault_id)?;
    if v.getattr(parent)?.kind != Kind::Dir {
        return Err(nv_fs::Error::NotDir.into());
    }
    let paths: Vec<PathBuf> = host_paths.into_iter().map(PathBuf::from).collect();
    Ok(start(state, vault_id, "nv-import", move |cx| {
        cx.total = paths.iter().map(|p| host_size(p)).sum();
        cx.progress("", true);
        let r = (|| {
            for p in &paths {
                let name = unique_in_vault(&v, parent, &name_of(p)?)?;
                import_one(cx, &v, parent, p, &name)?;
            }
            Ok(())
        })();
        let f = v.flush().map_err(ApiError::from);
        after();
        r.and(f)
    }))
}

pub const STREAM_FLUSH_EVERY: usize = 16;

pub struct Sink<'a, 'b> {
    cx: &'a mut Ctx<'b>,
    v: &'a Vault,
    parent: NodeId,
    since_flush: usize,
}

impl Sink<'_, '_> {
    pub fn put(&mut self, src: &Path) -> ApiResult<()> {
        self.cx.total += host_size(src);
        let name = unique_in_vault(self.v, self.parent, &name_of(src)?)?;
        import_one(self.cx, self.v, self.parent, src, &name)?;
        self.since_flush += 1;
        if self.since_flush >= STREAM_FLUSH_EVERY {
            self.since_flush = 0;
            self.v.flush()?;
        }
        Ok(())
    }
}

impl<'b> Sink<'_, 'b> {
    pub fn cancel_flag(&self) -> &'b AtomicBool {
        self.cx.cancel
    }
    pub fn job(&self) -> &'b str {
        self.cx.job
    }
}

pub fn import_stream(state: &AppState, vault_id: &str, parent: NodeId, feed: impl FnOnce(&mut Sink) -> ApiResult<()> + Send + 'static) -> ApiResult<String> {
    let v = state.vault(vault_id)?;
    if v.getattr(parent)?.kind != Kind::Dir {
        return Err(nv_fs::Error::NotDir.into());
    }
    Ok(start(state, vault_id, "nv-import-stream", move |cx| {
        cx.progress("", true);
        let r = {
            let mut sink = Sink { cx, v: &v, parent, since_flush: 0 };
            feed(&mut sink)
        };
        let f = v.flush().map_err(ApiError::from);
        r.and(f)
    }))
}

fn vault_size(v: &Vault, id: NodeId) -> ApiResult<u64> {
    let n = v.getattr(id)?;
    if n.kind == Kind::File {
        return Ok(n.size);
    }
    let mut s = 0;
    for c in v.readdir(id)? {
        s += vault_size(v, c.id)?;
    }
    Ok(s)
}

fn set_host_mtime(p: &Path, mtime_ns: i64) {
    if mtime_ns <= 0 {
        return;
    }
    let t = UNIX_EPOCH + Duration::from_nanos(mtime_ns as u64);
    if let Ok(f) = File::options().write(true).open(p) {
        let _ = f.set_modified(t);
    }
}

fn export_one(cx: &mut Ctx, v: &Vault, id: NodeId, dest: &Path) -> ApiResult<()> {
    cx.check()?;
    let n = v.getattr(id)?;
    if n.kind == Kind::Dir {
        std::fs::create_dir(dest)?;
        for c in v.readdir(id)? {
            export_one(cx, v, c.id, &unique_on_host(dest, &c.name)?)?;
        }
        return Ok(());
    }
    let mut f = File::options().write(true).create_new(true).open(dest)?;
    let r = (|| {
        let mut off = 0u64;
        while off < n.size {
            cx.check()?;
            let want = BLOCK.min((n.size - off) as usize);
            let k = v.read(id, off, &mut cx.buf[..want])?;
            if k == 0 {
                break;
            }
            f.write_all(&cx.buf[..k])?;
            off += k as u64;
            cx.done += k as u64;
            cx.progress(&n.name, false);
        }
        f.flush()?;
        Ok(())
    })();
    drop(f);
    match r {
        Ok(()) => {
            set_host_mtime(dest, n.mtime);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(dest);
            Err(e)
        }
    }
}

pub fn fs_export(state: &AppState, vault_id: &str, nodes: Vec<NodeId>, dest_dir: &str) -> ApiResult<String> {
    let v = state.vault(vault_id)?;
    let dest = PathBuf::from(dest_dir);
    if !dest.is_dir() {
        return Err(ApiError::new("not_dir", "The destination is not a folder."));
    }
    for &n in &nodes {
        v.getattr(n)?;
    }
    Ok(start(state, vault_id, "nv-export", move |cx| {
        let mut total = 0;
        for &n in &nodes {
            total += vault_size(&v, n)?;
        }
        cx.total = total;
        cx.progress("", true);
        for &n in &nodes {
            let node = v.getattr(n)?;
            let name = if node.id == nv_fs::ROOT { "Volume".to_owned() } else { node.name.clone() };
            export_one(cx, &v, n, &unique_on_host(&dest, &name)?)?;
        }
        Ok(())
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn import_size_sums_files_and_folders() {
        let d = std::env::temp_dir().join(format!("nv-impsize-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a"), [0u8; 1000]).unwrap();
        std::fs::write(d.join("sub/b"), [0u8; 234]).unwrap();
        let p = |x: &str| d.join(x).to_string_lossy().into_owned();
        assert_eq!(super::import_size(&[p("a"), p("sub")]), 1234);
        assert_eq!(super::import_size(&[p("gone"), "content://x/1".into()]), 0);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn numbered_names() {
        assert_eq!(super::numbered("a.txt", 2), "a (2).txt");
        assert_eq!(super::numbered("Foto", 3), "Foto (3)");
        assert_eq!(super::numbered(".bashrc", 2), ".bashrc (2)");
    }
}
