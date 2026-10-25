//! Windows backend: WinFsp (user-mode file system driver), loaded at runtime.
//!
//! `winfsp-x64.dll` is never a load-time import, so the executable starts without WinFsp. We look
//! the DLL up in the registry (`HKLM\SOFTWARE\WOW6432Node\WinFsp\InstallDir`, written by the
//! WinFsp installer), `LoadLibraryW` it by full path, and only then call `winfsp_init`.
//! MSVC builds delay-load it; MinGW builds (the Linux cross build) bind every entry point with
//! `GetProcAddress` (`winfsp_sys::nv_bind_module`, NiceVault's patch of winfsp-sys), so a
//! missing or old DLL is a reason code (`winfsp_missing`, `winfsp_unsupported`), never a crash.
//!
//! NOTE for the final executable: Cargo does not forward `rustc-link-arg` from dependencies, so
//! the binary crate's `build.rs` must emit the delay-load flags itself (see `build.rs` here, or
//! call `winfsp::build::winfsp_link_delayload()`), otherwise the exe gets a hard import on the DLL.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use nv_fs::{Error, Fs, Kind, Node, NodeId, ROOT};
use windows_sys::Win32::Foundation as wf;
use winfsp::U16CStr;
use winfsp::constants::FspCleanupFlags;
use winfsp::filesystem::{
    DirBuffer, DirInfo, DirMarker, FileInfo, FileSecurity, FileSystemContext, OpenFileInfo, VolumeInfo, WideNameInfo,
};
use winfsp::host::{FileSystemHost, FineGuard, VolumeParams};
use winfsp::{FspError, FspInit};

use crate::util;
use crate::{MountError, MountOpts, Result, Unmounter};

type FspResult<T> = winfsp::Result<T>;

const FILE_DIRECTORY_FILE: u32 = 0x0000_0001;
const FILE_ATTRIBUTE_READONLY: u32 = 0x01;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x20;
const ALLOCATION_UNIT: u64 = 4096;
/// 100 ns intervals between 1601-01-01 and 1970-01-01.
const FILETIME_UNIX_EPOCH: i64 = 116_444_736_000_000_000;

fn nt(s: wf::NTSTATUS) -> FspError {
    FspError::NTSTATUS(s)
}

fn fsp_err(e: Error) -> FspError {
    nt(match e {
        Error::NotFound => wf::STATUS_OBJECT_NAME_NOT_FOUND,
        Error::Exists => wf::STATUS_OBJECT_NAME_COLLISION,
        Error::NotEmpty => wf::STATUS_DIRECTORY_NOT_EMPTY,
        Error::NotDir => wf::STATUS_NOT_A_DIRECTORY,
        Error::IsDir => wf::STATUS_FILE_IS_A_DIRECTORY,
        Error::InvalidName => wf::STATUS_OBJECT_NAME_INVALID,
        Error::Locked => wf::STATUS_ACCESS_DENIED,
        Error::Corrupt(_) | Error::Io(_) => wf::STATUS_IO_DEVICE_ERROR,
    })
}

fn ns_to_filetime(ns: i64) -> u64 {
    (ns / 100 + FILETIME_UNIX_EPOCH).max(0) as u64
}

fn filetime_to_ns(ft: u64) -> i64 {
    (ft as i64 - FILETIME_UNIX_EPOCH).saturating_mul(100)
}

// ---------------------------------------------------------------------------------------------
// DLL discovery

fn reg_install_dir() -> Option<PathBuf> {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let value: Vec<u16> = "InstallDir\0".encode_utf16().collect();
    for key in ["SOFTWARE\\WOW6432Node\\WinFsp\0", "SOFTWARE\\WinFsp\0"] {
        let key: Vec<u16> = key.encode_utf16().collect();
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: all pointers are valid for the sizes given.
        let rc = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if rc == 0 {
            let len = (size as usize / 2).saturating_sub(1).min(buf.len());
            let s = String::from_utf16_lossy(&buf[..len]);
            let s = s.trim_end_matches('\0');
            if !s.is_empty() {
                return Some(PathBuf::from(s));
            }
        }
    }
    None
}

