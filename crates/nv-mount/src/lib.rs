use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

mod util;

#[cfg(target_os = "linux")]
mod fuse;

#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
mod nfs;

#[cfg(windows)]
mod winfsp_backend;

#[cfg(test)]
#[path = "../tests/common/memfs.rs"]
#[allow(dead_code)]
mod memfs;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Backend {
    Fuse,
    Nfs,
    WinFsp,
}

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum MountError {
    #[error("mounting is not available: {0}")]
    Unavailable(String),
    #[error("bad mountpoint: {0}")]
    Mountpoint(String),
    #[error("mount failed: {0}")]
    Failed(String),
    #[error("volume is busy: {0}")]
    Busy(String),
}

pub type Result<T, E = MountError> = std::result::Result<T, E>;

#[derive(Clone, Debug)]
pub struct MountOpts {
    pub volume_name: String,
    pub mountpoint: Option<PathBuf>,
    pub read_only: bool,
}

impl MountOpts {
    pub fn new(volume_name: impl Into<String>) -> Self {
        Self { volume_name: volume_name.into(), mountpoint: None, read_only: false }
    }
}

trait Unmounter: Send {
    fn unmount(self: Box<Self>) -> Result<()>;
}

pub struct MountHandle {
    mountpoint: PathBuf,
    backend: Backend,
    inner: Option<Box<dyn Unmounter>>,
}

impl std::fmt::Debug for MountHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MountHandle")
            .field("mountpoint", &self.mountpoint)
            .field("backend", &self.backend)
            .finish()
    }
}

impl MountHandle {
    pub fn mountpoint(&self) -> &Path {
        &self.mountpoint
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    pub fn unmount(mut self) -> Result<()> {
        match self.inner.take() {
            Some(u) => u.unmount(),
            None => Ok(()),
        }
    }
}

impl Drop for MountHandle {
    fn drop(&mut self) {
        if let Some(u) = self.inner.take()
            && let Err(e) = u.unmount()
        {
            log::warn!("unmount of {} failed: {e}", self.mountpoint.display());
        }
    }
}

pub fn available() -> Vec<Backend> {
    #[cfg_attr(not(any(target_os = "linux", target_os = "macos", windows)), allow(unused_mut))]
    let mut v = Vec::new();
    #[cfg(target_os = "linux")]
    if fuse::available() {
        v.push(Backend::Fuse);
    }
    #[cfg(target_os = "macos")]
    if nfs::available() {
        v.push(Backend::Nfs);
    }
    #[cfg(windows)]
    if winfsp_backend::available() {
        v.push(Backend::WinFsp);
    }
    v
}

pub fn unavailable_reason() -> Option<(&'static str, String)> {
    #[cfg(windows)]
    return winfsp_backend::unavailable_reason();
    #[cfg(target_os = "macos")]
    return nfs::unavailable_reason();
    #[cfg(not(any(windows, target_os = "macos")))]
    None
}

pub const WINFSP_MIN_VERSION: &str = "2.1";

pub fn mount(fs: Arc<dyn nv_fs::Fs>, opts: MountOpts) -> Result<MountHandle> {
    #[cfg(target_os = "linux")]
    {
        let (mountpoint, inner) = fuse::mount(fs, &opts)?;
        Ok(MountHandle { mountpoint, backend: Backend::Fuse, inner: Some(inner) })
    }
    #[cfg(target_os = "macos")]
    {
        let (mountpoint, inner) = nfs::mount(fs, &opts)?;
        Ok(MountHandle { mountpoint, backend: Backend::Nfs, inner: Some(inner) })
    }
    #[cfg(windows)]
    {
        let (mountpoint, inner) = winfsp_backend::mount(fs, &opts)?;
        Ok(MountHandle { mountpoint, backend: Backend::WinFsp, inner: Some(inner) })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (fs, opts);
        Err(MountError::Unavailable("unsupported operating system".into()))
    }
}
