use serde::Serialize;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

pub const CONCURRENCY: usize = 4;
pub const STAGED_MAX: usize = 4;
pub const ATTEMPTS: u32 = 4;
pub const SPACE_FLOOR: u64 = 512 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Offline,
    Icloud,
    Stalled,
    Unsupported,
    NoSpace,
    Cancelled,
    Expired,
    Other,
}

impl Reason {
    pub fn from_code(code: &str) -> Reason {
        match code {
            "offline" => Reason::Offline,
            "icloud" => Reason::Icloud,
            "stalled" => Reason::Stalled,
            "unsupported" => Reason::Unsupported,
            "no_space" => Reason::NoSpace,
            "cancelled" => Reason::Cancelled,
            "expired" => Reason::Expired,
            _ => Reason::Other,
        }
    }

    pub fn transient(self) -> bool {
        matches!(self, Reason::Offline | Reason::Icloud | Reason::Stalled | Reason::Other)
    }
}

pub fn backoff(n: u32) -> Duration {
    Duration::from_millis(match n {
        0 | 1 => 1_000,
        2 => 3_000,
        _ => 8_000,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fail {
    pub reason: Reason,
    pub message: String,
}

impl Fail {
    pub fn new(reason: Reason, message: impl Into<String>) -> Fail {
        Fail { reason, message: message.into() }
    }
}

#[derive(Debug)]
pub struct Fetched {
    pub index: usize,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Failure {
    pub index: usize,
    pub name: String,
    pub reason: Reason,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub total: usize,
    pub imported: usize,
    pub failed: usize,
    pub fetching: Vec<usize>,
}

pub struct Summary {
    pub imported: Vec<usize>,
    pub failures: Vec<Failure>,
}

pub struct ImportFail {
    pub fail: Fail,
    pub stop: bool,
}

pub struct Hooks<'a> {
    pub fetch: &'a (dyn Fn(usize, u32) -> Result<PathBuf, Fail> + Sync),
    pub import: &'a mut dyn FnMut(&Fetched) -> Result<(), ImportFail>,
    pub wipe: &'a (dyn Fn(&Fetched) + Sync),
    pub free_space: &'a (dyn Fn() -> Option<u64> + Sync),
    pub sleep: &'a (dyn Fn(Duration) + Sync),
    pub name: &'a (dyn Fn(usize) -> String + Sync),
}

pub fn run(indices: &[usize], h: Hooks, cancel: &AtomicBool, status: &Mutex<Status>) -> Summary {
    let upd = |f: &mut dyn FnMut(&mut Status)| f(&mut status.lock().unwrap_or_else(|e| e.into_inner()));
    upd(&mut |s| *s = Status { total: indices.len(), ..Status::default() });
    let queue = Mutex::new(indices.iter().copied().collect::<VecDeque<usize>>());
    let stop: Mutex<Option<Fail>> = Mutex::new(None);
    let stopped = AtomicBool::new(false);
    let mut imported = Vec::new();
    let mut failures: Vec<Failure> = Vec::new();
    let fail_of = |index: usize, f: Fail| Failure { index, name: (h.name)(index), reason: f.reason, message: f.message };
    let halt = |f: Fail| {
        let mut g = stop.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_none() {
            *g = Some(f);
        }
        stopped.store(true, Ordering::SeqCst);
    };
    let (tx, rx) = mpsc::sync_channel::<(usize, Result<PathBuf, Fail>)>(STAGED_MAX);
    std::thread::scope(|sc| {
        for _ in 0..CONCURRENCY.min(indices.len().max(1)) {
            let tx = tx.clone();
            let (queue, halt, stopped) = (&queue, &halt, &stopped);
            sc.spawn(move || {
                loop {
                    if cancel.load(Ordering::SeqCst) || stopped.load(Ordering::SeqCst) {
                        return;
                    }
                    let Some(i) = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front() else { return };
                    if (h.free_space)().is_some_and(|f| f < SPACE_FLOOR) {
                        queue.lock().unwrap_or_else(|e| e.into_inner()).push_front(i);
                        halt(Fail::new(Reason::NoSpace, "Not enough free space on the device."));
                        return;
                    }
                    upd(&mut |s| s.fetching.push(i));
                    let mut attempt = 1;
                    let r = loop {
                        let r = (h.fetch)(i, attempt);
                        match &r {
                            Err(f) if f.reason.transient() && attempt < ATTEMPTS && !cancel.load(Ordering::SeqCst) => {
                                let mut left = backoff(attempt);
                                while !left.is_zero() && !cancel.load(Ordering::SeqCst) {
                                    let step = left.min(Duration::from_millis(200));
                                    (h.sleep)(step);
                                    left -= step;
                                }
                                attempt += 1;
                            }
                            _ => break r,
                        }
                    };
                    if let Err(f) = &r
                        && f.reason == Reason::NoSpace
                    {
                        halt(f.clone());
                    }
                    upd(&mut |s| s.fetching.retain(|&x| x != i));
                    if tx.send((i, r)).is_err() {
                        return;
                    }
                }
            });
        }
        drop(tx);
        for (i, r) in rx {
            match r {
                Ok(path) => {
                    let f = Fetched { index: i, path };
                    let res = if cancel.load(Ordering::SeqCst) {
                        Err(ImportFail { fail: Fail::new(Reason::Cancelled, "Cancelled."), stop: false })
                    } else if stopped.load(Ordering::SeqCst) {
                        let why = stop.lock().unwrap_or_else(|e| e.into_inner()).clone();
                        Err(ImportFail { fail: why.unwrap_or_else(|| Fail::new(Reason::Other, "Stopped.")), stop: false })
                    } else {
                        (h.import)(&f)
                    };
                    (h.wipe)(&f);
                    match res {
                        Ok(()) => {
                            imported.push(i);
                            upd(&mut |s| s.imported += 1);
                        }
                        Err(e) => {
                            if e.stop {
                                halt(e.fail.clone());
                            }
                            failures.push(fail_of(i, e.fail));
                            upd(&mut |s| s.failed += 1);
                        }
                    }
                }
                Err(f) => {
                    failures.push(fail_of(i, f));
                    upd(&mut |s| s.failed += 1);
                }
            }
        }
    });
    let why = stop.into_inner().unwrap_or_else(|e| e.into_inner());
    for i in queue.into_inner().unwrap_or_else(|e| e.into_inner()) {
        let f = match (&why, cancel.load(Ordering::SeqCst)) {
            (Some(f), false) => f.clone(),
            _ => Fail::new(Reason::Cancelled, "Cancelled."),
        };
        failures.push(fail_of(i, f));
    }
    failures.sort_by_key(|f| f.index);
    upd(&mut |s| s.failed = failures.len());
    Summary { imported, failures }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicUsize;

    struct World {
        staged: AtomicUsize,
        max_staged: AtomicUsize,
        in_flight: AtomicUsize,
        max_in_flight: AtomicUsize,
        attempts: Mutex<HashMap<usize, u32>>,
    }
    impl World {
        fn new() -> World {
            World { staged: 0.into(), max_staged: 0.into(), in_flight: 0.into(), max_in_flight: 0.into(), attempts: Mutex::new(HashMap::new()) }
        }
    }

    fn go(n: usize, plan: impl Fn(usize, u32) -> Result<(), Fail> + Sync, import_fail: Option<(usize, ImportFailKind)>, cancel_at: Option<usize>, space: Option<u64>) -> (Summary, World) {
        let w = World::new();
        let cancel = AtomicBool::new(false);
        let status = Mutex::new(Status::default());
        let imported_count = AtomicUsize::new(0);
        let fetch = |i: usize, a: u32| -> Result<PathBuf, Fail> {
            *w.attempts.lock().unwrap().entry(i).or_default() = a;
            let f = w.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            w.max_in_flight.fetch_max(f, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(2));
            w.in_flight.fetch_sub(1, Ordering::SeqCst);
            plan(i, a)?;
            let s = w.staged.fetch_add(1, Ordering::SeqCst) + 1;
            w.max_staged.fetch_max(s, Ordering::SeqCst);
            Ok(PathBuf::from(format!("/stage/{i}")))
        };
        let mut import = |f: &Fetched| -> Result<(), ImportFail> {
            std::thread::sleep(Duration::from_millis(3));
            let k = imported_count.fetch_add(1, Ordering::SeqCst) + 1;
            if cancel_at == Some(k) {
                cancel.store(true, Ordering::SeqCst);
            }
            match import_fail {
                Some((i, kind)) if i == f.index => Err(ImportFail {
                    fail: Fail::new(if kind == ImportFailKind::Full { Reason::NoSpace } else { Reason::Other }, "x"),
                    stop: kind == ImportFailKind::Full,
                }),
                _ => Ok(()),
            }
        };
        let wipe = |_: &Fetched| {
            w.staged.fetch_sub(1, Ordering::SeqCst);
        };
        let free = || space;
        let sleep = |_: Duration| {};
        let name = |i: usize| format!("IMG_{i:04}.HEIC");
        let idx: Vec<usize> = (0..n).collect();
        let s = run(&idx, Hooks { fetch: &fetch, import: &mut import, wipe: &wipe, free_space: &free, sleep: &sleep, name: &name }, &cancel, &status);
        assert_eq!(w.staged.load(Ordering::SeqCst), 0, "every staged copy wiped");
        assert_eq!(status.lock().unwrap().failed, s.failures.len());
        (s, w)
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ImportFailKind {
        One,
        Full,
    }

    fn accounted(n: usize, s: &Summary) {
        let mut all: Vec<usize> = s.imported.iter().copied().chain(s.failures.iter().map(|f| f.index)).collect();
        all.sort();
        assert_eq!(all, (0..n).collect::<Vec<_>>(), "every item imported or reported, once");
    }

    #[test]
    fn eight_hundred_items_bounded_and_all_imported() {
        let (s, w) = go(800, |_, _| Ok(()), None, None, Some(10 << 30));
        accounted(800, &s);
        assert_eq!(s.imported.len(), 800);
        assert!(w.max_in_flight.load(Ordering::SeqCst) <= CONCURRENCY);
        assert!(w.max_staged.load(Ordering::SeqCst) <= STAGED_MAX + CONCURRENCY + 1, "{}", w.max_staged.load(Ordering::SeqCst));
    }

    #[test]
    fn transient_failures_are_retried_permanent_ones_reported() {
        let (s, w) = go(
            40,
            |i, a| match i {
                5 => Err(Fail::new(Reason::Offline, "offline")),
                7 => Err(Fail::new(Reason::Unsupported, "no")),
                i if i % 10 == 0 && a <= 2 => Err(Fail::new(Reason::Icloud, "busy")),
                _ => Ok(()),
            },
            None,
            None,
            None,
        );
        accounted(40, &s);
        assert_eq!(s.failures.iter().map(|f| (f.index, f.reason)).collect::<Vec<_>>(), [(5, Reason::Offline), (7, Reason::Unsupported)]);
        assert_eq!(s.failures[0].name, "IMG_0005.HEIC");
        let at = w.attempts.lock().unwrap();
        assert_eq!(at[&5], ATTEMPTS);
        assert_eq!(at[&7], 1);
        assert_eq!(at[&10], 3);
    }

    #[test]
    fn cancel_reports_the_rest_as_cancelled() {
        let (s, _) = go(100, |_, _| Ok(()), None, Some(10), None);
        accounted(100, &s);
        assert!(s.imported.len() >= 10 && s.imported.len() < 100);
        assert!(s.failures.iter().all(|f| f.reason == Reason::Cancelled));
    }

    #[test]
    fn low_space_stops_fetching_and_reports() {
        let (s, _) = go(20, |_, _| Ok(()), None, None, Some(100 << 20));
        accounted(20, &s);
        assert!(s.imported.is_empty());
        assert!(s.failures.iter().all(|f| f.reason == Reason::NoSpace));
    }

    #[test]
    fn a_full_volume_stops_the_import_one_bad_item_does_not() {
        let (s, _) = go(30, |_, _| Ok(()), Some((3, ImportFailKind::One)), None, None);
        accounted(30, &s);
        assert_eq!(s.failures.len(), 1);
        let (s, _) = go(30, |_, _| Ok(()), Some((3, ImportFailKind::Full)), None, None);
        accounted(30, &s);
        assert!(s.failures.iter().all(|f| f.reason == Reason::NoSpace));
        assert!(s.imported.len() < 30);
    }

    #[test]
    fn reasons_from_native_codes() {
        assert_eq!(Reason::from_code("offline"), Reason::Offline);
        assert_eq!(Reason::from_code("whatever"), Reason::Other);
        assert!(Reason::Stalled.transient() && !Reason::Unsupported.transient() && !Reason::NoSpace.transient());
        assert!(backoff(1) < backoff(2) && backoff(2) < backoff(3));
    }
}