fn dll_name() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "winfsp-x64.dll"
    } else if cfg!(target_arch = "aarch64") {
        "winfsp-a64.dll"
    } else {
        "winfsp-x86.dll"
    }
}

fn dll_path() -> Option<PathBuf> {
    let p = reg_install_dir()?.join("bin").join(dll_name());
    p.is_file().then_some(p)
}

/// The exports winfsp-rs calls on our mount path; without any of them the installed WinFsp is
/// too old (or not WinFsp) and mounting is refused with `winfsp_unsupported`.
#[cfg(target_env = "gnu")]
const REQUIRED: &[&str] = &[
    "FspFileSystemCreate",
    "FspFileSystemDelete",
    "FspFileSystemSetMountPoint",
    "FspFileSystemRemoveMountPoint",
    "FspFileSystemStartDispatcher",
    "FspFileSystemStopDispatcher",
    "FspFileSystemSetOperationGuardStrategyF",
    "FspFileSystemAcquireDirectoryBufferEx",
    "FspFileSystemFillDirectoryBuffer",
    "FspFileSystemReadDirectoryBuffer",
    "FspFileSystemReleaseDirectoryBuffer",
    "FspFileSystemDeleteDirectoryBuffer",
    "FspFileSystemAddDirInfo",
    "FspFileSystemFindReparsePoint",
    "FspFileSystemResolveReparsePoints",
    "FspFileSystemStopServiceIfNecessary",
    "FspNtStatusFromWin32",
];

/// File version of the DLL ("2.1.25156.0"), for messages and the log.
fn dll_version(path: &std::path::Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: buffers sized as the API reports; VerQueryValueW returns a pointer into `buf`.
    unsafe {
        let n = GetFileVersionInfoSizeW(wide.as_ptr(), std::ptr::null_mut());
        if n == 0 {
            return None;
        }
        let mut buf = vec![0u8; n as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, n, buf.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let mut p: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let root = [b'\\' as u16, 0];
        if VerQueryValueW(buf.as_ptr().cast(), root.as_ptr(), &mut p, &mut len) == 0
            || p.is_null()
            || (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>()
        {
            return None;
        }
        let fi = std::ptr::read_unaligned(p as *const VS_FIXEDFILEINFO);
        Some(format!(
            "{}.{}.{}.{}",
            fi.dwFileVersionMS >> 16,
            fi.dwFileVersionMS & 0xffff,
            fi.dwFileVersionLS >> 16,
            fi.dwFileVersionLS & 0xffff
        ))
    }
}

/// Why WinFsp cannot be used: (reason code, English detail).
/// Codes: `winfsp_missing` (not installed), `winfsp_unsupported` (too old / not WinFsp),
/// `winfsp_load_failed` (installed but LoadLibrary failed).
pub(crate) type Why = (&'static str, String);

/// Load the DLL by full path once and bind every entry point to it. Cached: later calls are
/// free. Never panics; every failure is a reason code.
fn init() -> std::result::Result<FspInit, Why> {
    static INIT: OnceLock<std::result::Result<FspInit, Why>> = OnceLock::new();
    INIT.get_or_init(|| {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
        let Some(path) = dll_path() else {
            log::info!("winfsp: not installed (no InstallDir in the registry or no bin\\{})", dll_name());
            return Err(("winfsp_missing", "WinFsp is not installed (winfsp.dev).".into()));
        };
        let version = dll_version(&path);
        log::info!("winfsp: found {} version {}", path.display(), version.as_deref().unwrap_or("?"));
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: NUL-terminated wide path. The module stays loaded for the process lifetime.
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            let e = std::io::Error::last_os_error();
            log::warn!("winfsp: LoadLibraryW failed: {e}");
            return Err(("winfsp_load_failed", format!("Cannot load {}: {e}", path.display())));
        }
        #[cfg(target_env = "gnu")]
        {
            // SAFETY: module is the loaded WinFsp DLL, never freed; nothing has called WinFsp yet
            // (we are inside the OnceLock that every WinFsp use goes through).
            let missing = unsafe { winfsp_sys::nv_bind_module(module) };
            let fatal: Vec<&str> = REQUIRED.iter().copied().filter(|r| missing.contains(r)).collect();
            if !missing.is_empty() {
                log::info!("winfsp: {} exports not in this DLL: {}", missing.len(), missing.join(", "));
            }
            if !fatal.is_empty() {
                log::warn!("winfsp: required exports missing: {}", fatal.join(", "));
                return Err((
                    "winfsp_unsupported",
                    format!(
                        "This WinFsp ({}) is too old: WinFsp {MIN_VERSION} or newer is needed.",
                        version.as_deref().unwrap_or("unknown version")
                    ),
                ));
            }
        }
        let _ = module;
        let init = winfsp::winfsp_init().map_err(|e| {
            log::warn!("winfsp: winfsp_init: {e:?}");
            ("winfsp_load_failed", format!("WinFsp initialisation failed: {e:?}"))
        })?;
        log::info!("winfsp: ready");
        Ok(init)
    })
    .clone()
}

/// What the UI asks for: the oldest WinFsp release whose API this build uses.
pub(crate) const MIN_VERSION: &str = crate::WINFSP_MIN_VERSION;

pub(crate) fn available() -> bool {
    init().is_ok()
}

pub(crate) fn unavailable_reason() -> Option<Why> {
    init().err()
}

// ---------------------------------------------------------------------------------------------
// Security descriptor: SYSTEM, Administrators and the mounting user get full access.

fn current_user_sid() -> Option<String> {
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // SAFETY: standard token query; every buffer is sized as reported by the API.
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut len = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
        let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len);
        wf::CloseHandle(token);
        if ok == 0 {
            return None;
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut s) == 0 {
            return None;
        }
        let n = (0..).take_while(|&i| *s.add(i) != 0).count();
        let out = String::from_utf16_lossy(std::slice::from_raw_parts(s, n));
        wf::LocalFree(s.cast());
        Some(out)
    }
}

