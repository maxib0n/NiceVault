//! One NiceVault per user session. A second launch (login autostart, the desktop file, a double
//! click on a `.nvault` file, a path on the command line) never starts a second process: the
//! first one brings its window forward and, when a volume file came with it, shows that volume's
//! unlock screen (`tauri-plugin-single-instance`: D-Bus on Linux, a named mutex + window message on
//! Windows, a Unix socket on macOS). Portable and installed runs share the guard.
//!
//! The volume path is kept in `Launch` until the page asks for it (`launch_path_take`): at a cold
//! start the page is not listening yet; for a second launch the `open-path` event wakes it.
//!
//! Test opt-out (environment): `NICEVAULT_ALLOW_MULTI=1` registers no guard (the e2e harness
//! starts throw-away copies next to the user's real session).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

pub const OPT_OUT: &str = "NICEVAULT_ALLOW_MULTI";

/// The guard is on unless the test opt-out is set.
pub fn enabled() -> bool {
    std::env::var_os(OPT_OUT).is_none_or(|v| v.is_empty() || v == "0")
}

/// The first argument (the program excluded) that is an existing `.nvault` file, made absolute
/// against `cwd`. Flags (`--minimized`) and anything else are ignored.
pub fn volume_arg<I: IntoIterator<Item = S>, S: AsRef<str>>(args: I, cwd: &Path) -> Option<PathBuf> {
    args.into_iter().skip(1).find_map(|a| {
        let a = a.as_ref();
        if a.starts_with("--") {
            return None;
        }
        let p = Path::new(a);
        let p = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
        let is_nv = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nvault"));
        (is_nv && p.is_file()).then_some(p)
    })
}

/// Managed state: the volume path of the latest launch, until the page takes it.
#[derive(Default)]
pub struct Launch(Mutex<Option<String>>);

impl Launch {
    pub fn put(&self, path: PathBuf) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.to_string_lossy().into_owned());
    }
    pub fn take(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

/// A second launch reached this process: window forward, path (if any) to the page.
pub fn on_second(app: &AppHandle, args: Vec<String>, cwd: String) {
    crate::tray::show_window(app);
    if let (Some(path), Some(launch)) = (volume_arg(&args, Path::new(&cwd)), app.try_state::<Launch>()) {
        launch.put(path);
        let _ = app.emit("open-path", ());
    }
}

pub mod cmd {
    use super::Launch;

    /// The volume file this run was started with / the latest second launch brought, once.
    #[tauri::command]
    pub fn launch_path_take(launch: tauri::State<'_, Launch>) -> Option<String> {
        launch.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_volume_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("A.NVAULT"), b"x").unwrap();
        std::fs::write(d.path().join("notes.txt"), b"x").unwrap();
        let cwd = d.path();
        assert_eq!(volume_arg(["nicevault", "--minimized", "A.NVAULT"], cwd), Some(cwd.join("A.NVAULT")));
        assert_eq!(volume_arg(["nicevault", "--minimized"], cwd), None);
        assert_eq!(volume_arg(["nicevault", "notes.txt", "missing.nvault"], cwd), None);
        let abs = cwd.join("A.NVAULT");
        assert_eq!(volume_arg(["nicevault".into(), abs.to_string_lossy().into_owned()], Path::new("/")), Some(abs));
    }

    #[test]
    fn launch_hands_over_once() {
        let l = Launch::default();
        l.put(PathBuf::from("/a/b.nvault"));
        assert_eq!(l.take().as_deref(), Some("/a/b.nvault"));
        assert_eq!(l.take(), None);
    }
}
