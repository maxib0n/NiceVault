//! Diagnostic log for Windows, so a user can send what happened before a failure or crash:
//! `%LOCALAPPDATA%\NiceVault\logs\nicevault.log`.
//!
//! Records `log` messages (info and up from NiceVault's crates: mount steps, WinFsp
//! detection; warnings and up from everything else), panics (release builds abort on panic,
//! so the hook is the only trace) and native crashes (unhandled SEH exceptions: code, address,
//! module). No file names, volume names or contents are logged by the mount path.
//! Over 1 MiB at start, the file is moved to `nicevault.old.log`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX: u64 = 1 << 20;

/// `%LOCALAPPDATA%\NiceVault\logs\nicevault.log` (Windows only; elsewhere `None`).
pub fn path() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    Some(dirs::data_local_dir()?.join("NiceVault").join("logs").join("nicevault.log"))
}

fn open(p: &PathBuf) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(p).ok()
}

fn stamp() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let s = d.as_secs();
    let (h, m, sec) = ((s / 3600) % 24, (s / 60) % 60, s % 60);
    // days since 1970 -> civil date (UTC)
    let z = (s / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{sec:02}.{:03}Z", d.subsec_millis())
}

struct FileLog {
    file: Mutex<File>,
}

fn ours(target: &str) -> bool {
    target.starts_with("nv_") || target.starts_with("nicevault")
}

impl log::Log for FileLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= if ours(m.target()) { log::Level::Info } else { log::Level::Warn }
    }

    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!("{} {:5} {}: {}\n", stamp(), r.level(), r.target(), r.args());
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut f) = self.file.lock() {
            let _ = f.flush();
        }
    }
}

/// Append one line straight to the file (panic hook, crash filter: no logger lock).
fn emergency(msg: &str) {
    if let Some(p) = path()
        && let Some(mut f) = open(&p)
    {
        let _ = f.write_all(format!("{} FATAL {msg}\n", stamp()).as_bytes());
        let _ = f.flush();
    }
}

/// Install the logger, the panic hook and the crash filter. Windows only; never fails (no log
/// directory = no log).
pub fn init() {
    let Some(p) = path() else { return };
    let Some(dir) = p.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    if std::fs::metadata(&p).map(|m| m.len() > MAX).unwrap_or(false) {
        let _ = std::fs::rename(&p, p.with_file_name("nicevault.old.log"));
    }
    let Some(file) = open(&p) else { return };
    if log::set_boxed_logger(Box::new(FileLog { file: Mutex::new(file) })).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    log::info!(
        "NiceVault {} started ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("?").to_owned();
        emergency(&format!("panic on thread '{name}': {info}"));
        prev(info);
    }));
    #[cfg(windows)]
    seh::install();
}

#[cfg(windows)]
mod seh {
    use std::ffi::c_void;

    #[repr(C)]
    struct ExceptionRecord {
        code: u32,
        flags: u32,
        next: *mut ExceptionRecord,
        address: *mut c_void,
        n_params: u32,
        params: [usize; 15],
    }

    #[repr(C)]
    struct ExceptionPointers {
        record: *mut ExceptionRecord,
        context: *mut c_void,
    }

    type Filter = unsafe extern "system" fn(*const ExceptionPointers) -> i32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetUnhandledExceptionFilter(f: Option<Filter>) -> Option<Filter>;
        fn GetModuleHandleExW(flags: u32, name: *const c_void, module: *mut *mut c_void) -> i32;
        fn GetModuleFileNameW(module: *mut c_void, buf: *mut u16, len: u32) -> u32;
    }

    const FROM_ADDRESS: u32 = 0x4;
    const UNCHANGED_REFCOUNT: u32 = 0x2;

    /// Which module `addr` is in, and the offset into it.
    fn module_of(addr: *const c_void) -> String {
        let mut m = std::ptr::null_mut();
        // SAFETY: flags ask for no refcount change; buffers sized as given.
        unsafe {
            if GetModuleHandleExW(FROM_ADDRESS | UNCHANGED_REFCOUNT, addr, &mut m) == 0 || m.is_null() {
                return "unknown module".into();
            }
            let mut buf = [0u16; 512];
            let n = GetModuleFileNameW(m, buf.as_mut_ptr(), buf.len() as u32) as usize;
            let name = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
            let name = name.rsplit('\\').next().unwrap_or("").to_owned();
            format!("{name}+{:#x}", addr as usize - m as usize)
        }
    }

    unsafe extern "system" fn filter(p: *const ExceptionPointers) -> i32 {
        // SAFETY: the OS passes valid pointers to the filter.
        let (code, addr, info) = unsafe {
            match p.as_ref().and_then(|p| p.record.as_ref()) {
                Some(r) => (r.code, r.address, if r.n_params >= 2 { (r.params[0], r.params[1]) } else { (0, 0) }),
                None => (0, std::ptr::null_mut(), (0, 0)),
            }
        };
        let what = if code == 0xC000_0005 {
            format!(" ({} {:#x})", ["read of", "write to", "?", "?", "?", "?", "?", "?", "execute at"].get(info.0).unwrap_or(&"?"), info.1)
        } else {
            String::new()
        };
        super::emergency(&format!(
            "crash: exception {code:#010x}{what} at {addr:?} [{}] on thread '{}'",
            module_of(addr),
            std::thread::current().name().unwrap_or("?")
        ));
        0 // EXCEPTION_CONTINUE_SEARCH: the default handling (WER) still runs
    }

    pub fn install() {
        // SAFETY: plain registration of a process-wide filter.
        unsafe { SetUnhandledExceptionFilter(Some(filter)) };
    }
}