/// No fallback: without the user's SID, or if the SDDL does not convert, the mount is refused.
fn build_security_descriptor() -> Result<Vec<u8>> {
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    let sid = current_user_sid().ok_or_else(|| MountError::Failed("cannot read the user SID".into()))?;
    let sddl = format!("O:{sid}G:{sid}D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;{sid})");
    let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut psd: *mut c_void = std::ptr::null_mut();
    let mut len = 0u32;
    // SAFETY: valid NUL-terminated SDDL; the returned buffer is copied then LocalFree'd.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(wide.as_ptr(), SDDL_REVISION_1, &mut psd, &mut len)
            == 0
            || len == 0
        {
            return Err(MountError::Failed("cannot build the security descriptor".into()));
        }
        let v = std::slice::from_raw_parts(psd as *const u8, len as usize).to_vec();
        wf::LocalFree(psd);
        Ok(v)
    }
}

// ---------------------------------------------------------------------------------------------
// FileSystemContext

pub(crate) struct Handle {
    id: NodeId,
    kind: Kind,
    delete_on_close: AtomicBool,
    dir_buffer: DirBuffer,
}

struct WinFs {
    fs: Arc<dyn Fs>,
    sd: Vec<u8>,
    label: String,
    read_only: bool,
}

/// `\a\b\c` -> ["a", "b", "c"].
fn components(name: &U16CStr) -> FspResult<Vec<String>> {
    let s = name.to_string().map_err(|_| nt(wf::STATUS_OBJECT_NAME_INVALID))?;
    Ok(s.split('\\').filter(|c| !c.is_empty()).map(str::to_string).collect())
}

impl WinFs {
    /// Exact match first, then a case-insensitive scan (Windows semantics on a case-sensitive store)
    /// that also matches the `host_name` shown by read_directory.
    fn child(&self, parent: NodeId, name: &str) -> std::result::Result<Node, Error> {
        match self.fs.lookup(parent, name) {
            Err(Error::NotFound) => {
                let lower = name.to_lowercase();
                self.fs.readdir(parent)?.into_iter().find(|n| n.name.to_lowercase() == lower || nv_fs::host_name(&n.name).to_lowercase() == lower).ok_or(Error::NotFound)
            }
            r => r,
        }
    }

