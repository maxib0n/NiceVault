use crate::state::AppState;
use nv_fs::{Fs, Kind, Node, NodeId, Result, Stats};
use std::collections::BTreeSet;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

pub const QUIET: Duration = Duration::from_millis(150);
pub const MAX_WAIT: Duration = Duration::from_millis(250);

pub type Sink = Box<dyn Fn(Vec<NodeId>) + Send + 'static>;

#[derive(Default)]
struct Pending {
    dirs: BTreeSet<NodeId>,
    first: Option<Instant>,
    last: Option<Instant>,
    closed: bool,
}

#[derive(Default)]
struct Shared {
    st: Mutex<Pending>,
    cv: Condvar,
}

pub struct Notifying {
    inner: Arc<dyn Fs>,
    shared: Arc<Shared>,
}

pub fn for_vault(inner: Arc<dyn Fs>, app: Weak<AppState>, vault: String) -> Arc<dyn Fs> {
    Arc::new(Notifying::new(
        inner,
        Box::new(move |dirs| {
            if let Some(app) = app.upgrade() {
                app.emit("fs-changed", serde_json::json!({ "vault": vault, "dirs": dirs }));
            }
        }),
    ))
}

impl Notifying {
    pub fn new(inner: Arc<dyn Fs>, sink: Sink) -> Self {
        let shared = Arc::new(Shared::default());
        let s = shared.clone();
        std::thread::Builder::new()
            .name("nv-fs-events".into())
            .spawn(move || run(&s, &sink))
            .expect("spawn fs-events thread");
        Self { inner, shared }
    }

    fn mark(&self, dirs: &[NodeId]) {
        let now = Instant::now();
        let mut g = crate::state::lock_or_recover(&self.shared.st);
        g.dirs.extend(dirs);
        g.first.get_or_insert(now);
        g.last = Some(now);
        drop(g);
        self.shared.cv.notify_one();
    }

    fn parent_of(&self, id: NodeId) -> Option<NodeId> {
        self.inner.getattr(id).ok().map(|n| n.parent)
    }
}

impl Drop for Notifying {
    fn drop(&mut self) {
        crate::state::lock_or_recover(&self.shared.st).closed = true;
        self.shared.cv.notify_one();
    }
}

fn run(s: &Shared, sink: &Sink) {
    let mut last_emit: Option<Instant> = None;
    let mut g = crate::state::lock_or_recover(&s.st);
    loop {
        if g.dirs.is_empty() {
            if g.closed {
                return;
            }
            g = s.cv.wait(g).unwrap_or_else(|e| e.into_inner());
            continue;
        }
        let now = Instant::now();
        let (first, last) = (g.first.unwrap_or(now), g.last.unwrap_or(now));
        let mut due = (last + QUIET).min(first + MAX_WAIT);
        if let Some(t) = last_emit {
            due = due.max(t + MAX_WAIT);
        }
        if g.closed || now >= due {
            let dirs: Vec<NodeId> = std::mem::take(&mut g.dirs).into_iter().collect();
            g.first = None;
            g.last = None;
            last_emit = Some(now);
            drop(g);
            sink(dirs);
            g = crate::state::lock_or_recover(&s.st);
            continue;
        }
        g = s.cv.wait_timeout(g, due - now).unwrap_or_else(|e| e.into_inner()).0;
    }
}

