use crate::state::{AppState, lock_or_recover};
use nv_fs::{Fs, Kind, Node, NodeId, Result, Stats};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

fn epoch() -> Instant {
    static E: OnceLock<Instant> = OnceLock::new();
    *E.get_or_init(Instant::now)
}

#[derive(Debug)]
pub struct Activity(AtomicU64);

impl Default for Activity {
    fn default() -> Self {
        let a = Activity(AtomicU64::new(0));
        a.touch();
        a
    }
}

impl Activity {
    pub fn touch(&self) {
        self.0.store(Instant::now().saturating_duration_since(epoch()).as_millis() as u64, Ordering::Relaxed);
    }
    pub fn idle(&self, now: Instant) -> Duration {
        now.saturating_duration_since(epoch() + Duration::from_millis(self.0.load(Ordering::Relaxed)))
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AutoLockStatus {
    pub vault: String,
    pub auto_lock_minutes: u32,
    pub remaining_ms: Option<u64>,
}

impl AppState {
    fn busy_vaults(&self) -> HashSet<String> {
        lock_or_recover(&self.jobs).values().map(|j| j.vault.clone()).collect()
    }

    pub fn idle_tick(&self, now: Instant) -> Vec<String> {
        let busy = self.busy_vaults();
        let due: Vec<(String, bool)> = lock_or_recover(&self.vaults)
            .values()
            .filter(|o| {
                let min = o.autolock.auto_lock_minutes;
                if busy.contains(&o.id) {
                    o.activity.touch();
                    return false;
                }
                min > 0 && o.activity.idle(now) >= Duration::from_secs(u64::from(min) * 60)
            })
            .map(|o| (o.id.clone(), o.autolock.locks_all()))
            .collect();
        self.auto_lock(due, "idle")
    }

    pub fn lock_event(&self, reason: &str) -> Vec<String> {
        let due: Vec<(String, bool)> = lock_or_recover(&self.vaults)
            .values()
            .filter(|o| o.autolock.locks_on(reason))
            .map(|o| (o.id.clone(), o.autolock.locks_all()))
            .collect();
        self.auto_lock(due, reason)
    }

    fn auto_lock(&self, due: Vec<(String, bool)>, reason: &str) -> Vec<String> {
        if due.is_empty() {
            return vec![];
        }
        let mut ids: Vec<String> = if due.iter().any(|(_, all)| *all) {
            lock_or_recover(&self.vaults).keys().cloned().collect()
        } else {
            due.into_iter().map(|(id, _)| id).collect()
        };
        ids.sort();
        for id in &ids {
            let _ = self.vault_lock(id, reason);
        }
        if lock_or_recover(&self.vaults).is_empty() {
            self.external.wipe_all();
        }
        ids
    }

    pub fn vault_touch(&self, vault: &str) -> crate::error::ApiResult<()> {
        self.vault(vault).map(|_| ())
    }

    pub fn autolock_status(&self) -> Vec<AutoLockStatus> {
        let busy = self.busy_vaults();
        let now = Instant::now();
        let mut v: Vec<AutoLockStatus> = lock_or_recover(&self.vaults)
            .values()
            .map(|o| {
                let min = o.autolock.auto_lock_minutes;
                let remaining_ms = (min > 0 && !busy.contains(&o.id)).then(|| {
                    Duration::from_secs(u64::from(min) * 60).saturating_sub(o.activity.idle(now)).as_millis() as u64
                });
                AutoLockStatus { vault: o.id.clone(), auto_lock_minutes: min, remaining_ms }
            })
            .collect();
        v.sort_by(|a, b| a.vault.cmp(&b.vault));
        v
    }
}

pub fn spawn_ticker(state: Weak<AppState>) {
    let _ = std::thread::Builder::new().name("nv-autolock".into()).spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let Some(s) = state.upgrade() else { return };
            s.idle_tick(Instant::now());
        }
    });
}

pub struct Touching {
    pub fs: Arc<nv_core::Vault>,
    pub activity: Arc<Activity>,
}

impl Touching {
    fn t<T>(&self, r: T) -> T {
        self.activity.touch();
        r
    }
}

impl Fs for Touching {
    fn getattr(&self, id: NodeId) -> Result<Node> {
        self.fs.getattr(id)
    }
    fn lookup(&self, parent: NodeId, name: &str) -> Result<Node> {
        self.fs.lookup(parent, name)
    }
    fn readdir(&self, dir: NodeId) -> Result<Vec<Node>> {
        self.fs.readdir(dir)
    }
    fn create(&self, parent: NodeId, name: &str, kind: Kind) -> Result<Node> {
        self.t(self.fs.create(parent, name, kind))
    }
    fn read(&self, id: NodeId, offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.t(self.fs.read(id, offset, buf))
    }
    fn write(&self, id: NodeId, offset: u64, data: &[u8]) -> Result<usize> {
        self.t(self.fs.write(id, offset, data))
    }
    fn set_size(&self, id: NodeId, size: u64) -> Result<()> {
        self.t(self.fs.set_size(id, size))
    }
    fn set_mtime(&self, id: NodeId, mtime: i64) -> Result<()> {
        self.fs.set_mtime(id, mtime)
    }
    fn rename(&self, id: NodeId, new_parent: NodeId, new_name: &str) -> Result<()> {
        self.t(self.fs.rename(id, new_parent, new_name))
    }
    fn remove(&self, id: NodeId) -> Result<()> {
        self.t(self.fs.remove(id))
    }
    fn flush(&self) -> Result<()> {
        self.fs.flush()
    }
    fn stats(&self) -> Result<Stats> {
        self.fs.stats()
    }
}