    fn resolve(&self, name: &U16CStr) -> FspResult<Node> {
        let parts = components(name)?;
        let mut node = self.fs.getattr(ROOT).map_err(fsp_err)?;
        let last = parts.len().saturating_sub(1);
        for (i, p) in parts.iter().enumerate() {
            node = self.child(node.id, p).map_err(|e| match e {
                Error::NotFound if i < last => nt(wf::STATUS_OBJECT_PATH_NOT_FOUND),
                e => fsp_err(e),
            })?;
        }
        Ok(node)
    }

    /// (parent node, leaf name)
    fn resolve_parent(&self, name: &U16CStr) -> FspResult<(Node, String)> {
        let mut parts = components(name)?;
        let leaf = parts.pop().ok_or_else(|| nt(wf::STATUS_OBJECT_NAME_INVALID))?;
        let mut node = self.fs.getattr(ROOT).map_err(fsp_err)?;
        for p in &parts {
            node = self.child(node.id, p).map_err(|e| match e {
                Error::NotFound => nt(wf::STATUS_OBJECT_PATH_NOT_FOUND),
                e => fsp_err(e),
            })?;
        }
        if node.kind != Kind::Dir {
            return Err(nt(wf::STATUS_NOT_A_DIRECTORY));
        }
        Ok((node, leaf))
    }

    fn attributes(&self, n: &Node) -> u32 {
        match n.kind {
            Kind::Dir => FILE_ATTRIBUTE_DIRECTORY,
            Kind::File if self.read_only => FILE_ATTRIBUTE_ARCHIVE | FILE_ATTRIBUTE_READONLY,
            Kind::File => FILE_ATTRIBUTE_ARCHIVE,
        }
    }

    fn fill(&self, n: &Node, fi: &mut FileInfo) {
        let t = ns_to_filetime(n.mtime);
        fi.file_attributes = self.attributes(n);
        fi.reparse_tag = 0;
        fi.file_size = n.size;
        fi.allocation_size = n.size.div_ceil(ALLOCATION_UNIT) * ALLOCATION_UNIT;
        fi.creation_time = t;
        fi.last_access_time = t;
        fi.last_write_time = t;
        fi.change_time = t;
        fi.index_number = n.id;
        fi.hard_links = 0;
        fi.ea_size = 0;
    }

    fn refill(&self, id: NodeId, fi: &mut FileInfo) -> FspResult<()> {
        let n = self.fs.getattr(id).map_err(fsp_err)?;
        self.fill(&n, fi);
        Ok(())
    }

    fn handle(n: &Node) -> Handle {
        Handle {
            id: n.id,
            kind: n.kind,
            delete_on_close: AtomicBool::new(false),
            dir_buffer: DirBuffer::new(),
        }
    }
}

impl FileSystemContext for WinFs {
    type FileContext = Handle;

    fn get_security_by_name(
        &self,
        file_name: &U16CStr,
        security_descriptor: Option<&mut [c_void]>,
        _reparse_point_resolver: impl FnOnce(&U16CStr) -> Option<FileSecurity>,
    ) -> FspResult<FileSecurity> {
        let n = self.resolve(file_name)?;
        if let Some(buf) = security_descriptor {
            if buf.len() < self.sd.len() {
                return Err(nt(wf::STATUS_BUFFER_OVERFLOW));
            }
            // SAFETY: buf has at least sd.len() bytes.
            unsafe { std::ptr::copy_nonoverlapping(self.sd.as_ptr(), buf.as_mut_ptr().cast::<u8>(), self.sd.len()) };
        }
        Ok(FileSecurity { reparse: false, sz_security_descriptor: self.sd.len() as u64, attributes: self.attributes(&n) })
    }

    fn open(
        &self,
        file_name: &U16CStr,
        create_options: u32,
        _granted_access: u32,
        file_info: &mut OpenFileInfo,
    ) -> FspResult<Handle> {
        let n = self.resolve(file_name)?;
        if create_options & FILE_DIRECTORY_FILE != 0 && n.kind != Kind::Dir {
            return Err(nt(wf::STATUS_NOT_A_DIRECTORY));
        }
        self.fill(&n, file_info.as_mut());
        Ok(Self::handle(&n))
    }