impl Fs for Notifying {
    fn getattr(&self, id: NodeId) -> Result<Node> {
        self.inner.getattr(id)
    }
    fn lookup(&self, parent: NodeId, name: &str) -> Result<Node> {
        self.inner.lookup(parent, name)
    }
    fn readdir(&self, dir: NodeId) -> Result<Vec<Node>> {
        self.inner.readdir(dir)
    }
    fn create(&self, parent: NodeId, name: &str, kind: Kind) -> Result<Node> {
        let n = self.inner.create(parent, name, kind)?;
        self.mark(&[parent]);
        Ok(n)
    }
    fn read(&self, id: NodeId, offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.inner.read(id, offset, buf)
    }
    fn write(&self, id: NodeId, offset: u64, data: &[u8]) -> Result<usize> {
        let before = self.inner.getattr(id).ok();
        let n = self.inner.write(id, offset, data)?;
        if let Some(b) = before
            && offset + n as u64 > b.size
        {
            self.mark(&[b.parent]);
        }
        Ok(n)
    }
    fn set_size(&self, id: NodeId, size: u64) -> Result<()> {
        self.inner.set_size(id, size)?;
        if let Some(p) = self.parent_of(id) {
            self.mark(&[p]);
        }
        Ok(())
    }
    fn set_mtime(&self, id: NodeId, mtime: i64) -> Result<()> {
        self.inner.set_mtime(id, mtime)?;
        if let Some(p) = self.parent_of(id) {
            self.mark(&[p]);
        }
        Ok(())
    }
    fn rename(&self, id: NodeId, new_parent: NodeId, new_name: &str) -> Result<()> {
        let old = self.parent_of(id);
        self.inner.rename(id, new_parent, new_name)?;
        match old {
            Some(o) => self.mark(&[o, new_parent]),
            None => self.mark(&[new_parent]),
        }
        Ok(())
    }
    fn remove(&self, id: NodeId) -> Result<()> {
        let old = self.parent_of(id);
        self.inner.remove(id)?;
        if let Some(p) = old {
            self.mark(&[p]);
        }
        Ok(())
    }
    fn flush(&self) -> Result<()> {
        self.inner.flush()
    }
    fn stats(&self) -> Result<Stats> {
        self.inner.stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nv_fs::ROOT;
    use std::sync::mpsc::{Receiver, channel};

    type Ev = (Instant, Vec<NodeId>);

    fn setup() -> (tempfile::TempDir, Arc<nv_core::Vault>, Notifying, Receiver<Ev>) {
        let tmp = tempfile::tempdir().unwrap();
        let pw = secrecy::SecretString::from("x".to_owned());
        let v = Arc::new(nv_core::Vault::create(&tmp.path().join("t.nvault"), &pw, nv_core::KdfPreset::Fast).unwrap());
        let (tx, rx) = channel();
        let n = Notifying::new(v.clone(), Box::new(move |d| tx.send((Instant::now(), d)).unwrap()));
        (tmp, v, n, rx)
    }

    fn drain(rx: &Receiver<Ev>, quiet: Duration) -> Vec<Ev> {
        let mut out = vec![];
        while let Ok(e) = rx.recv_timeout(quiet) {
            out.push(e);
        }
        out
    }

    #[test]
    fn coalesces_a_burst_into_one_event_with_the_right_dirs() {
        let (_t, v, n, rx) = setup();
        let a = v.create(ROOT, "A", Kind::Dir).unwrap();
        let t0 = Instant::now();
        let f = n.create(a.id, "copia.bin", Kind::File).unwrap();
        for i in 0..20u64 {
            n.write(f.id, i * 4096, &[7u8; 4096]).unwrap();
        }
        n.create(ROOT, "B", Kind::Dir).unwrap();
        let evs = drain(&rx, Duration::from_millis(600));
        assert_eq!(evs.len(), 1, "{evs:?}");
        assert_eq!(evs[0].1, vec![ROOT, a.id]);
        assert!(evs[0].0 - t0 < Duration::from_millis(500));
    }

    #[test]
    fn rename_reports_both_parents_remove_its_parent_overwrite_nothing() {
        let (_t, v, n, rx) = setup();
        let a = v.create(ROOT, "A", Kind::Dir).unwrap();
        let b = v.create(ROOT, "B", Kind::Dir).unwrap();
        let f = v.create(a.id, "f.txt", Kind::File).unwrap();
        v.write(f.id, 0, b"ciao").unwrap();

        n.write(f.id, 0, b"CIAO").unwrap();
        n.read(f.id, 0, &mut [0u8; 4]).unwrap();
        assert!(n.create(a.id, "f.txt", Kind::File).is_err());
        assert!(drain(&rx, Duration::from_millis(400)).is_empty());

        n.rename(f.id, b.id, "g.txt").unwrap();
        assert_eq!(drain(&rx, Duration::from_millis(400)).concat_dirs(), vec![a.id, b.id]);

        n.set_size(f.id, 0).unwrap();
        assert_eq!(drain(&rx, Duration::from_millis(400)).concat_dirs(), vec![b.id]);

        n.remove(f.id).unwrap();
        n.remove(a.id).unwrap();
        assert_eq!(drain(&rx, Duration::from_millis(400)).concat_dirs(), vec![ROOT, b.id]);
    }

    #[test]
    fn a_long_copy_emits_steadily_but_at_most_every_max_wait() {
        let (_t, _v, n, rx) = setup();
        let f = n.create(ROOT, "grande.bin", Kind::File).unwrap();
        let t0 = Instant::now();
        let mut off = 0;
        while t0.elapsed() < Duration::from_millis(1200) {
            off += n.write(f.id, off, &[1u8; 1024]).unwrap() as u64;
            std::thread::sleep(Duration::from_millis(10));
        }
        let evs = drain(&rx, Duration::from_millis(600));
        assert!((3..=7).contains(&evs.len()), "{} events", evs.len());
        for w in evs.windows(2) {
            assert!(w[1].0 - w[0].0 >= MAX_WAIT - Duration::from_millis(5), "gap {:?}", w[1].0 - w[0].0);
        }
        assert!(evs.iter().all(|e| e.1 == vec![ROOT]));
    }

    #[test]
    fn drop_flushes_pending_changes() {
        let (_t, _v, n, rx) = setup();
        n.create(ROOT, "x", Kind::Dir).unwrap();
        drop(n);
        assert_eq!(rx.recv_timeout(Duration::from_millis(100)).unwrap().1, vec![ROOT]);
    }

    trait ConcatDirs {
        fn concat_dirs(&self) -> Vec<NodeId>;
    }
    impl ConcatDirs for Vec<Ev> {
        fn concat_dirs(&self) -> Vec<NodeId> {
            let s: BTreeSet<NodeId> = self.iter().flat_map(|e| e.1.iter().copied()).collect();
            s.into_iter().collect()
        }
    }
}