    fn close(&self, _context: Handle) {}

    fn create(
        &self,
        file_name: &U16CStr,
        create_options: u32,
        _granted_access: u32,
        _file_attributes: u32,
        _security_descriptor: Option<&[c_void]>,
        _allocation_size: u64,
        _extra_buffer: Option<&[u8]>,
        _extra_buffer_is_reparse_point: bool,
        file_info: &mut OpenFileInfo,
    ) -> FspResult<Handle> {
        if self.read_only {
            return Err(nt(wf::STATUS_MEDIA_WRITE_PROTECTED));
        }
        let (parent, leaf) = self.resolve_parent(file_name)?;
        if self.child(parent.id, &leaf).is_ok() {
            return Err(nt(wf::STATUS_OBJECT_NAME_COLLISION));
        }
        let kind = if create_options & FILE_DIRECTORY_FILE != 0 { Kind::Dir } else { Kind::File };
        let n = self.fs.create(parent.id, &leaf, kind).map_err(fsp_err)?;
        self.fill(&n, file_info.as_mut());
        Ok(Self::handle(&n))
    }

    fn cleanup(&self, context: &Handle, _file_name: Option<&U16CStr>, flags: u32) {
        if FspCleanupFlags::FspCleanupDelete.is_flagged(flags) {
            // set_delete already verified a directory is empty.
            if let Err(e) = self.fs.remove(context.id) {
                log::warn!("delete on close failed: {e}");
            }
        }
        // Closing a handle is not a durability point (FlushFileBuffers is, see `flush`); the Fs
        // makes everything durable within a few seconds anyway.
    }

    fn flush(&self, context: Option<&Handle>, file_info: &mut FileInfo) -> FspResult<()> {
        self.fs.flush().map_err(fsp_err)?;
        if let Some(c) = context {
            self.refill(c.id, file_info)?;
        }
        Ok(())
    }

    fn get_file_info(&self, context: &Handle, file_info: &mut FileInfo) -> FspResult<()> {
        self.refill(context.id, file_info)
    }

    fn get_security(&self, _context: &Handle, security_descriptor: Option<&mut [c_void]>) -> FspResult<u64> {
        if let Some(buf) = security_descriptor {
            if buf.len() < self.sd.len() {
                return Err(nt(wf::STATUS_BUFFER_OVERFLOW));
            }
            // SAFETY: buf has at least sd.len() bytes.
            unsafe { std::ptr::copy_nonoverlapping(self.sd.as_ptr(), buf.as_mut_ptr().cast::<u8>(), self.sd.len()) };
        }
        Ok(self.sd.len() as u64)
    }

    fn overwrite(
        &self,
        context: &Handle,
        _file_attributes: u32,
        _replace_file_attributes: bool,
        _allocation_size: u64,
        _extra_buffer: Option<&[u8]>,
        file_info: &mut FileInfo,
    ) -> FspResult<()> {
        self.fs.set_size(context.id, 0).map_err(fsp_err)?;
        self.refill(context.id, file_info)
    }

    fn read_directory(
        &self,
        context: &Handle,
        _pattern: Option<&U16CStr>,
        marker: DirMarker,
        buffer: &mut [u8],
    ) -> FspResult<u32> {
        if context.kind != Kind::Dir {
            return Err(nt(wf::STATUS_NOT_A_DIRECTORY));
        }
        // Fill the (sorted, marker-searchable) buffer on the first call of an enumeration.
        if let Ok(lock) = context.dir_buffer.acquire(marker.is_none(), None) {
            let me = self.fs.getattr(context.id).map_err(fsp_err)?;
            let mut entries: Vec<(String, Node)> = Vec::new();
            if me.id != ROOT {
                entries.push((".".into(), me.clone()));
                let parent = self.fs.getattr(me.parent).map_err(fsp_err)?;
                entries.push(("..".into(), parent));
            }
            for c in self.fs.readdir(context.id).map_err(fsp_err)? {
                entries.push((nv_fs::host_name(&c.name), c));
            }
            let mut di: DirInfo<255> = DirInfo::new();
            for (name, n) in &entries {
                di.reset();
                self.fill(n, di.file_info_mut());
                if di.set_name(name).is_err() {
                    continue; // longer than 255 UTF-16 units: cannot be represented
                }
                lock.write(&mut di)?;
            }
        }
        Ok(context.dir_buffer.read(marker, buffer))
    }

    fn rename(
        &self,
        context: &Handle,
        _file_name: &U16CStr,
        new_file_name: &U16CStr,
        replace_if_exists: bool,
    ) -> FspResult<()> {
        let (parent, leaf) = self.resolve_parent(new_file_name)?;
        match self.fs.lookup(parent.id, &leaf) {
            Ok(t) if t.id == context.id => {}
            Ok(t) => {
                if !replace_if_exists || t.kind == Kind::Dir {
                    return Err(nt(wf::STATUS_OBJECT_NAME_COLLISION));
                }
            }
            Err(Error::NotFound) => {}
            Err(e) => return Err(fsp_err(e)),
        }
        self.fs.rename(context.id, parent.id, &leaf).map_err(|e| match e {
            Error::Exists => nt(wf::STATUS_OBJECT_NAME_COLLISION),
            e => fsp_err(e),
        })?;
        Ok(())
    }

    fn set_basic_info(
        &self,
        context: &Handle,
        _file_attributes: u32,
        _creation_time: u64,
        _last_access_time: u64,
        last_write_time: u64,
        _last_change_time: u64,
        file_info: &mut FileInfo,
    ) -> FspResult<()> {
        if last_write_time != 0 && last_write_time != u64::MAX {
            self.fs.set_mtime(context.id, filetime_to_ns(last_write_time)).map_err(fsp_err)?;
        }
        self.refill(context.id, file_info)
    }

    fn set_delete(&self, context: &Handle, _file_name: &U16CStr, delete_file: bool) -> FspResult<()> {
        if delete_file && context.kind == Kind::Dir && !self.fs.readdir(context.id).map_err(fsp_err)?.is_empty() {
            return Err(nt(wf::STATUS_DIRECTORY_NOT_EMPTY));
        }
        context.delete_on_close.store(delete_file, Ordering::Relaxed);
        Ok(())
    }

    fn set_file_size(
        &self,
        context: &Handle,
        new_size: u64,
        set_allocation_size: bool,
        file_info: &mut FileInfo,
    ) -> FspResult<()> {
        if set_allocation_size {
            // Allocation is virtual; only honour a shrink below the current end of file.
            let n = self.fs.getattr(context.id).map_err(fsp_err)?;
            if new_size < n.size {
                self.fs.set_size(context.id, new_size).map_err(fsp_err)?;
            }
        } else {
            self.fs.set_size(context.id, new_size).map_err(fsp_err)?;
        }
        self.refill(context.id, file_info)
    }

    fn read(&self, context: &Handle, buffer: &mut [u8], offset: u64) -> FspResult<u32> {
        let n = self.fs.getattr(context.id).map_err(fsp_err)?;
        if offset >= n.size {
            return Err(nt(wf::STATUS_END_OF_FILE));
        }
        let got = self.fs.read(context.id, offset, buffer).map_err(fsp_err)?;
        Ok(got as u32)
    }

    fn write(
        &self,
        context: &Handle,
        buffer: &[u8],
        offset: u64,
        write_to_eof: bool,
        constrained_io: bool,
        file_info: &mut FileInfo,
    ) -> FspResult<u32> {
        let size = self.fs.getattr(context.id).map_err(fsp_err)?.size;
        let offset = if write_to_eof { size } else { offset };
        let data = if constrained_io {
            // Paging I/O must not extend the file.
            if offset >= size {
                self.refill(context.id, file_info)?;
                return Ok(0);
            }
            &buffer[..buffer.len().min((size - offset) as usize)]
        } else {
            buffer
        };
        let n = self.fs.write(context.id, offset, data).map_err(fsp_err)?;
        self.refill(context.id, file_info)?;
        Ok(n as u32)
    }

    fn get_volume_info(&self, out: &mut VolumeInfo) -> FspResult<()> {
        let s = self.fs.stats().map_err(fsp_err)?;
        out.total_size = s.total_bytes;
        out.free_size = s.free_bytes;
        out.set_volume_label(&self.label);
        Ok(())
    }

    fn set_volume_label(&self, _volume_label: &U16CStr, out: &mut VolumeInfo) -> FspResult<()> {
        // The label is the vault name; renaming the drive is not supported.
        self.get_volume_info(out)
    }
}

// ---------------------------------------------------------------------------------------------
// Mount

/// First unused drive letter, from Z: down to D:.
fn free_drive() -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;
    // SAFETY: no preconditions.
    let used = unsafe { GetLogicalDrives() };
    (3..26u32).rev().find(|i| used & (1 << i) == 0).map(|i| format!("{}:", (b'A' + i as u8) as char))
}

struct WinMount {
    host: FileSystemHost<WinFs, FineGuard>,
    fs: Arc<dyn Fs>,
}

impl Unmounter for WinMount {
    fn unmount(self: Box<Self>) -> Result<()> {
        let WinMount { mut host, fs } = *self;
        log::info!("winfsp unmount");
        host.unmount();
        host.stop();
        drop(host);
        let _ = fs.flush();
        Ok(())
    }
}

pub(crate) fn mount(fs: Arc<dyn Fs>, opts: &MountOpts) -> Result<(PathBuf, Box<dyn Unmounter>)> {
    log::info!("winfsp mount: start (read_only={})", opts.read_only);
    let _init = init().map_err(|(code, msg)| MountError::Unavailable(format!("{code}: {msg}")))?;
    let label = util::sanitize_name(&opts.volume_name);
    let mut vp = VolumeParams::new();
    vp.sector_size(4096)
        .sectors_per_allocation_unit(1)
        .max_component_length(255)
        .volume_creation_time(ns_to_filetime(util::now_ns()))
        .volume_serial_number((util::now_ns() as u64 ^ (util::now_ns() as u64 >> 32)) as u32)
        .file_info_timeout(1000)
        .case_sensitive_search(false)
        .case_preserved_names(true)
        .unicode_on_disk(true)
        .persistent_acls(false)
        .post_cleanup_when_modified_only(true)
        .read_only_volume(opts.read_only)
        .filesystem_name("NiceVault");
    log::info!("winfsp mount: security descriptor");
    let sd = build_security_descriptor().inspect_err(|e| log::warn!("winfsp mount: {e}"))?;
    let ctx = WinFs { fs: fs.clone(), sd, label, read_only: opts.read_only };
    log::info!("winfsp mount: FspFileSystemCreate");
    let mut host = FileSystemHost::<WinFs, FineGuard>::new(vp, ctx).map_err(|e| {
        log::warn!("winfsp mount: FspFileSystemCreate failed: {e:?}");
        MountError::Failed(format!("FspFileSystemCreate: {e:?}"))
    })?;
    let target = match &opts.mountpoint {
        Some(p) => p.to_string_lossy().into_owned(),
        None => free_drive().ok_or_else(|| MountError::Mountpoint("no free drive letter".into()))?,
    };
    log::info!("winfsp mount: FspFileSystemSetMountPoint {target}");
    host.mount(target.clone()).map_err(|e| {
        log::warn!("winfsp mount: set mount point {target} failed: {e:?}");
        MountError::Failed(format!("mount {target}: {e:?}"))
    })?;
    log::info!("winfsp mount: FspFileSystemStartDispatcher");
    if let Err(e) = host.start() {
        log::warn!("winfsp mount: start dispatcher failed: {e:?}");
        host.unmount();
        return Err(MountError::Failed(format!("start dispatcher: {e:?}")));
    }
    log::info!("winfsp mount: mounted on {target}");
    let path = if target.len() == 2 && target.ends_with(':') { PathBuf::from(format!("{target}\\")) } else { PathBuf::from(&target) };
    Ok((path, Box::new(WinMount { host, fs })))
}
